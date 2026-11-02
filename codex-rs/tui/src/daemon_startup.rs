//! Optional background servers never prevent an independent session from starting.

use super::AppServerTarget;
use super::Cli;
use super::Config;
use super::LoaderOverrides;
use super::RemoteAppServerEndpoint;
use super::connect_remote_app_server;
use super::loader_overrides_are_default;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigRequirementsReadResponse;
use codex_app_server_protocol::ExperimentalFeatureStage;
use codex_app_server_protocol::RequestId;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::collections::BTreeMap;
use std::time::Duration;

pub(super) const FAILURE_HINT: &str = "To work without the background server, rerun the same command with --no-daemon (including resume or fork and its arguments).";
pub(super) const WSL_DRVFS_EXCLUSION: &str = "a Windows-mounted WSL CODEX_HOME (DrvFS/9p)";

/// Returns whether `codex_home` is on a Windows-mounted WSL filesystem.
pub fn uses_wsl_drvfs(codex_home: &std::path::Path) -> bool {
    // The managed daemon writes an executable and Unix-style state beneath CODEX_HOME.
    // DrvFS does not reliably support the required permission semantics, so starting it
    // there can fail before the TUI opens.
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;

        if !codex_utils_path::is_wsl() && std::env::var_os("WSL_INTEROP").is_none() {
            return false;
        }
        let Some(directory) = codex_home
            .ancestors()
            .find_map(|path| std::fs::File::open(path).ok())
        else {
            return false;
        };
        let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
        if unsafe { libc::fstatfs(directory.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
            return false;
        }
        unsafe { stats.assume_init() }.f_type == 0x0102_1997
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = codex_home;
        false
    }
}

#[cfg(any(windows, test))]
pub(super) const ELEVATED_LAUNCH_WARNING: &str = "Running as administrator: shared background server disabled. To enable it, restart Codex in a terminal without administrator permissions.";

#[derive(Debug, thiserror::Error)]
#[error("Cannot use the shared background server: {reason}.\n{FAILURE_HINT}")]
pub(crate) struct CompatibilityError {
    pub(crate) reason: String,
}

pub(super) fn exclusion(
    cli: &Cli,
    cli_kv_overrides: &[(String, toml::Value)],
    loader_overrides: &LoaderOverrides,
    workload_identity_selected: bool,
    exec_server_url: Option<&std::ffi::OsStr>,
) -> Option<&'static str> {
    if cli.no_daemon {
        Some("--no-daemon")
    } else if cli.oss {
        Some("--oss")
    } else if workload_identity_selected {
        Some("workload identity")
    } else if exec_server_url.is_some() {
        Some("executor selection (CODEX_EXEC_SERVER_URL)")
    } else if cli.config_profile_v2.is_some() {
        Some("--profile")
    } else {
        config_exclusion(
            cli_kv_overrides,
            loader_overrides,
            cli.strict_config,
            cli.bypass_hook_trust,
        )
    }
}

pub(super) fn config_exclusion(
    cli_kv_overrides: &[(String, toml::Value)],
    loader_overrides: &LoaderOverrides,
    strict_config: bool,
    bypass_hook_trust: bool,
) -> Option<&'static str> {
    if !cli_kv_overrides
        .iter()
        .all(|(key, value)| match key.as_str() {
            "suppress_unstable_features_warning" | "tui.fullscreen_transcript" => value.is_bool(),
            "tui" => value.as_table().is_some_and(|tui| {
                tui.len() == 1
                    && tui
                        .get("fullscreen_transcript")
                        .is_some_and(toml::Value::is_bool)
            }),
            "features" => value.as_table().is_some_and(|features| {
                !features.is_empty() && features.values().all(toml::Value::is_bool)
            }),
            _ => {
                key.strip_prefix("features.")
                    .is_some_and(|name| !name.contains('.'))
                    && value.is_bool()
            }
        })
    {
        Some("command-line configuration overrides (-c, --enable, --disable, or --search)")
    } else if !loader_overrides_are_default(loader_overrides) {
        Some("custom configuration loader")
    } else if strict_config {
        Some("--strict-config")
    } else if bypass_hook_trust {
        Some("--dangerously-bypass-hook-trust")
    } else {
        None
    }
}

pub(super) struct DaemonLaunch {
    pub(super) target: AppServerTarget,
    pub(super) warning: Option<String>,
}

impl DaemonLaunch {
    fn independent(reason: impl std::fmt::Display) -> Self {
        Self {
            target: AppServerTarget::Embedded,
            warning: Some(format!(
                "Running without the shared background server: {reason}."
            )),
        }
    }
}

pub(super) fn server_features(overrides: &[(String, toml::Value)]) -> BTreeMap<String, bool> {
    let layer = codex_config::build_cli_overrides_layer(overrides);
    let Some(features) = layer
        .get("features")
        .and_then(toml::Value::as_table)
    else {
        return BTreeMap::new();
    };
    features
        .iter()
        .filter_map(|(name, value)| {
            if name == "imagegenext" && features.contains_key("image_generation") {
                return None;
            }
            let name = match codex_features::feature_for_key(name) {
                Some(feature) if feature.stage() == codex_features::Stage::Removed => return None,
                Some(feature) => feature.key(),
                None => name,
            };
            Some((name.to_string(), value.as_bool()?))
        })
        .collect()
}

