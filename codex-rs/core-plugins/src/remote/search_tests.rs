use super::*;
use crate::remote::REMOTE_GLOBAL_MARKETPLACE_NAME;
use codex_app_server_protocol::PluginAuthPolicy;
use codex_app_server_protocol::PluginAvailability;
use codex_app_server_protocol::PluginInstallPolicy;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::query_param;

fn remote_plugin_json(remote_plugin_id: &str, plugin_name: &str) -> serde_json::Value {
    json!({
        "id": remote_plugin_id,
        "name": plugin_name,
        "scope": "GLOBAL",
        "installation_policy": "AVAILABLE",
        "authentication_policy": "ON_USE",
        "release": {
            "display_name": "",
            "description": "",
            "interface": {},
        },
    })
}

#[tokio::test]
async fn search_remote_plugins_forwards_parameters_and_bounds_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/ps/plugins/search"))
        .and(query_param("q", "linear & docs/+"))
        .and(query_param("scope", "GLOBAL"))
        .and(query_param("limit", "1"))
        .and(query_param("pageToken", "next page/+"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "plugins": [
                remote_plugin_json("plugin-linear", "linear"),
                remote_plugin_json("plugin-calendar", "calendar"),
            ],
            "pagination": {"next_page_token": "later page/+"},
        })))
        .expect(1)
        .mount(&server)
        .await;
    let config = RemotePluginServiceConfig {
        chatgpt_base_url: format!("{}/backend-api/", server.uri()),
    };
    let auth = CodexAuth::create_dummy_chatgpt_auth_for_testing();

    let page = search_remote_plugins(
        &config,
        Some(&auth),
        RemotePluginSearchRequest {
            query: "linear & docs/+",
            scope: Some(RemotePluginScope::Global),
            limit: 1,
            page_token: Some("next page/+"),
        },
    )
    .await
    .expect("plugin search should succeed");

    assert_eq!(
        page,
        RemotePluginSearchPage {
            plugins: vec![RemotePluginSummary {
                id: format!("linear@{REMOTE_GLOBAL_MARKETPLACE_NAME}"),
                remote_plugin_id: "plugin-linear".to_string(),
                version: None,
                local_version: None,
                name: "linear".to_string(),
                share_context: None,
                installed: false,
                enabled: false,
                install_policy: PluginInstallPolicy::Available,
                install_policy_source: None,
                auth_policy: PluginAuthPolicy::OnUse,
                availability: PluginAvailability::Available,
                disabled_reason: None,
                eligible_plan_types: None,
                interface: None,
                keywords: Vec::new(),
            }],
            next_page_token: Some("later page/+".to_string()),
        }
    );
}

#[tokio::test]
async fn search_remote_plugins_requires_chatgpt_authentication() {
    let config = RemotePluginServiceConfig {
        chatgpt_base_url: "https://chatgpt.example/backend-api".to_string(),
    };

    let result = search_remote_plugins(
        &config,
        /*auth*/ None,
        RemotePluginSearchRequest {
            query: "calendar",
            scope: None,
            limit: 16,
            page_token: None,
        },
    )
    .await;

    assert!(matches!(
        result,
        Err(RemotePluginCatalogError::AuthRequired)
    ));
}

#[tokio::test]
async fn search_remote_plugins_redacts_sensitive_parameters_from_transport_errors() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("test listener should bind to a local port");
    let address = listener
        .local_addr()
        .expect("test listener should have a local address");
    let connection = std::thread::spawn(move || {
        let (stream, _) = listener
            .accept()
            .expect("test listener should accept the plugin search request");
        drop(stream);
    });
    let config = RemotePluginServiceConfig {
        chatgpt_base_url: format!("http://{address}/backend-api"),
    };
    let auth = CodexAuth::create_dummy_chatgpt_auth_for_testing();

    let error = search_remote_plugins(
        &config,
        Some(&auth),
        RemotePluginSearchRequest {
            query: "sensitive search term",
            scope: Some(RemotePluginScope::Global),
            limit: 16,
            page_token: Some("sensitive pagination token"),
        },
    )
    .await
    .expect_err("closed connection should fail the plugin search request");
    connection
        .join()
        .expect("test listener should close the accepted connection");

    assert!(!error.to_string().contains("sensitive"));
    let RemotePluginCatalogError::Request { url, source } = error else {
        panic!("expected transport request error");
    };
    assert_eq!(
        url,
        format!("http://{address}/backend-api/ps/plugins/search")
    );
    assert!(!source.to_string().contains("sensitive"));
}

#[tokio::test]
async fn search_remote_plugins_rejects_oversized_responses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/ps/plugins/search"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![
            b'x';
            MAX_REMOTE_PLUGIN_SEARCH_RESPONSE_BYTES
                + 1
        ]))
        .expect(1)
        .mount(&server)
        .await;
    let config = RemotePluginServiceConfig {
        chatgpt_base_url: format!("{}/backend-api", server.uri()),
    };
    let auth = CodexAuth::create_dummy_chatgpt_auth_for_testing();

    let error = search_remote_plugins(
        &config,
        Some(&auth),
        RemotePluginSearchRequest {
            query: "sensitive search term",
            scope: None,
            limit: 16,
            page_token: Some("sensitive pagination token"),
        },
    )
    .await
    .expect_err("oversized plugin search response should be rejected");

    assert!(
        !error.to_string().contains("sensitive"),
        "search parameters should stay out of errors"
    );
    assert!(matches!(
        error,
        RemotePluginCatalogError::ResponseTooLarge { url, max_bytes }
            if url == format!("{}/backend-api/ps/plugins/search", server.uri())
                && max_bytes == MAX_REMOTE_PLUGIN_SEARCH_RESPONSE_BYTES
    ));
}
