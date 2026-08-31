use super::*;
use codex_core::config::ConfigBuilder;
use codex_login::login_with_api_key;
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

static TEST_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = TEST_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "codex-cloud-tasks-auth-{}-{timestamp}-{id}",
            std::process::id(),
        ));
        std::fs::create_dir(&path).expect("create temporary Codex home");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn auth_manager_respects_resolved_login_policy() {
    let codex_home = TestDirectory::new();
    std::fs::write(
        codex_home.path().join("config.toml"),
        "forced_login_method = \"chatgpt\"\ncli_auth_credentials_store = \"file\"\n",
    )
    .expect("write config");
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .build()
        .await
        .expect("load config");
    login_with_api_key(
        codex_home.path(),
        "sk-disallowed",
        config.cli_auth_credentials_store_mode,
        config.auth_keyring_backend_kind(),
    )
    .expect("save API key fixture");

    let auth_manager = load_auth_manager_from_config(&config, /*chatgpt_base_url*/ None).await;

    assert!(auth_manager.auth().await.is_none());
}
