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
const STANDALONE_INSTALL_REMEDIATION: &str = "Install the Better Codex standalone release with `curl -fsSL https://raw.githubusercontent.com/looooonk/better-codex/main/scripts/install.sh | sh`.";
const WINDOWS_UPDATE_REMEDIATION: &str = "Download the latest Better Codex release from https://github.com/looooonk/better-codex/releases.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InstallClassification {
    action_label: &'static str,
    status: CheckStatus,
    summary: &'static str,
    remediation: Option<&'static str>,
}

struct UpdateCheckInput<'a> {
    check_for_update_on_startup: bool,
    version_file: &'a Path,
    install_context: &'a InstallContext,
}

/// Builds the update-health row for the current installation.
///
/// Network failures while fetching latest-version metadata degrade the row to a
/// warning instead of failing doctor outright; update freshness is useful
/// support context but should not mask more direct install/config failures.
pub(super) fn updates_check(config: &Config) -> DoctorCheck {
    let current_exe = std::env::current_exe().ok();
    let install_context = doctor_install_context(current_exe.as_deref());
    let version_file = config.codex_home.join(VERSION_FILE_NAME);
    build_updates_check(
        UpdateCheckInput {
            check_for_update_on_startup: config.check_for_update_on_startup,
            version_file: &version_file,
            install_context: &install_context,
        },
        fetch_latest_github_release_version,
    )
}

fn build_updates_check(
    input: UpdateCheckInput<'_>,
    latest_version_probe: impl FnOnce() -> Result<String, String>,
) -> DoctorCheck {
    let classification = classify_install_method(&input.install_context.method);
    let mut details = vec![
        format!(
            "check for update on startup: {}",
            input.check_for_update_on_startup
        ),
        format!("update action: {}", classification.action_label),
    ];
    push_cached_version_details(&mut details, input.version_file);

    let mut status = classification.status;
    match latest_version_probe() {
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

    let mut check = DoctorCheck::new("updates.status", "updates", status, classification.summary)
        .details(details);
    if let Some(remediation) = classification.remediation {
        check = check.remediation(remediation);
    }
    check
}

fn classify_install_method(method: &InstallMethod) -> InstallClassification {
    match method {
        InstallMethod::Npm => package_manager_install_classification(
            "unsupported npm install (use standalone installer)",
        ),
        InstallMethod::Bun => package_manager_install_classification(
            "unsupported bun install (use standalone installer)",
        ),
        InstallMethod::Pnpm => package_manager_install_classification(
            "unsupported pnpm install (use standalone installer)",
        ),
        InstallMethod::Brew => package_manager_install_classification(
            "unsupported Homebrew install (use standalone installer)",
        ),
        InstallMethod::Standalone {
            platform: StandalonePlatform::Windows,
            ..
        } => InstallClassification {
            action_label: "manual (automatic update unavailable)",
            status: CheckStatus::Warning,
            summary: "automatic updates are unavailable for this standalone platform",
            remediation: Some(WINDOWS_UPDATE_REMEDIATION),
        },
        InstallMethod::Standalone {
            platform: StandalonePlatform::Unix,
            ..
        } => InstallClassification {
            action_label: "standalone installer",
            status: CheckStatus::Ok,
            summary: "update configuration is locally consistent",
            remediation: None,
        },
        InstallMethod::Other => InstallClassification {
            action_label: "manual or unknown",
            status: CheckStatus::Ok,
            summary: "update configuration is locally consistent",
            remediation: None,
        },
    }
}

fn package_manager_install_classification(action_label: &'static str) -> InstallClassification {
    InstallClassification {
        action_label,
        status: CheckStatus::Warning,
        summary: "this package-manager install is not a Better Codex update channel",
        remediation: Some(STANDALONE_INSTALL_REMEDIATION),
    }
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
#[path = "updates_tests.rs"]
mod tests;
