use codex_install_context::InstallContext;
use codex_install_context::InstallMethod;
use codex_install_context::StandalonePlatform;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

use super::InstallClassification;
use super::MAX_VERSION_RESPONSE_BYTES;
use super::STANDALONE_INSTALL_REMEDIATION;
use super::UpdateCheckInput;
use super::WINDOWS_UPDATE_REMEDIATION;
use super::build_updates_check;
use super::classify_install_method;
use super::http_get_json;
use super::is_newer;
use crate::doctor::CheckStatus;
use crate::doctor::DoctorCheck;
use codex_http_client::ClientRouteClass;
use codex_http_client::RouteAwareClientPool;
use std::time::Duration;

#[test]
fn is_newer_compares_plain_semver() {
    assert_eq!(is_newer("1.2.4", "1.2.3"), Some(true));
    assert_eq!(is_newer("1.2.3", "1.2.4"), Some(false));
    assert_eq!(is_newer("1.2.3-beta.1", "1.2.2"), Some(true));
}

#[test]
fn classifies_every_install_method() {
    let release_dir = release_dir();
    let cases = [
        (
            InstallMethod::Npm,
            InstallClassification {
                action_label: "unsupported npm install (use standalone installer)",
                status: CheckStatus::Warning,
                summary: "this package-manager install is not a Better Codex update channel",
                remediation: Some(STANDALONE_INSTALL_REMEDIATION),
            },
        ),
        (
            InstallMethod::Bun,
            InstallClassification {
                action_label: "unsupported bun install (use standalone installer)",
                status: CheckStatus::Warning,
                summary: "this package-manager install is not a Better Codex update channel",
                remediation: Some(STANDALONE_INSTALL_REMEDIATION),
            },
        ),
        (
            InstallMethod::Pnpm,
            InstallClassification {
                action_label: "unsupported pnpm install (use standalone installer)",
                status: CheckStatus::Warning,
                summary: "this package-manager install is not a Better Codex update channel",
                remediation: Some(STANDALONE_INSTALL_REMEDIATION),
            },
        ),
        (
            InstallMethod::Brew,
            InstallClassification {
                action_label: "unsupported Homebrew install (use standalone installer)",
                status: CheckStatus::Warning,
                summary: "this package-manager install is not a Better Codex update channel",
                remediation: Some(STANDALONE_INSTALL_REMEDIATION),
            },
        ),
        (
            InstallMethod::Standalone {
                release_dir: release_dir.clone(),
                resources_dir: Some(release_dir.join("codex-resources")),
                platform: StandalonePlatform::Unix,
            },
            InstallClassification {
                action_label: "standalone installer",
                status: CheckStatus::Ok,
                summary: "update configuration is locally consistent",
                remediation: None,
            },
        ),
        (
            InstallMethod::Standalone {
                release_dir,
                resources_dir: None,
                platform: StandalonePlatform::Windows,
            },
            InstallClassification {
                action_label: "manual (automatic update unavailable)",
                status: CheckStatus::Warning,
                summary: "automatic updates are unavailable for this standalone platform",
                remediation: Some(WINDOWS_UPDATE_REMEDIATION),
            },
        ),
        (
            InstallMethod::Other,
            InstallClassification {
                action_label: "manual or unknown",
                status: CheckStatus::Ok,
                summary: "update configuration is locally consistent",
                remediation: None,
            },
        ),
    ];

    for (method, expected) in cases {
        assert_eq!(classify_install_method(&method), expected);
    }
}

#[test]
fn builds_complete_checks_for_every_install_method() {
    let temp = tempfile::tempdir().expect("temp dir");
    let version_file = temp.path().join("missing-version.json");
    let release_dir = release_dir();
    let cases = [
        (
            InstallMethod::Npm,
            "unsupported npm install (use standalone installer)",
            CheckStatus::Warning,
            "this package-manager install is not a Better Codex update channel",
            Some(STANDALONE_INSTALL_REMEDIATION),
        ),
        (
            InstallMethod::Bun,
            "unsupported bun install (use standalone installer)",
            CheckStatus::Warning,
            "this package-manager install is not a Better Codex update channel",
            Some(STANDALONE_INSTALL_REMEDIATION),
        ),
        (
            InstallMethod::Pnpm,
            "unsupported pnpm install (use standalone installer)",
            CheckStatus::Warning,
            "this package-manager install is not a Better Codex update channel",
            Some(STANDALONE_INSTALL_REMEDIATION),
        ),
        (
            InstallMethod::Brew,
            "unsupported Homebrew install (use standalone installer)",
            CheckStatus::Warning,
            "this package-manager install is not a Better Codex update channel",
            Some(STANDALONE_INSTALL_REMEDIATION),
        ),
        (
            InstallMethod::Standalone {
                release_dir: release_dir.clone(),
                resources_dir: Some(release_dir.join("codex-resources")),
                platform: StandalonePlatform::Unix,
            },
            "standalone installer",
            CheckStatus::Ok,
            "update configuration is locally consistent",
            None,
        ),
        (
            InstallMethod::Standalone {
                release_dir,
                resources_dir: None,
                platform: StandalonePlatform::Windows,
            },
            "manual (automatic update unavailable)",
            CheckStatus::Warning,
            "automatic updates are unavailable for this standalone platform",
            Some(WINDOWS_UPDATE_REMEDIATION),
        ),
        (
            InstallMethod::Other,
            "manual or unknown",
            CheckStatus::Ok,
            "update configuration is locally consistent",
            None,
        ),
    ];

    for (method, action_label, status, summary, remediation) in cases {
        let install_context = InstallContext {
            method,
            package_layout: None,
        };
        let actual = build_updates_check(
            UpdateCheckInput {
                check_for_update_on_startup: true,
                version_file: &version_file,
                install_context: &install_context,
            },
            || Ok(env!("CARGO_PKG_VERSION").to_string()),
        );
        let mut expected =
            DoctorCheck::new("updates.status", "updates", status, summary).details(vec![
                "check for update on startup: true".to_string(),
                format!("update action: {action_label}"),
                format!("version cache: {}", version_file.display()),
                "version cache: missing".to_string(),
                format!("latest version: {}", env!("CARGO_PKG_VERSION")),
                "latest version status: current version is not older".to_string(),
            ]);
        if let Some(remediation) = remediation {
            expected = expected.remediation(remediation);
        }

        assert_eq!(actual, expected);
    }
}

