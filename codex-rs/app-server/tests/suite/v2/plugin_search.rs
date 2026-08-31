use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::PluginSearchParams;
use codex_app_server_protocol::PluginSearchResponse;
use codex_app_server_protocol::PluginSearchScope;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::query_param;
use wiremock::matchers::query_param_is_missing;

use super::plugin_search_support::DEFAULT_TIMEOUT;
use super::plugin_search_support::remote_plugin_json;
use super::plugin_search_support::write_chatgpt_search_auth;
use super::plugin_search_support::write_remote_search_config;

#[tokio::test]
async fn plugin_search_routes_bounded_remote_queries_and_filters_shared_results() -> Result<()> {
    let codex_home = TempDir::new()?;
    let server = MockServer::start().await;
    write_remote_search_config(
        codex_home.path(),
        &server,
        /*remote_plugin*/ true,
        /*plugin_sharing*/ false,
    )?;
    write_chatgpt_search_auth(codex_home.path())?;

    Mock::given(method("GET"))
        .and(path("/backend-api/ps/plugins/search"))
        .and(query_param("q", "linear & docs"))
        .and(query_param("scope", "GLOBAL"))
        .and(query_param("limit", "100"))
        .and(query_param("pageToken", "incoming-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "plugins": [
                remote_plugin_json(
                    "plugin-global",
                    "global-linear",
                    "GLOBAL",
                    /*discoverability*/ None,
                ),
                remote_plugin_json(
                    "plugin-listed",
                    "listed-linear",
                    "WORKSPACE",
                    Some("LISTED"),
                ),
                remote_plugin_json(
                    "plugin-private",
                    "private-linear",
                    "WORKSPACE",
                    Some("PRIVATE"),
                ),
                remote_plugin_json(
                    "plugin-unlisted",
                    "unlisted-linear",
                    "WORKSPACE",
                    Some("UNLISTED"),
                ),
            ],
            "pagination": {"next_page_token": "outgoing-token"},
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build_initialized_with_timeout(DEFAULT_TIMEOUT)
        .await?;
    let request_id = app_server
        .send_plugin_search_request(PluginSearchParams {
            search_term: "linear & docs".to_string(),
            scope: Some(PluginSearchScope::Global),
            cwds: None,
            cursor: Some("incoming-token".to_string()),
            limit: Some(u32::MAX),
        })
        .await?;
    let response: PluginSearchResponse =
        timeout(DEFAULT_TIMEOUT, app_server.read_response(request_id)).await??;

    assert_eq!(response.next_cursor.as_deref(), Some("outgoing-token"));
    assert_eq!(
        response
            .data
            .iter()
            .map(|result| (result.marketplace_name.as_str(), result.plugin.name.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("openai-curated-remote", "global-linear"),
            ("workspace-directory", "listed-linear"),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn plugin_search_falls_back_to_workspace_when_remote_plugin_is_disabled() -> Result<()> {
    let codex_home = TempDir::new()?;
    let server = MockServer::start().await;
    write_remote_search_config(
        codex_home.path(),
        &server,
        /*remote_plugin*/ false,
        /*plugin_sharing*/ false,
    )?;
    write_chatgpt_search_auth(codex_home.path())?;

    Mock::given(method("GET"))
        .and(path("/backend-api/ps/plugins/search"))
        .and(query_param("q", "linear"))
        .and(query_param("scope", "WORKSPACE"))
        .and(query_param("limit", "16"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "plugins": [remote_plugin_json(
                "plugin-workspace",
                "workspace-linear",
                "WORKSPACE",
                Some("LISTED"),
            )],
            "pagination": {"next_page_token": null},
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build_initialized_with_timeout(DEFAULT_TIMEOUT)
        .await?;
    let request_id = app_server
        .send_plugin_search_request(PluginSearchParams {
            search_term: "linear".to_string(),
            scope: None,
            cwds: None,
            cursor: None,
            limit: None,
        })
        .await?;
    let response: PluginSearchResponse =
        timeout(DEFAULT_TIMEOUT, app_server.read_response(request_id)).await??;
    assert_eq!(
        response
            .data
            .iter()
            .map(|result| result.plugin.name.as_str())
            .collect::<Vec<_>>(),
        vec!["workspace-linear"]
    );

    let request_id = app_server
        .send_plugin_search_request(PluginSearchParams {
            search_term: "linear".to_string(),
            scope: Some(PluginSearchScope::Global),
            cwds: None,
            cursor: None,
            limit: None,
        })
        .await?;
    let response: PluginSearchResponse =
        timeout(DEFAULT_TIMEOUT, app_server.read_response(request_id)).await??;
    assert_eq!(
        response,
        PluginSearchResponse {
            data: Vec::new(),
            next_cursor: None,
        }
    );
    Ok(())
}

#[tokio::test]
async fn plugin_search_returns_empty_when_plugins_are_disabled() -> Result<()> {
    let codex_home = TempDir::new()?;
    std::fs::write(
        codex_home.path().join("config.toml"),
        "[features]\nplugins = false\n",
    )?;
    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build_initialized_with_timeout(DEFAULT_TIMEOUT)
        .await?;

    let request_id = app_server
        .send_plugin_search_request(PluginSearchParams {
            search_term: "linear".to_string(),
            scope: None,
            cwds: None,
            cursor: None,
            limit: None,
        })
        .await?;
    let response: PluginSearchResponse =
        timeout(DEFAULT_TIMEOUT, app_server.read_response(request_id)).await??;
    assert_eq!(
        response,
        PluginSearchResponse {
            data: Vec::new(),
            next_cursor: None,
        }
    );
    Ok(())
}
