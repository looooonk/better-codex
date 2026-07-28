//! Diagnoses Better Codex update availability.
//!
//! Update diagnostics combine cached version metadata, install-channel hints,
//! and bounded latest-version probes.

use std::path::Path;

use codex_core::config::Config;
use codex_install_context::InstallContext;
use codex_install_context::InstallMethod;
use codex_install_context::StandalonePlatform;
use semver::Version;
use serde::Deserialize;

use super::CheckStatus;
use super::DoctorCheck;
use super::doctor_install_context;
use super::run_command;

const VERSION_FILE_NAME: &str = "better-codex-version.json";
const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/looooonk/better-codex/releases?per_page=1";

/// Builds the update-health row for the current installation.
///
/// Network failures while fetching latest-version metadata degrade the row to a
/// warning instead of failing doctor outright; update freshness is useful
/// support context but should not mask more direct install/config failures.
pub(super) fn updates_check(config: &Config) -> DoctorCheck {
    let current_exe = std::env::current_exe().ok();
    let install_context = doctor_install_context(current_exe.as_deref());
    let mut details = vec![
        format!(
            "check for update on startup: {}",
            config.check_for_update_on_startup
        ),
        format!("update action: {}", update_action_label(&install_context)),
    ];
    let version_file = config.codex_home.join(VERSION_FILE_NAME);
    push_cached_version_details(&mut details, &version_file);

    let (mut status, summary, remediation) = match &install_context.method {
        InstallMethod::Npm | InstallMethod::Bun | InstallMethod::Pnpm | InstallMethod::Brew => (
            CheckStatus::Warning,
            "this package-manager install is not a Better Codex update channel".to_string(),
            Some(
                "Install the Better Codex standalone release with `curl -fsSL https://raw.githubusercontent.com/looooonk/better-codex/main/scripts/install.sh | sh`."
                    .to_string(),
            ),
        ),
        InstallMethod::Standalone {
            platform: StandalonePlatform::Windows,
            ..
        } => (
            CheckStatus::Warning,
            "automatic updates are unavailable for this standalone platform".to_string(),
            Some(
                "Download the latest Better Codex release from https://github.com/looooonk/better-codex/releases."
                    .to_string(),
            ),
        ),
        InstallMethod::Standalone {
            platform: StandalonePlatform::Unix,
            ..
        }
        | InstallMethod::Other => (
            CheckStatus::Ok,
            "update configuration is locally consistent".to_string(),
            None,
        ),
    };

    match fetch_latest_version() {
        Ok(latest_version) => {
            details.push(format!("latest version: {latest_version}"));
            if is_newer(&latest_version, env!("CARGO_PKG_VERSION")) == Some(true) {
                details.push("latest version status: newer version is available".to_string());
            } else {
                details.push("latest version status: current version is not older".to_string());
            }
        }
        Err(err) => {
            status = status.max(CheckStatus::Warning);
            details.push(format!("latest version probe: {err}"));
        }
    }

    let mut check = DoctorCheck::new("updates.status", "updates", status, summary).details(details);
    if let Some(remediation) = remediation {
        check = check.remediation(remediation);
    }
    check
}

fn push_cached_version_details(details: &mut Vec<String>, version_file: &Path) {
    details.push(format!("version cache: {}", version_file.display()));
    match std::fs::read_to_string(version_file) {
        Ok(contents) => match serde_json::from_str::<VersionInfo>(&contents) {
            Ok(info) => {
                details.push(format!("cached latest version: {}", info.latest_version));
                if let Some(last_checked_at) = info.last_checked_at {
                    details.push(format!("last checked at: {last_checked_at}"));
                }
                if let Some(dismissed_version) = info.dismissed_version {
                    details.push(format!("dismissed version: {dismissed_version}"));
                }
            }
            Err(err) => details.push(format!("version cache parse: {err}")),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            details.push("version cache: missing".to_string());
        }
        Err(err) => details.push(format!("version cache read: {err}")),
    }
}

fn update_action_label(context: &InstallContext) -> &'static str {
    match &context.method {
        InstallMethod::Standalone {
            platform: StandalonePlatform::Unix,
            ..
        } => "standalone installer",
        InstallMethod::Standalone {
            platform: StandalonePlatform::Windows,
            ..
        } => "manual (automatic update unavailable)",
        InstallMethod::Npm => "unsupported npm install (use standalone installer)",
        InstallMethod::Bun => "unsupported bun install (use standalone installer)",
        InstallMethod::Pnpm => "unsupported pnpm install (use standalone installer)",
        InstallMethod::Brew => "unsupported Homebrew install (use standalone installer)",
        InstallMethod::Other => "manual or unknown",
    }
}

fn fetch_latest_version() -> Result<String, String> {
    fetch_latest_github_release_version()
}

fn fetch_latest_github_release_version() -> Result<String, String> {
    #[derive(Deserialize)]
    struct ReleaseInfo {
        tag_name: String,
    }

    let info = http_get_json::<Vec<ReleaseInfo>>(GITHUB_LATEST_RELEASE_URL)?
        .into_iter()
        .next()
        .ok_or_else(|| "Better Codex has no published releases".to_string())?;
    let version = info
        .tag_name
        .strip_prefix('v')
        .ok_or_else(|| format!("failed to parse latest tag {}", info.tag_name))?;
    Version::parse(version)
        .map(|_| version.to_string())
        .map_err(|err| format!("failed to parse latest tag {}: {err}", info.tag_name))
}

fn http_get_json<T>(url: &str) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let body = run_command("curl", ["-fsSL", "--max-time", "5", url])?;
    serde_json::from_str::<T>(&body).map_err(|err| err.to_string())
}

fn is_newer(latest: &str, current: &str) -> Option<bool> {
    Some(Version::parse(latest.trim()).ok()? > Version::parse(current.trim()).ok()?)
}

#[derive(Deserialize)]
struct VersionInfo {
    latest_version: String,
    #[serde(default)]
    last_checked_at: Option<String>,
    #[serde(default)]
    dismissed_version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_compares_plain_semver() {
        assert_eq!(is_newer("1.2.4", "1.2.3"), Some(true));
        assert_eq!(is_newer("1.2.3", "1.2.4"), Some(false));
        assert_eq!(is_newer("1.2.3-beta.1", "1.2.2"), Some(true));
    }

    #[test]
    fn update_action_labels_install_contexts() {
        assert_eq!(
            update_action_label(&InstallContext {
                method: InstallMethod::Npm,
                package_layout: None,
            }),
            "unsupported npm install (use standalone installer)"
        );
        assert_eq!(
            update_action_label(&InstallContext {
                method: InstallMethod::Pnpm,
                package_layout: None,
            }),
            "unsupported pnpm install (use standalone installer)"
        );
        assert_eq!(
            update_action_label(&InstallContext {
                method: InstallMethod::Other,
                package_layout: None,
            }),
            "manual or unknown"
        );
    }
}
