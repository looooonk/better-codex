use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::time::Duration;

use codex_core::config::Config;
use codex_http_client::ClientRouteClass;
use codex_http_client::RouteAwareClientPool;
use http::Method;
use serde::Deserialize;
#[cfg(target_os = "macos")]
use url::Url;

use crate::doctor::CheckStatus;
use crate::doctor::DoctorCheck;
use crate::doctor::DoctorIssue;
use crate::doctor::desktop::platform::InstalledApp;
use crate::doctor::network;

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const DESKTOP_UPDATE_URL: &str = "https://persistent.oaistatic.com/codex-app-prod/appcast-x64.xml";
#[cfg(all(target_os = "macos", not(target_arch = "x86_64")))]
const DESKTOP_UPDATE_URL: &str = "https://persistent.oaistatic.com/codex-app-prod/appcast.xml";
#[cfg(target_os = "macos")]
const BACKEND_DESKTOP_UPDATE_URL: &str = "https://chatgpt.com/backend-api/wham/app/appcast";
#[cfg(target_os = "windows")]
const DESKTOP_UPDATE_URL: &str =
    "https://persistent.oaistatic.com/codex-app-prod/windows-store-update.json";

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) async fn append_desktop_update(
    checks: &mut [DoctorCheck],
    config: Option<&Config>,
    application: &InstalledApp,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Some(build) = latest_macos_staged_build(
            &home
                .join("Library/Caches")
                .join(application.identity)
                .join("org.sparkle-project.Sparkle/Installation"),
            application.build,
        )
        .await
        && let Some(update) = checks.iter_mut().find(|check| check.id == "updates.status")
    {
        update.details.extend([
            "desktop update status: ready to install".to_string(),
            format!("desktop latest build: {build}"),
            format!("desktop application: {}", application.identity),
        ]);
    }

    let Some(config) = config else {
        return;
    };
    let Some(reachability_index) = checks
        .iter()
        .position(|check| check.id == "network.provider_reachability")
    else {
        return;
    };
    #[cfg(target_os = "macos")]
    let desktop_update_url = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| {
            macos_desktop_update_url(&home, application, &os_info::get().version().to_string())
        })
        .unwrap_or_else(|| DESKTOP_UPDATE_URL.to_string());
    #[cfg(target_os = "windows")]
    let desktop_update_url = DESKTOP_UPDATE_URL;
    #[cfg(target_os = "macos")]
    let desktop_update_url = desktop_update_url.as_str();
    let desktop_update_display_url = desktop_update_url
        .split_once('?')
        .map_or(desktop_update_url, |(endpoint, _)| endpoint);
    let client = RouteAwareClientPool::new_without_request_logging(
        config.http_client_factory(),
        ClientRouteClass::Other,
    );
    let outcome = match client
        .request(Method::GET, desktop_update_url)
        .timeout(deadline.saturating_duration_since(tokio::time::Instant::now()))
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status().as_u16();
            #[cfg(target_os = "windows")]
            if status == 404 && response.url().scheme() == "https" {
                checks[reachability_index].details.push(format!(
                    "desktop assets CDN: {desktop_update_display_url} reachable (HTTP 404; no update available)"
                ));
                return;
            }
            if cfg!(target_os = "windows") && response.url().scheme() != "https" {
                Err("update manifest redirected to a non-HTTPS URL".to_string())
            } else if status == 407 {
                Err("proxy authentication required (HTTP 407)".to_string())
            } else if !(200..=299).contains(&status) {
                Err(format!("HTTP {status}"))
            } else {
                checks[reachability_index].details.push(format!(
                    "desktop assets CDN: {desktop_update_display_url} reachable (HTTP {status})"
                ));
                #[cfg(target_os = "windows")]
                if let Some(update) = checks.iter_mut().find(|check| check.id == "updates.status") {
                    match response.bytes().await {
                        Ok(body) => match windows_store_update(&body, &application.version) {
                            Ok(Some(build)) => update.details.extend([
                                "desktop update status: available".to_string(),
                                format!("desktop latest build: {build}"),
                                format!("desktop application: {}", application.identity),
                            ]),
                            Ok(None) => {}
                            Err(error) => {
                                update.status = update.status.max(CheckStatus::Warning);
                                update
                                    .details
                                    .push(format!("desktop update manifest: {error}"));
                            }
                        },
                        Err(_) => {
                            update.status = update.status.max(CheckStatus::Warning);
                            update
                                .details
                                .push("desktop update manifest: response could not be read".into());
                        }
                    }
                }
                Ok(())
            }
        }
        Err(error) => Err(network::request_error(error)),
    };

    if let Err(error) = outcome {
        let reachability = &mut checks[reachability_index];
        reachability.details.push(format!(
            "desktop assets CDN: {desktop_update_display_url} {error} (optional)"
        ));
        if reachability.status == CheckStatus::Ok {
            reachability.status = CheckStatus::Warning;
            reachability.summary = "desktop update and runtime CDN is unreachable".to_string();
        }
        reachability.issues.push(
            DoctorIssue::new(
                CheckStatus::Warning,
                "desktop update and runtime CDN is unreachable",
            )
            .measured(format!("{desktop_update_display_url} {error}"))
            .expected("desktop update and runtime CDN reachable over HTTPS")
            .remedy(
                if desktop_update_display_url.starts_with("https://chatgpt.com/") {
                    "check proxy, firewall, DNS, and certificate access to chatgpt.com"
                } else {
                    "check proxy, firewall, DNS, and certificate access to persistent.oaistatic.com"
                },
            )
            .field("desktop assets CDN"),
        );
    }
}