#[test]
fn newer_probe_result_is_reported_without_degrading_status() {
    let temp = tempfile::tempdir().expect("temp dir");
    let version_file = temp.path().join("missing-version.json");
    let install_context = InstallContext {
        method: InstallMethod::Other,
        package_layout: None,
    };

    let actual = build_updates_check(
        UpdateCheckInput {
            check_for_update_on_startup: false,
            version_file: &version_file,
            install_context: &install_context,
        },
        || Ok("9999.0.0".to_string()),
    );
    let expected = DoctorCheck::new(
        "updates.status",
        "updates",
        CheckStatus::Ok,
        "update configuration is locally consistent",
    )
    .details(vec![
        "check for update on startup: false".to_string(),
        "update action: manual or unknown".to_string(),
        format!("version cache: {}", version_file.display()),
        "version cache: missing".to_string(),
        "latest version: 9999.0.0".to_string(),
        "latest version status: newer version is available".to_string(),
    ]);

    assert_eq!(actual, expected);
}

#[test]
fn failed_probe_degrades_an_other_install_to_warning() {
    let temp = tempfile::tempdir().expect("temp dir");
    let version_file = temp.path().join("missing-version.json");
    let install_context = InstallContext {
        method: InstallMethod::Other,
        package_layout: None,
    };

    let actual = build_updates_check(
        UpdateCheckInput {
            check_for_update_on_startup: true,
            version_file: &version_file,
            install_context: &install_context,
        },
        || Err("request timed out".to_string()),
    );
    let expected = DoctorCheck::new(
        "updates.status",
        "updates",
        CheckStatus::Warning,
        "update configuration is locally consistent",
    )
    .details(vec![
        "check for update on startup: true".to_string(),
        "update action: manual or unknown".to_string(),
        format!("version cache: {}", version_file.display()),
        "version cache: missing".to_string(),
        "latest version probe: request timed out".to_string(),
    ]);

    assert_eq!(actual, expected);
}

fn release_dir() -> AbsolutePathBuf {
    AbsolutePathBuf::from_absolute_path(std::env::temp_dir().join("better-codex-release"))
        .expect("release dir path should be absolute")
}

#[tokio::test]
async fn version_http_probe_decodes_json_and_rejects_invalid_responses() {
    use codex_http_client::HttpClientFactory;
    use codex_http_client::OutboundProxyPolicy;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    let server = MockServer::start().await;
    let client = RouteAwareClientPool::new_without_request_logging(
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        ClientRouteClass::Other,
    );
    for (endpoint, response) in [
        (
            "valid",
            ResponseTemplate::new(/*s*/ 200).set_body_json(serde_json::json!({"version": "1.2.3"})),
        ),
        (
            "invalid",
            ResponseTemplate::new(/*s*/ 200).set_body_string("not JSON"),
        ),
        ("unavailable", ResponseTemplate::new(/*s*/ 503)),
        ("proxy_auth_required", ResponseTemplate::new(/*s*/ 407)),
        (
            "redirect",
            ResponseTemplate::new(/*s*/ 302)
                .insert_header("Location", format!("{}/valid", server.uri())),
        ),
        (
            "timeout",
            ResponseTemplate::new(/*s*/ 200).set_delay(Duration::from_secs(/*secs*/ 6)),
        ),
        (
            "oversized",
            ResponseTemplate::new(/*s*/ 200)
                .set_body_bytes(vec![b' '; MAX_VERSION_RESPONSE_BYTES + 1]),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/{endpoint}")))
            .respond_with(response)
            .mount(&server)
            .await;
        let result =
            http_get_json::<serde_json::Value>(&client, &format!("{}/{endpoint}", server.uri()))
                .await;
        if matches!(endpoint, "valid" | "redirect") {
            assert_eq!(result, Ok(serde_json::json!({"version": "1.2.3"})));
        } else if endpoint == "timeout" {
            assert_eq!(result, Err("version request timed out".to_string()));
        } else if endpoint == "proxy_auth_required" {
            assert_eq!(
                result,
                Err("HTTP 407 Proxy Authentication Required".to_string())
            );
        } else {
            assert!(result.is_err(), "{endpoint} must not be accepted");
        }
    }
}
