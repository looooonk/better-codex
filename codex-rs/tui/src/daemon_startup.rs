//! Shared-daemon startup policy and bounded compatibility checks.

use super::AppServerTarget;
use super::Cli;
use super::Config;
use super::LoaderOverrides;
use super::RemoteAppServerEndpoint;
use super::connect_remote_app_server;
use super::loader_overrides_are_default;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ExperimentalFeatureListParams;
use codex_app_server_protocol::ExperimentalFeatureListResponse;
use codex_app_server_protocol::RequestId;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::collections::BTreeMap;
use std::time::Duration;

const SERVER_FEATURES: [Feature; 4] = [
    Feature::ApiKeyModelDiscovery,
    Feature::CodeModeHost,
    Feature::AuthElicitation,
    Feature::McpOAuthRefreshCoordination,
];
const FAILURE_HINT: &str = "Rerun with --no-daemon to work without the shared background server.";

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
    } else if cli.agents_overview {
        None
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
                !features.is_empty()
                    && features
                        .iter()
                        .all(|(name, value)| allowed_feature(name) && value.is_bool())
            }),
            _ => key.strip_prefix("features.").is_some_and(allowed_feature) && value.is_bool(),
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

fn allowed_feature(name: &str) -> bool {
    matches!(
        name,
        // Client gates and per-thread settings already forwarded in thread requests.
        "daemon_auto_start" | "worktrees" | "transcript_v2" | "realtime_conversation" | "standalone_web_search"
        // Shared services and threadless MCP operations need daemon compatibility checks.
        | "api_key_model_discovery" | "code_mode_host" | "auth_elicitation"
        | "mcp_oauth_refresh_coordination"
        // Removed flags still passed by older launch scripts.
        | "remote_models" | "request_rule" | "responses_websockets_v2"
        | "workspace_owner_usage_nudge" | "tool_search_always_defer_mcp_tools"
        | "remote_compaction_v2" | "multi_agent_mode"
    )
}

pub(super) fn server_features(overrides: &[(String, toml::Value)]) -> BTreeMap<String, bool> {
    let layer = codex_config::build_cli_overrides_layer(overrides);
    SERVER_FEATURES
        .into_iter()
        .filter_map(|feature| {
            let name = feature.key();
            let enabled = layer.get("features")?.get(name)?.as_bool()?;
            Some((name.to_string(), enabled))
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
#[error("Cannot use the shared background server: {reason}.\n{FAILURE_HINT}")]
pub(crate) struct CompatibilityError {
    pub(crate) reason: String,
    pub(crate) restart_features: Option<BTreeMap<String, bool>>,
}

pub(super) struct DaemonLaunch {
    pub(super) target: AppServerTarget,
    pub(super) warning: Option<String>,
}

pub(super) async fn start(
    config: &Config,
    overrides: &[(String, toml::Value)],
) -> std::io::Result<DaemonLaunch> {
    let mut features = server_features(overrides);
    features.retain(|_, enabled| *enabled);
    let output = codex_app_server_daemon::start_with_features(&features)
        .await
        .map_err(|error| std::io::Error::other(format!("{error:#}\n{FAILURE_HINT}")))?;
    let endpoint = RemoteAppServerEndpoint::UnixSocket {
        socket_path: AbsolutePathBuf::from_absolute_path(output.socket_path)?,
    };
    if let Err(issue) = check(&endpoint, config).await {
        match crate::app_shell::recover_daemon(config, &issue, output.backend.is_some()).await? {
            crate::app_shell::DaemonRecovery::Independent => {
                return Ok(DaemonLaunch {
                    target: AppServerTarget::Embedded,
                    warning: Some(format!(
                        "Running without the shared background server: {}.",
                        issue.reason
                    )),
                });
            }
            crate::app_shell::DaemonRecovery::Restart => {
                let features = issue
                    .restart_features
                    .as_ref()
                    .ok_or_else(|| std::io::Error::other(issue.to_string()))?;
                codex_app_server_daemon::restart_with_features(features)
                    .await
                    .map_err(std::io::Error::other)?;
                check(&endpoint, config)
                    .await
                    .map_err(std::io::Error::other)?;
            }
            crate::app_shell::DaemonRecovery::Cancel => return Err(std::io::Error::other(issue)),
        }
    }
    Ok(DaemonLaunch {
        target: AppServerTarget::LocalDaemon { endpoint },
        warning: None,
    })
}

async fn check(
    endpoint: &RemoteAppServerEndpoint,
    config: &Config,
) -> Result<(), CompatibilityError> {
    let mut restart_features = None;
    let check = async {
        if !config.features.enabled(Feature::CodeModeHost)
            && config.code_mode.disable_in_process_fallback
        {
            return Err("code-mode fallback policy requires an independent session".to_string());
        }
        let client = connect_remote_app_server(endpoint.clone())
            .await
            .map_err(|error| error.to_string())?;
        let result = async {
            let mut cursor = None;
            let mut enabled = BTreeMap::new();
            for _ in 0..10 {
                let response: ExperimentalFeatureListResponse = client
                    .request_handle()
                    .request_typed(ClientRequest::ExperimentalFeatureList {
                        request_id: RequestId::String("better-codex-daemon-features".to_string()),
                        params: ExperimentalFeatureListParams {
                            cursor,
                            limit: Some(100),
                            thread_id: None,
                        },
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                if response.data.len() > 100 {
                    return Err("daemon feature page exceeds requested limit".to_string());
                }
                enabled.extend(
                    response
                        .data
                        .into_iter()
                        .map(|feature| (feature.name, feature.enabled)),
                );
                cursor = response.next_cursor;
                if cursor.is_none() {
                    for feature in SERVER_FEATURES {
                        let desired = config.features.enabled(feature);
                        if enabled.get(feature.key()).copied().unwrap_or(false) != desired {
                            restart_features = Some(
                                SERVER_FEATURES
                                    .into_iter()
                                    .map(|feature| {
                                        (
                                            feature.key().to_string(),
                                            config.features.enabled(feature),
                                        )
                                    })
                                    .collect(),
                            );
                            let state = if desired { "enabled" } else { "disabled" };
                            return Err(format!(
                                "This session requires {} to be {state}",
                                feature.key()
                            ));
                        }
                    }
                    return Ok(());
                }
            }
            Err("daemon feature discovery exceeded ten pages".to_string())
        }
        .await;
        let _ = client.shutdown().await;
        result
    };
    tokio::time::timeout(Duration::from_secs(/*secs*/ 5), check)
        .await
        .map_err(|_| "daemon feature discovery timed out".to_string())
        .and_then(std::convert::identity)
        .map_err(|reason| CompatibilityError {
            reason,
            restart_features,
        })
}

#[cfg(test)]
#[path = "daemon_startup_tests.rs"]
mod tests;