#[cfg(target_os = "macos")]
fn macos_desktop_update_url(home: &Path, application: &InstalledApp, os_version: &str) -> String {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ProductionAppcastState {
        #[serde(default)]
        backend_appcast_enabled: bool,
        installation_id: Option<String>,
    }

    let state_path = home
        .join("Library/Application Support")
        .join(application.identity)
        .join("production-appcast-bootstrap.json");
    let Some(state) = std::fs::read(state_path)
        .ok()
        .and_then(|contents| serde_json::from_slice::<ProductionAppcastState>(&contents).ok())
    else {
        return DESKTOP_UPDATE_URL.to_string();
    };
    let Some(installation_id) = state
        .backend_appcast_enabled
        .then_some(state.installation_id)
        .flatten()
    else {
        return DESKTOP_UPDATE_URL.to_string();
    };

    let Ok(mut url) = Url::parse(BACKEND_DESKTOP_UPDATE_URL) else {
        return DESKTOP_UPDATE_URL.to_string();
    };
    url.query_pairs_mut().extend_pairs([
        ("installation_id", installation_id.as_str()),
        (
            "arch",
            if cfg!(target_arch = "x86_64") {
                "x64"
            } else {
                "arm64"
            },
        ),
        ("app_version", application.version.as_str()),
        ("beta", "false"),
        ("os-version", os_version),
        ("plan_type", "unknown"),
    ]);
    url.to_string()
}

#[cfg(any(target_os = "windows", test))]
fn windows_store_update(
    manifest: &[u8],
    installed_version: &str,
) -> Result<Option<String>, &'static str> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StoreManifest {
        schema_version: u64,
        build_version: String,
        store_product_id: String,
        package_identity: String,
    }

    let manifest: StoreManifest =
        serde_json::from_slice(manifest).map_err(|_| "invalid Windows Store update manifest")?;
    if manifest.schema_version == 0
        || manifest.store_product_id != "9PLM9XGG6VKS"
        || manifest.package_identity != "OpenAI.Codex"
    {
        return Err("Windows Store update manifest does not target the production application");
    }
    let version = |value: &str| -> Option<[u64; 4]> {
        value
            .split('.')
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()?
            .try_into()
            .ok()
    };
    let latest = version(&manifest.build_version)
        .ok_or("Windows Store update manifest contains an invalid build version")?;
    let installed =
        version(installed_version).ok_or("installed Windows application has an invalid version")?;
    Ok((latest > installed).then_some(manifest.build_version))
}

#[cfg(target_os = "macos")]
async fn latest_macos_staged_build(root: &Path, installed_build: u64) -> Option<u64> {
    const MAX_STAGED_BUNDLES: usize = 64;

    if !std::fs::symlink_metadata(root).ok()?.is_dir() {
        return None;
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let mut inspected = 0;
    let mut latest = None;
    for entry in std::fs::read_dir(root).ok()? {
        if inspected == MAX_STAGED_BUNDLES || tokio::time::Instant::now() >= deadline {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let extracted = entry.path().join("extracted");
        if !std::fs::symlink_metadata(&extracted).is_ok_and(|metadata| metadata.is_dir()) {
            continue;
        }
        let bundle = extracted.join("ChatGPT.app");
        if !std::fs::symlink_metadata(&bundle).is_ok_and(|metadata| metadata.is_dir()) {
            continue;
        }
        inspected += 1;
        let Ok(result) = tokio::time::timeout_at(
            deadline,
            crate::doctor::desktop::platform::inspect_macos_bundle(&bundle),
        )
        .await
        else {
            break;
        };
        if let Ok(Some(application)) = result
            && application.build > installed_build
        {
            latest = Some(latest.map_or(application.build, |latest: u64| {
                latest.max(application.build)
            }));
        }
    }
    latest
}

#[cfg(test)]
#[path = "updates_desktop_tests.rs"]
mod tests;
