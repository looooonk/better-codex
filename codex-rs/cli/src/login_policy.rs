use codex_login::AuthConfig;
use codex_login::AuthManager;
use codex_login::ServerOptions;
use codex_login::login_with_access_token;
use codex_login::login_with_api_key;
use codex_login::run_device_code_login;
use codex_protocol::config_types::ForcedLoginMethod;

pub(crate) fn ensure_login_method_allowed(
    auth_config: &AuthConfig,
    method: ForcedLoginMethod,
) -> std::io::Result<()> {
    if auth_config.is_login_method_allowed(method) {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "authentication method is disabled by policy",
        ))
    }
}

pub(crate) async fn chatgpt_server_options(
    auth_config: &AuthConfig,
    client_id: String,
) -> std::io::Result<ServerOptions> {
    ensure_login_method_allowed(auth_config, ForcedLoginMethod::Chatgpt)?;
    clear_existing_auth_before_login(auth_config).await;
    Ok(ServerOptions::new(
        auth_config.codex_home.clone(),
        client_id,
        auth_config.effective_chatgpt_workspaces(),
        auth_config.auth_credentials_store_mode,
        auth_config.keyring_backend_kind,
        auth_config.auth_route_config.clone(),
    ))
}

pub(crate) async fn run_managed_device_code_login(
    auth_config: &AuthConfig,
    issuer_base_url: Option<String>,
    client_id: String,
) -> std::io::Result<()> {
    let mut options = chatgpt_server_options(auth_config, client_id).await?;
    if let Some(issuer_base_url) = issuer_base_url {
        options.issuer = issuer_base_url;
    }
    run_device_code_login(options).await
}

pub(crate) fn persist_api_key(auth_config: &AuthConfig, api_key: &str) -> std::io::Result<()> {
    ensure_login_method_allowed(auth_config, ForcedLoginMethod::Api)?;
    login_with_api_key(
        &auth_config.codex_home,
        api_key,
        auth_config.auth_credentials_store_mode,
        auth_config.keyring_backend_kind,
    )
}

pub(crate) async fn persist_access_token(
    auth_config: &AuthConfig,
    access_token: &str,
) -> std::io::Result<()> {
    ensure_login_method_allowed(auth_config, ForcedLoginMethod::Chatgpt)?;
    let effective_workspaces = auth_config.effective_chatgpt_workspaces();
    login_with_access_token(
        &auth_config.codex_home,
        access_token,
        auth_config.auth_credentials_store_mode,
        effective_workspaces.as_deref(),
        auth_config.chatgpt_base_url.as_deref(),
        auth_config.keyring_backend_kind,
        auth_config.auth_route_config.as_ref(),
    )
    .await
}

pub(crate) async fn logout_stored_auth_with_revoke(
    auth_config: &AuthConfig,
) -> std::io::Result<bool> {
    AuthManager::shared_from_stored_auth_config(auth_config.clone())
        .await
        .logout_with_revoke()
        .await
}

async fn clear_existing_auth_before_login(auth_config: &AuthConfig) {
    if let Err(err) = logout_stored_auth_with_revoke(auth_config).await {
        tracing::warn!("failed to clear existing auth before login: {err}");
    }
}

#[cfg(test)]
#[path = "login_policy_tests.rs"]
mod tests;
