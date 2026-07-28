use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_install_context::InstallContext;
use codex_install_context::InstallMethod;
use codex_install_context::StandalonePlatform;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::cell::RefCell;
use std::future::ready;
use tempfile::TempDir;
use tempfile::tempdir;

const CURRENT_VERSION: &str = "1.0.0";
const LATEST_VERSION: &str = "2.0.0";

async fn test_config() -> (TempDir, Config) {
    let codex_home = tempdir().expect("temp codex home");
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .build()
        .await
        .expect("load config");
    (codex_home, config)
}

fn fixed_now() -> chrono::DateTime<Utc> {
    "2026-07-28T00:00:00Z"
        .parse()
        .expect("fixed timestamp should parse")
}

fn write_version_info(config: &Config, info: &VersionInfo) {
    let json = format!(
        "{}\n",
        serde_json::to_string(info).expect("serialize version info")
    );
    std::fs::write(version_filepath(config), json).expect("write version info");
}

#[tokio::test]
async fn unsupported_installs_do_not_read_or_refresh_the_cache() {
    let (codex_home, config) = test_config().await;
    write_version_info(
        &config,
        &VersionInfo {
            latest_version: LATEST_VERSION.to_string(),
            last_checked_at: fixed_now() - Duration::days(1),
            dismissed_version: None,
        },
    );
    let release_dir = AbsolutePathBuf::from_absolute_path(codex_home.path().join("release"))
        .expect("release dir should be absolute");
    let mut contexts = [
        InstallMethod::Npm,
        InstallMethod::Bun,
        InstallMethod::Pnpm,
        InstallMethod::Brew,
        InstallMethod::Other,
    ]
    .into_iter()
    .map(|method| InstallContext {
        method,
        package_layout: None,
    })
    .collect::<Vec<_>>();
    contexts.push(InstallContext {
        method: InstallMethod::Standalone {
            platform: StandalonePlatform::Windows,
            resources_dir: Some(release_dir.join("codex-resources")),
            release_dir,
        },
        package_layout: None,
    });

    for context in contexts {
        let refreshes = RefCell::new(Vec::new());
        let eligibility =
            UpdateCheckEligibility::from(UpdateAction::from_install_context(&context));
        let upgrade = get_upgrade_version_with_refresh(
            &config,
            CURRENT_VERSION,
            eligibility,
            fixed_now(),
            |path| refreshes.borrow_mut().push(path),
        );

        assert_eq!((upgrade, refreshes.into_inner()), (None, Vec::new()));
    }
}

#[tokio::test]
async fn fresh_cache_returns_the_upgrade_without_refreshing() {
    let (_codex_home, config) = test_config().await;
    write_version_info(
        &config,
        &VersionInfo {
            latest_version: LATEST_VERSION.to_string(),
            last_checked_at: fixed_now() - Duration::hours(19),
            dismissed_version: None,
        },
    );
    let refreshes = RefCell::new(Vec::new());

    let upgrade = get_upgrade_version_with_refresh(
        &config,
        CURRENT_VERSION,
        UpdateCheckEligibility::StandaloneUnix,
        fixed_now(),
        |path| refreshes.borrow_mut().push(path),
    );

    assert_eq!(
        (upgrade, refreshes.into_inner()),
        (Some(LATEST_VERSION.to_string()), Vec::new())
    );
}

#[tokio::test]
async fn stale_cache_returns_the_cached_upgrade_and_schedules_a_refresh() {
    let (_codex_home, config) = test_config().await;
    write_version_info(
        &config,
        &VersionInfo {
            latest_version: LATEST_VERSION.to_string(),
            last_checked_at: fixed_now() - Duration::hours(21),
            dismissed_version: None,
        },
    );
    let refreshes = RefCell::new(Vec::new());

    let upgrade = get_upgrade_version_with_refresh(
        &config,
        CURRENT_VERSION,
        UpdateCheckEligibility::StandaloneUnix,
        fixed_now(),
        |path| refreshes.borrow_mut().push(path),
    );

    assert_eq!(
        (upgrade, refreshes.into_inner()),
        (
            Some(LATEST_VERSION.to_string()),
            vec![version_filepath(&config)]
        )
    );
}

#[tokio::test]
async fn missing_cache_schedules_a_refresh_without_showing_an_upgrade() {
    let (_codex_home, config) = test_config().await;
    let refreshes = RefCell::new(Vec::new());

    let upgrade = get_upgrade_version_with_refresh(
        &config,
        CURRENT_VERSION,
        UpdateCheckEligibility::StandaloneUnix,
        fixed_now(),
        |path| refreshes.borrow_mut().push(path),
    );

    assert_eq!(
        (upgrade, refreshes.into_inner()),
        (None, vec![version_filepath(&config)])
    );
}