pub(super) async fn start(
    config: &Config,
    overrides: &[(String, toml::Value)],
) -> std::io::Result<DaemonLaunch> {
    let features = server_features(overrides);
    let output = match codex_app_server_daemon::start_with_features(&features).await {
        Ok(output) => output,
        Err(error) => return Ok(DaemonLaunch::independent(format!("{error:#}"))),
    };
    let endpoint = RemoteAppServerEndpoint::UnixSocket {
        socket_path: AbsolutePathBuf::from_absolute_path(output.socket_path)?,
    };
    Ok(validate_existing(endpoint, config, overrides).await)
}

pub(super) async fn validate_existing(
    endpoint: RemoteAppServerEndpoint,
    config: &Config,
    overrides: &[(String, toml::Value)],
) -> DaemonLaunch {
    if let Err(issue) = check(&endpoint, config, overrides).await {
        return DaemonLaunch::independent(issue.reason);
    }
    DaemonLaunch {
        target: AppServerTarget::LocalDaemon { endpoint },
        warning: None,
    }
}

pub(super) async fn check(
    endpoint: &RemoteAppServerEndpoint,
    config: &Config,
    cli_kv_overrides: &[(String, toml::Value)],
) -> Result<(), CompatibilityError> {
    let mut requested = server_features(cli_kv_overrides);
    let check = async {
        let client = connect_remote_app_server(endpoint.clone())
            .await
            .map_err(|error| format!("could not connect to the background server: {error}"))?;
        let result = async {
            if let AppServerClient::Remote(remote) = &client
                && remote.server_version() != Some(codex_build_info::CODEX_BACKEND_COMPAT_VERSION)
            {
                return Err(format!(
                    "background server protocol {} differs from this client's {}",
                    remote.server_version().unwrap_or("unknown"),
                    codex_build_info::CODEX_BACKEND_COMPAT_VERSION,
                ));
            }
            // Both legacy managed config and current requirements override CLI flags.
            let response: serde_json::Value = client
                .request_handle()
                .request_typed(ClientRequest::ConfigRead {
                    request_id: RequestId::String("better-codex-daemon-config".into()),
                    params: ConfigReadParams {
                        include_layers: true,
                        cwd: Some(config.cwd.display().to_string()),
                    },
                })
                .await
                .map_err(|error| format!("could not read daemon configuration: {error}"))?;
            let layers = response
                .get("layers")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            for layer in layers.iter().filter(|layer| {
                layer["disabledReason"].is_null()
                    && matches!(
                        layer["name"]["type"].as_str(),
                        Some("legacyManagedConfigTomlFromFile" | "legacyManagedConfigTomlFromMdm")
                    )
            }) {
                requested.retain(|name, _| layer["config"]["features"].get(name).is_none());
            }
            match client
                .request_handle()
                .request_typed::<ConfigRequirementsReadResponse>(
                    ClientRequest::ConfigRequirementsRead {
                        request_id: RequestId::String("tui-daemon-requirements".to_string()),
                        params: None,
                    },
                )
                .await
            {
                Ok(response) => {
                    if let Some(required) =
                        response.requirements.and_then(|r| r.feature_requirements)
                    {
                        requested.retain(|name, _| !required.contains_key(name));
                    }
                }
                Err(TypedRequestError::Server { source, .. })
                    if source.code == -32601
                        || source.code == -32600
                            && source.message.contains("configRequirements/read")
                            && (source.message.contains("unknown variant")
                                || source.message.contains("unknown method")) => {}
                Err(_) => {
                    return Err("could not check daemon configuration requirements".to_string());
                }
            }
            let (tx, rx) = tokio::sync::oneshot::channel();
            crate::experimental_features::fetch(
                client.request_handle(),
                /*thread_id*/ None,
                "tui-daemon-features",
                tx,
            );
            let result = rx.await;
            let features =
                result.map_err(|_| "daemon feature check was interrupted".to_string())??;
            requested.retain(|name, enabled| {
                match features.iter().find(|feature| feature.name == *name) {
                    Some(feature) => {
                        feature.stage != ExperimentalFeatureStage::Removed
                            && feature.enabled != *enabled
                    }
                    None => *enabled,
                }
            });
            if let Some((name, enabled)) = requested.first_key_value() {
                let state = if *enabled { "enabled" } else { "disabled" };
                let reason = format!("This session requires {name} to be {state}");
                return Err(reason);
            }
            Ok::<(), String>(())
        }
        .await;
        let _ = client.shutdown().await;
        result
    };

    tokio::time::timeout(Duration::from_secs(/*secs*/ 5), check)
        .await
        .map_err(|_| "background server compatibility check timed out".to_string())
        .and_then(std::convert::identity)
        .map_err(|reason| CompatibilityError { reason })
}

#[cfg(test)]
#[path = "daemon_startup_tests.rs"]
mod tests;
