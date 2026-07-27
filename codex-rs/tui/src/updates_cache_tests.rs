use super::*;
use crate::legacy_core::config::ConfigBuilder;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test]
async fn dismiss_version_does_not_reuse_official_codex_cache() {
    let codex_home = tempdir().expect("temp codex home");
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .build()
        .await
        .expect("load config");
    let official_version_file = codex_home.path().join("version.json");
    let official_version_info = VersionInfo {
        latest_version: "0.145.0".to_string(),
        last_checked_at: Utc::now(),
        dismissed_version: None,
    };
    let official_json = format!(
        "{}\n",
        serde_json::to_string(&official_version_info).expect("serialize cache")
    );
    tokio::fs::write(&official_version_file, &official_json)
        .await
        .expect("write official Codex cache");
    let version_file = version_filepath(&config);

    dismiss_version(&config, "999.0.0")
        .await
        .expect("dismiss version");

    let info = read_version_info(&version_file).expect("read version info");
    assert_eq!(
        info,
        VersionInfo {
            latest_version: "999.0.0".to_string(),
            last_checked_at: DateTime::<Utc>::UNIX_EPOCH,
            dismissed_version: Some("999.0.0".to_string()),
        }
    );
    assert_eq!(
        std::fs::read_to_string(official_version_file).expect("read official Codex cache"),
        official_json
    );
}
