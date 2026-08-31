use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::write_chatgpt_auth;
use codex_config::types::AuthCredentialsStoreMode;
use serde_json::json;
use wiremock::MockServer;

pub(super) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn write_remote_search_config(
    codex_home: &Path,
    server: &MockServer,
    remote_plugin: bool,
    plugin_sharing: bool,
) -> Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"chatgpt_base_url = "{}/backend-api/"

[features]
plugins = true
remote_plugin = {remote_plugin}
plugin_sharing = {plugin_sharing}
"#,
            server.uri()
        ),
    )?;
    Ok(())
}

pub(super) fn write_chatgpt_search_auth(codex_home: &Path) -> Result<()> {
    write_chatgpt_auth(
        codex_home,
        ChatGptAuthFixture::new("chatgpt-token")
            .account_id("account-123")
            .chatgpt_user_id("user-123")
            .chatgpt_account_id("account-123"),
        AuthCredentialsStoreMode::File,
    )?;
    Ok(())
}

pub(super) fn remote_plugin_json(
    remote_plugin_id: &str,
    plugin_name: &str,
    scope: &str,
    discoverability: Option<&str>,
) -> serde_json::Value {
    json!({
        "id": remote_plugin_id,
        "name": plugin_name,
        "scope": scope,
        "discoverability": discoverability,
        "installation_policy": "AVAILABLE",
        "authentication_policy": "ON_USE",
        "release": {
            "display_name": plugin_name,
            "description": format!("{plugin_name} description"),
            "interface": {},
        },
    })
}