#[tokio::test]
async fn popup_hides_the_dismissed_latest_version() {
    let (_codex_home, config) = test_config().await;
    write_version_info(
        &config,
        &VersionInfo {
            latest_version: LATEST_VERSION.to_string(),
            last_checked_at: fixed_now(),
            dismissed_version: Some(LATEST_VERSION.to_string()),
        },
    );
    let refreshes = RefCell::new(Vec::new());

    let upgrade = get_upgrade_version_for_popup_with_refresh(
        &config,
        CURRENT_VERSION,
        UpdateCheckEligibility::StandaloneUnix,
        fixed_now(),
        |path| refreshes.borrow_mut().push(path),
    );

    assert_eq!((upgrade, refreshes.into_inner()), (None, Vec::new()));
}

#[tokio::test]
async fn standalone_refresh_populates_the_cache_without_network_access() {
    let (codex_home, config) = test_config().await;
    let release_dir = AbsolutePathBuf::from_absolute_path(codex_home.path().join("release"))
        .expect("release dir should be absolute");
    let eligibility =
        UpdateCheckEligibility::from(UpdateAction::from_install_context(&InstallContext {
            method: InstallMethod::Standalone {
                platform: StandalonePlatform::Unix,
                resources_dir: Some(release_dir.join("codex-resources")),
                release_dir,
            },
            package_layout: None,
        }));
    let refreshes = RefCell::new(Vec::new());

    let upgrade = get_upgrade_version_with_refresh(
        &config,
        CURRENT_VERSION,
        eligibility,
        fixed_now(),
        |path| refreshes.borrow_mut().push(path),
    );
    assert_eq!(
        (upgrade, refreshes.into_inner()),
        (None, vec![version_filepath(&config)])
    );

    check_for_update_with_fetch(
        &version_filepath(&config),
        fixed_now(),
        ready(Ok(LATEST_VERSION.to_string())),
    )
    .await
    .expect("refresh version cache");

    let unexpected_refreshes = RefCell::new(Vec::new());
    let upgrade = get_upgrade_version_with_refresh(
        &config,
        CURRENT_VERSION,
        eligibility,
        fixed_now(),
        |path| unexpected_refreshes.borrow_mut().push(path),
    );
    assert_eq!(
        (upgrade, unexpected_refreshes.into_inner()),
        (Some(LATEST_VERSION.to_string()), Vec::new())
    );
}

#[tokio::test]
async fn refresh_preserves_the_dismissed_version() {
    let (_codex_home, config) = test_config().await;
    let version_file = version_filepath(&config);
    write_version_info(
        &config,
        &VersionInfo {
            latest_version: "1.5.0".to_string(),
            last_checked_at: fixed_now() - Duration::days(1),
            dismissed_version: Some("1.5.0".to_string()),
        },
    );

    check_for_update_with_fetch(
        &version_file,
        fixed_now(),
        ready(Ok(LATEST_VERSION.to_string())),
    )
    .await
    .expect("refresh version cache");

    assert_eq!(
        read_version_info(&version_file).expect("read refreshed version info"),
        VersionInfo {
            latest_version: LATEST_VERSION.to_string(),
            last_checked_at: fixed_now(),
            dismissed_version: Some("1.5.0".to_string()),
        }
    );
}

#[test]
fn release_response_extracts_the_latest_version() {
    assert_eq!(
        parse_latest_github_release_version(br#"[{"tag_name":"v2.0.0"}]"#)
            .expect("valid release response should parse"),
        LATEST_VERSION
    );
}

#[test]
fn empty_release_response_reports_that_no_release_exists() {
    assert_eq!(
        parse_latest_github_release_version(b"[]")
            .expect_err("empty release response should fail")
            .to_string(),
        "Better Codex has no published releases"
    );
}

#[test]
fn malformed_release_responses_are_rejected() {
    let errors = [
        "not-json".as_bytes(),
        br#"[{"tag_name":"latest"}]"#.as_slice(),
    ]
    .into_iter()
    .map(|response| {
        parse_latest_github_release_version(response)
            .expect_err("malformed release response should fail")
            .to_string()
    })
    .collect::<Vec<_>>();

    assert_eq!(
        errors,
        vec![
            "expected ident at line 1 column 2".to_string(),
            "Failed to parse latest tag name 'latest'".to_string(),
        ]
    );
}
