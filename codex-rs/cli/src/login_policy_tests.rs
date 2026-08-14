use super::*;
use anyhow::Context;
use codex_config::LoaderOverrides;
use codex_config::types::AuthCredentialsStoreMode;
use codex_core::config::ConfigBuilder;
use codex_login::AuthKeyringBackendKind;
use codex_login::CLIENT_ID;
use codex_login::load_auth_dot_json;
use codex_login::login_with_api_key;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

const WORKSPACE_ALLOWED: &str = "workspace-allowed";
const WORKSPACE_DENIED_JWT: &str = concat!(
    "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.",
    "eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjo",
    "id29ya3NwYWNlLWRlbmllZCIsIm9yZ2FuaXphdGlvbl9pZCI6IndvcmtzcGFjZS1kZW5pZWQifX0.",
    "c2ln"
);

async fn managed_auth_config(
    codex_home: &TempDir,
    requirements: &str,
) -> anyhow::Result<AuthConfig> {
    std::fs::write(
        codex_home.path().join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )?;
    let requirements_path = codex_home.path().join("requirements.toml");
    std::fs::write(&requirements_path, requirements)?;
    let mut loader_overrides = LoaderOverrides::without_managed_config_for_tests();
    loader_overrides.system_requirements_path = Some(requirements_path);
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .fallback_cwd(Some(codex_home.path().to_path_buf()))
        .loader_overrides(loader_overrides)
        .build()
        .await?;
    Ok(config.auth_config())
}

fn stored_auth(auth_config: &AuthConfig) -> anyhow::Result<Option<codex_login::AuthDotJson>> {
    Ok(load_auth_dot_json(
        &auth_config.codex_home,
        auth_config.auth_credentials_store_mode,
        auth_config.keyring_backend_kind,
    )?)
}

#[tokio::test]
async fn disallowed_managed_device_login_preserves_auth_and_makes_no_request()
-> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let auth_config = managed_auth_config(
        &codex_home,
        r#"allowed_login_methods = ["api"]"#,
    )
    .await?;
    login_with_api_key(
        &auth_config.codex_home,
        "sk-existing",
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )?;
    let expected_auth = stored_auth(&auth_config)?;
    let server = MockServer::start().await;

    let err = run_managed_device_code_login(
        &auth_config,
        Some(server.uri()),
        CLIENT_ID.to_string(),
    )
    .await
    .expect_err("managed API-only policy should reject device login");

    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(stored_auth(&auth_config)?, expected_auth);
    let requests = server
        .received_requests()
        .await
        .context("read mock login requests")?;
    assert!(requests.is_empty(), "disallowed login made {requests:?}");
    Ok(())
}

#[tokio::test]
async fn disallowed_managed_persistence_paths_write_no_auth() -> anyhow::Result<()> {
    let api_only_home = TempDir::new()?;
    let api_only = managed_auth_config(
        &api_only_home,
        r#"allowed_login_methods = ["api"]"#,
    )
    .await?;
    let access_token_error = persist_access_token(&api_only, "not-a-token")
        .await
        .expect_err("API-only policy should reject access-token login");
    assert_eq!(
        (access_token_error.kind(), stored_auth(&api_only)?),
        (std::io::ErrorKind::PermissionDenied, None)
    );

    let chatgpt_only_home = TempDir::new()?;
    let chatgpt_only = managed_auth_config(
        &chatgpt_only_home,
        r#"allowed_login_methods = ["chatgpt"]"#,
    )
    .await?;
    let api_key_error = persist_api_key(&chatgpt_only, "sk-disallowed")
        .expect_err("ChatGPT-only policy should reject API-key login");
    assert_eq!(
        (api_key_error.kind(), stored_auth(&chatgpt_only)?),
        (std::io::ErrorKind::PermissionDenied, None)
    );
    Ok(())
}

#[tokio::test]
async fn managed_workspace_mismatch_does_not_persist_device_login() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let auth_config = managed_auth_config(
        &codex_home,
        &format!(
            "allowed_login_methods = [\"chatgpt\"]\nallowed_chatgpt_workspaces = [\"{WORKSPACE_ALLOWED}\"]\n"
        ),
    )
    .await?;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/usercode"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "device_auth_id": "device-auth-123",
            "user_code": "CODE-12345",
            "interval": "0",
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authorization_code": "authorization-code-123",
            "code_challenge": "code-challenge-123",
            "code_verifier": "code-verifier-123",
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id_token": WORKSPACE_DENIED_JWT,
            "access_token": "new-access",
            "refresh_token": "new-refresh",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let err = run_managed_device_code_login(
        &auth_config,
        Some(server.uri()),
        CLIENT_ID.to_string(),
    )
    .await
    .expect_err("device login should reject a workspace outside the managed allowlist");

    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(stored_auth(&auth_config)?, None);
    server.verify().await;
    Ok(())
}
