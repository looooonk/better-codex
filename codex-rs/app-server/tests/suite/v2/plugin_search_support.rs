use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use app_test_support::write_chatgpt_auth;
use codex_app_server_protocol::PluginSearchResponse;
use codex_app_server_protocol::RequestId;
use codex_config::types::AuthCredentialsStoreMode;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde_json::json;
use tokio::time::timeout;
use wiremock::MockServer;

pub(super) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) async fn read_plugin_search_response(
    app_server: &mut TestAppServer,
    request_id: i64,
) -> Result<PluginSearchResponse> {
    let response = timeout(
        DEFAULT_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    to_response(response)
}

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

pub(super) fn write_remote_search_config_with_enabled_plugin(
    codex_home: &Path,
    server: &MockServer,
    plugin_id: &str,
) -> Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"chatgpt_base_url = "{}/backend-api/"

[features]
plugins = true
remote_plugin = true
plugin_sharing = true

[plugins."{plugin_id}"]
enabled = true
"#,
            server.uri()
        ),
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

pub(super) fn write_local_marketplace(
    root: &Path,
    marketplace_name: &str,
    plugin_name: &str,
    display_name: &str,
    keywords: &[&str],
) -> Result<AbsolutePathBuf> {
    std::fs::create_dir_all(root.join(".git"))?;
    std::fs::create_dir_all(root.join(".agents/plugins"))?;
    let marketplace_path = root.join(".agents/plugins/marketplace.json");
    std::fs::write(
        &marketplace_path,
        serde_json::to_string(&json!({
            "name": marketplace_name,
            "plugins": [{
                "name": plugin_name,
                "source": {
                    "source": "local",
                    "path": format!("./plugins/{plugin_name}"),
                },
            }],
        }))?,
    )?;

    let manifest_dir = root.join("plugins").join(plugin_name).join(".codex-plugin");
    std::fs::create_dir_all(&manifest_dir)?;
    std::fs::write(
        manifest_dir.join("plugin.json"),
        serde_json::to_string(&json!({
            "name": plugin_name,
            "keywords": keywords,
            "interface": {
                "displayName": display_name,
                "shortDescription": format!("{display_name} description"),
            },
        }))?,
    )?;

    Ok(AbsolutePathBuf::try_from(marketplace_path)?)
}

pub(super) fn write_installed_plugin(
    codex_home: &Path,
    marketplace_name: &str,
    plugin_name: &str,
) -> Result<()> {
    let manifest_dir = codex_home
        .join("plugins/cache")
        .join(marketplace_name)
        .join(plugin_name)
        .join("1.2.3")
        .join(".codex-plugin");
    std::fs::create_dir_all(&manifest_dir)?;
    std::fs::write(
        manifest_dir.join("plugin.json"),
        serde_json::to_string(&json!({
            "name": plugin_name,
            "version": "1.2.3",
            "interface": {"displayName": plugin_name},
        }))?,
    )?;
    Ok(())
}
