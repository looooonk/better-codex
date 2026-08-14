use super::load_cli_auth_mode;
use codex_config::LoaderOverrides;
use codex_core::config::ConfigBuilder;
use codex_login::login_with_api_key;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[tokio::test]
async fn managed_deny_all_auth_policy_reports_no_stored_auth_mode() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    std::fs::write(
        codex_home.path().join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )?;
    let requirements_path = codex_home.path().join("requirements.toml");
    std::fs::write(
        &requirements_path,
        "allowed_login_methods = []\n",
    )?;
    let mut loader_overrides = LoaderOverrides::without_managed_config_for_tests();
    loader_overrides.system_requirements_path = Some(requirements_path);
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .fallback_cwd(Some(codex_home.path().to_path_buf()))
        .loader_overrides(loader_overrides)
        .build()
        .await?;
    login_with_api_key(
        &config.codex_home,
        "sk-disallowed",
        config.cli_auth_credentials_store_mode,
        config.auth_keyring_backend_kind(),
    )?;

    assert_eq!(load_cli_auth_mode(&config).await, None);
    Ok(())
}
