use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::PluginSearchParams;
use codex_app_server_protocol::PluginSearchScope;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use codex_login::login_with_api_key;
use codex_utils_absolute_path::AbsolutePathBuf;
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
use super::plugin_search_support::read_plugin_search_response;
use super::plugin_search_support::remote_plugin_json;
use super::plugin_search_support::write_chatgpt_search_auth;
use super::plugin_search_support::write_installed_plugin;
use super::plugin_search_support::write_local_marketplace;
use super::plugin_search_support::write_remote_search_config;
use super::plugin_search_support::write_remote_search_config_with_enabled_plugin;

#[tokio::test]
async fn plugin_search_uses_local_catalogs_for_api_key_auth() -> Result<()> {
    let codex_home = TempDir::new()?;
    let repo_root = TempDir::new()?;
    let server = MockServer::start().await;
    let marketplace_path = write_local_marketplace(
        repo_root.path(),
        "personal-tools",
        "calendar-local",
        "Calendar Local",
        &[],
    )?;
    write_remote_search_config(
        codex_home.path(),
        &server,
        /*remote_plugin*/ true,
        /*plugin_sharing*/ false,
    )?;
    login_with_api_key(
        codex_home.path(),
        "sk-test-key",
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )?;

    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build()
        .await?;
    timeout(DEFAULT_TIMEOUT, app_server.initialize()).await??;
    let request_id = app_server
        .send_plugin_search_request(PluginSearchParams {
            search_term: "calendar".to_string(),
            scope: Some(PluginSearchScope::Personal),
            cwds: Some(vec![AbsolutePathBuf::try_from(repo_root.path())?]),
            cursor: None,
            limit: None,
        })
        .await?;
    let response = read_plugin_search_response(&mut app_server, request_id).await?;

    assert_eq!(response.next_cursor, None);
    assert_eq!(
        response
            .data
            .iter()
            .map(|result| (
                result.plugin.name.as_str(),
                result.marketplace_path.as_ref()
            ))
            .collect::<Vec<_>>(),
        vec![("calendar-local", Some(&marketplace_path))]
    );
    assert!(
        server
            .received_requests()
            .await
            .expect("wiremock should record requests")
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn plugin_search_adds_local_results_only_to_the_first_remote_page() -> Result<()> {
    let codex_home = TempDir::new()?;
    let repo_root = TempDir::new()?;
    let server = MockServer::start().await;
    write_local_marketplace(
        repo_root.path(),
        "personal-tools",
        "calendar-notes",
        "Calendar Notes",
        &[],
    )?;
    write_remote_search_config(
        codex_home.path(),
        &server,
        /*remote_plugin*/ true,
        /*plugin_sharing*/ false,
    )?;
    write_chatgpt_search_auth(codex_home.path())?;

    Mock::given(method("GET"))
        .and(path("/backend-api/ps/plugins/search"))
        .and(query_param("q", "calendar"))
        .and(query_param("limit", "1"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "plugins": [remote_plugin_json(
                "remote-first",
                "calendar",
                "GLOBAL",
                /*discoverability*/ None,
            )],
            "pagination": {"next_page_token": "next-page"},
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/backend-api/ps/plugins/search"))
        .and(query_param("q", "calendar"))
        .and(query_param("limit", "1"))
        .and(query_param("pageToken", "next-page"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "plugins": [remote_plugin_json(
                "remote-later",
                "calendar-later",
                "GLOBAL",
                /*discoverability*/ None,
            )],
            "pagination": {"next_page_token": null},
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build()
        .await?;
    timeout(DEFAULT_TIMEOUT, app_server.initialize()).await??;
    let roots = vec![AbsolutePathBuf::try_from(repo_root.path())?];
    let request_id = app_server
        .send_plugin_search_request(PluginSearchParams {
            search_term: "calendar".to_string(),
            scope: None,
            cwds: Some(roots.clone()),
            cursor: None,
            limit: Some(1),
        })
        .await?;
    let first_page = read_plugin_search_response(&mut app_server, request_id).await?;
    assert_eq!(first_page.next_cursor.as_deref(), Some("next-page"));
    assert_eq!(
        first_page
            .data
            .iter()
            .map(|result| result.plugin.name.as_str())
            .collect::<Vec<_>>(),
        vec!["calendar", "calendar-notes"]
    );

    let request_id = app_server
        .send_plugin_search_request(PluginSearchParams {
            search_term: "calendar".to_string(),
            scope: None,
            cwds: Some(roots),
            cursor: first_page.next_cursor,
            limit: Some(1),
        })
        .await?;
    let later_page = read_plugin_search_response(&mut app_server, request_id).await?;
    assert_eq!(later_page.next_cursor, None);
    assert_eq!(
        later_page
            .data
            .iter()
            .map(|result| (
                result.plugin.name.as_str(),
                result.marketplace_path.as_ref()
            ))
            .collect::<Vec<_>>(),
        vec![("calendar-later", None)]
    );
    Ok(())
}

#[tokio::test]
async fn plugin_search_deduplicates_remote_matches_and_retains_installed_state() -> Result<()> {
    let codex_home = TempDir::new()?;
    let repo_root = TempDir::new()?;
    let server = MockServer::start().await;
    write_local_marketplace(
        repo_root.path(),
        "personal-tools",
        "local-planner",
        "Local Calendar Planner",
        &[],
    )?;
    let shared_plugin_path = repo_root.path().join("plugins/local-planner");
    std::fs::create_dir_all(codex_home.path().join(".tmp"))?;
    std::fs::write(
        codex_home
            .path()
            .join(".tmp/plugin-share-local-paths-v1.json"),
        serde_json::to_string(&json!({
            "localPluginPathsByRemotePluginId": {
                "remote-shared": shared_plugin_path,
            },
        }))?,
    )?;
    write_installed_plugin(codex_home.path(), "personal-tools", "local-planner")?;
    write_remote_search_config_with_enabled_plugin(
        codex_home.path(),
        &server,
        "local-planner@personal-tools",
    )?;
    write_chatgpt_search_auth(codex_home.path())?;

    Mock::given(method("GET"))
        .and(path("/backend-api/ps/plugins/search"))
        .and(query_param("q", "calendar"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "plugins": [remote_plugin_json(
                "remote-shared",
                "remote-calendar",
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
        .build()
        .await?;
    timeout(DEFAULT_TIMEOUT, app_server.initialize()).await??;
    let request_id = app_server
        .send_plugin_search_request(PluginSearchParams {
            search_term: "calendar".to_string(),
            scope: None,
            cwds: Some(vec![AbsolutePathBuf::try_from(repo_root.path())?]),
            cursor: None,
            limit: None,
        })
        .await?;
    let response = read_plugin_search_response(&mut app_server, request_id).await?;

    assert_eq!(
        response
            .data
            .iter()
            .map(|result| (
                result.plugin.name.as_str(),
                result.plugin.installed,
                result.plugin.local_version.as_deref(),
                result.marketplace_path.as_ref(),
            ))
            .collect::<Vec<_>>(),
        vec![("remote-calendar", true, Some("1.2.3"), None)]
    );
    Ok(())
}
