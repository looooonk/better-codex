#![cfg(any(not(debug_assertions), test))]

use crate::legacy_core::config::Config;
use crate::update_action::UpdateAction;
use crate::update_versions::extract_version_from_latest_tag;
use crate::update_versions::is_newer;
use crate::update_versions::is_source_build_version;
use crate::updates_cache::VersionInfo;
use crate::updates_cache::read_version_info;
use crate::updates_cache::version_filepath;
use chrono::Duration;
use chrono::Utc;
#[cfg(not(debug_assertions))]
use codex_login::default_client::create_client;
use serde::Deserialize;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;

#[cfg(not(debug_assertions))]
use crate::version::CODEX_CLI_VERSION;

#[cfg(not(debug_assertions))]
pub(crate) use crate::updates_cache::dismiss_version;

#[cfg(not(debug_assertions))]
pub fn get_upgrade_version(config: &Config) -> Option<String> {
    get_upgrade_version_with_refresh(
        config,
        CODEX_CLI_VERSION,
        UpdateCheckEligibility::from(crate::update_action::get_update_action()),
        Utc::now(),
        spawn_update_refresh,
    )
}

#[cfg(not(debug_assertions))]
const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/looooonk/better-codex/releases?per_page=1";

#[derive(Deserialize, Debug, Clone)]
struct ReleaseInfo {
    tag_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateCheckEligibility {
    UnsupportedInstall,
    StandaloneUnix,
}

impl From<Option<UpdateAction>> for UpdateCheckEligibility {
    fn from(update_action: Option<UpdateAction>) -> Self {
        match update_action {
            Some(UpdateAction::StandaloneUnix) => Self::StandaloneUnix,
            None => Self::UnsupportedInstall,
        }
    }
}

fn get_upgrade_version_with_refresh(
    config: &Config,
    current_version: &str,
    eligibility: UpdateCheckEligibility,
    now: chrono::DateTime<Utc>,
    refresh: impl FnOnce(PathBuf),
) -> Option<String> {
    if !config.check_for_update_on_startup
        || is_source_build_version(current_version)
        || eligibility == UpdateCheckEligibility::UnsupportedInstall
    {
        return None;
    }

    let version_file = version_filepath(config);
    let info = read_version_info(&version_file).ok();

    if match &info {
        None => true,
        Some(info) => info.last_checked_at < now - Duration::hours(20),
    } {
        refresh(version_file);
    }

    info.and_then(|info| {
        is_newer(&info.latest_version, current_version)
            .unwrap_or(false)
            .then_some(info.latest_version)
    })
}

#[cfg(not(debug_assertions))]
fn spawn_update_refresh(version_file: PathBuf) {
    // Refresh the cached latest version in the background so TUI startup
    // isn’t blocked by a network call. The UI reads the previously cached
    // value (if any) for this run; the next run shows the banner if needed.
    tokio::spawn(async move {
        check_for_update(&version_file)
            .await
            .inspect_err(|e| tracing::error!("Failed to update version: {e}"))
    });
}

#[cfg(not(debug_assertions))]
async fn check_for_update(version_file: &Path) -> anyhow::Result<()> {
    check_for_update_with_fetch(
        version_file,
        Utc::now(),
        fetch_latest_github_release_version(),
    )
    .await
}

async fn check_for_update_with_fetch(
    version_file: &Path,
    checked_at: chrono::DateTime<Utc>,
    fetch_latest_version: impl Future<Output = anyhow::Result<String>>,
) -> anyhow::Result<()> {
    let latest_version = fetch_latest_version.await?;
    // Preserve any previously dismissed version if present.
    let prev_info = read_version_info(version_file).ok();
    let info = VersionInfo {
        latest_version,
        last_checked_at: checked_at,
        dismissed_version: prev_info.and_then(|p| p.dismissed_version),
    };

    let json_line = format!("{}\n", serde_json::to_string(&info)?);
    if let Some(parent) = version_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(version_file, json_line).await?;
    Ok(())
}

#[cfg(not(debug_assertions))]
async fn fetch_latest_github_release_version() -> anyhow::Result<String> {
    let response = create_client()
        .get(LATEST_RELEASE_URL)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    parse_latest_github_release_version(&response)
}

fn parse_latest_github_release_version(response: &[u8]) -> anyhow::Result<String> {
    let releases = serde_json::from_slice::<Vec<ReleaseInfo>>(response)?;
    let latest_tag_name = releases
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Better Codex has no published releases"))?
        .tag_name;
    extract_version_from_latest_tag(&latest_tag_name)
}

/// Returns the latest version to show in a popup, if it should be shown.
/// This respects the user's dismissal choice for the current latest version.
#[cfg(not(debug_assertions))]
pub fn get_upgrade_version_for_popup(config: &Config) -> Option<String> {
    get_upgrade_version_for_popup_with_refresh(
        config,
        CODEX_CLI_VERSION,
        UpdateCheckEligibility::from(crate::update_action::get_update_action()),
        Utc::now(),
        spawn_update_refresh,
    )
}

fn get_upgrade_version_for_popup_with_refresh(
    config: &Config,
    current_version: &str,
    eligibility: UpdateCheckEligibility,
    now: chrono::DateTime<Utc>,
    refresh: impl FnOnce(PathBuf),
) -> Option<String> {
    let version_file = version_filepath(config);
    let latest =
        get_upgrade_version_with_refresh(config, current_version, eligibility, now, refresh)?;
    // If the user dismissed this exact version previously, do not show the popup.
    if let Ok(info) = read_version_info(&version_file)
        && info.dismissed_version.as_deref() == Some(latest.as_str())
    {
        return None;
    }
    Some(latest)
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
