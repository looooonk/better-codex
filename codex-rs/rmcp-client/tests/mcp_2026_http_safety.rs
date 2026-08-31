use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_config::types::AuthKeyringBackendKind;
use codex_config::types::OAuthCredentialsStoreMode;
use codex_exec_server::Environment;
use codex_rmcp_client::ElicitationAction;
use codex_rmcp_client::ElicitationResponse;
use codex_rmcp_client::McpProtocolMode;
use codex_rmcp_client::RmcpClient;
use futures::FutureExt;
use pretty_assertions::assert_eq;
use rmcp::model::ClientCapabilities;
use rmcp::model::Implementation;
use rmcp::model::InitializeRequestParams;
use rmcp::model::ProtocolVersion;
use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

const MAX_MCP_HTTP_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

fn modern_discovery(body: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": body["id"],
        "result": {
            "resultType": "complete",
            "supportedVersions": ["2026-07-28"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "http-safety-test", "version": "1.0.0"},
            "ttlMs": 0,
            "cacheScope": "private",
        },
    })
}

fn legacy_initialize(body: &Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "jsonrpc": "2.0",
        "id": body["id"],
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "legacy-http-test", "version": "1.0.0"},
        },
    }))
}

async fn client(
    server: &MockServer,
    headers: Option<HashMap<String, String>>,
) -> anyhow::Result<RmcpClient> {
    RmcpClient::new_streamable_http_client_with_protocol_mode(
        "http-safety-test",
        &format!("{}/mcp", server.uri()),
        /*bearer_token*/ None,
        headers,
        /*env_http_headers*/ None,
        OAuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
        Environment::default_for_tests().get_http_client(),
        /*auth_provider*/ None,
        McpProtocolMode::V20260728,
    )
    .await
}

async fn initialize(client: &RmcpClient) -> anyhow::Result<rmcp::model::ServerPeerInfo> {
    client
        .initialize(
            InitializeRequestParams::new(
                ClientCapabilities::default(),
                Implementation::new("codex-http-safety-test", "1.0.0"),
            )
            .with_protocol_version(ProtocolVersion::V_2025_06_18),
            Some(Duration::from_secs(5)),
            Box::new(|_, _| {
                async {
                    Ok(ElicitationResponse {
                        action: ElicitationAction::Decline,
                        content: None,
                        meta: None,
                    })
                }
                .boxed()
            }),
        )
        .await
}

#[tokio::test]
async fn live_json_discovery_rejects_a_wrong_response_id() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .respond_with(|request: &Request| {
            let body: Value = request.body_json().expect("discovery request");
            let mut response = modern_discovery(&body);
            response["id"] = json!("wrong-request");
            ResponseTemplate::new(200).set_body_json(response)
        })
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server, /*headers*/ None).await?;
    let error = initialize(&client)
        .await
        .expect_err("wrong discovery IDs must be rejected");
    assert!(format!("{error:#}").contains("response ID did not match"));
    client.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn live_sse_discovery_ignores_wrong_ids_until_the_match() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .respond_with(|request: &Request| {
            let body: Value = request.body_json().expect("discovery request");
            let mut wrong = modern_discovery(&body);
            wrong["id"] = json!("wrong-request");
            let matching = modern_discovery(&body);
            ResponseTemplate::new(200).set_body_raw(
                format!("event: message\ndata: {wrong}\n\nevent: message\ndata: {matching}\n\n"),
                "text/event-stream; charset=utf-8",
            )
        })
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server, /*headers*/ None).await?;
    let peer = initialize(&client).await?;
    assert_eq!(peer.protocol_version, ProtocolVersion::V_2026_07_28);
    client.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn modern_mode_falls_back_for_idless_prevalidation_and_plain_404() -> anyhow::Result<()> {
    for idless_prevalidation in [true, false] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(move |request: &Request| {
                let body: Value = request.body_json().expect("MCP request");
                match body["method"].as_str() {
                    Some("server/discover") if idless_prevalidation => ResponseTemplate::new(400)
                        .set_body_json(json!({
                            "jsonrpc": "2.0",
                            "id": null,
                            "error": {
                                "code": -32000,
                                "message": "Bad Request: No valid session ID provided",
                            },
                        })),
                    Some("server/discover") => ResponseTemplate::new(404),
                    Some("initialize") => legacy_initialize(&body),
                    Some("notifications/initialized") => ResponseTemplate::new(202),
                    other => panic!("unexpected fallback method {other:?}"),
                }
            })
            .expect(3)
            .mount(&server)
            .await;

        let client = client(&server, /*headers*/ None).await?;
        let peer = initialize(&client).await?;
        assert_eq!(peer.protocol_version, ProtocolVersion::V_2025_06_18);
        client.shutdown().await;
    }
    Ok(())
}

#[tokio::test]
async fn modern_discovery_stops_redirects_before_forwarding_headers() -> anyhow::Result<()> {
    let target = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/forwarded"))
        .and(header("x-api-key", "sensitive-key"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&target)
        .await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/forwarded", target.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = client(
        &server,
        Some(HashMap::from([(
            "x-api-key".to_string(),
            "sensitive-key".to_string(),
        )])),
    )
    .await?;
    initialize(&client)
        .await
        .expect_err("modern discovery must not follow redirects");
    target.verify().await;
    client.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn modern_discovery_bounds_json_bodies_and_sse_events() -> anyhow::Result<()> {
    for use_sse in [false, true] {
        let server = MockServer::start().await;
        let padding = Arc::new("x".repeat(MAX_MCP_HTTP_RESPONSE_BYTES + 1));
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(move |request: &Request| {
                let body: Value = request.body_json().expect("discovery request");
                let mut response = modern_discovery(&body);
                response["result"]["instructions"] = json!(padding.as_str());
                if use_sse {
                    ResponseTemplate::new(200).set_body_raw(
                        format!("event: message\ndata: {response}\n\n"),
                        "text/event-stream; charset=utf-8",
                    )
                } else {
                    ResponseTemplate::new(200).set_body_json(response)
                }
            })
            .expect(1)
            .mount(&server)
            .await;

        let client = client(&server, /*headers*/ None).await?;
        let error = initialize(&client)
            .await
            .expect_err("oversized modern discovery responses must be rejected");
        assert!(format!("{error:#}").contains("8388608 bytes"));
        client.shutdown().await;
    }
    Ok(())
}

#[tokio::test]
async fn modern_body_limit_survives_session_recovery() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let discoveries = Arc::new(AtomicUsize::new(0));
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let discovery_count = Arc::clone(&discoveries);
    let tool_count = Arc::clone(&tool_calls);
    let oversized = Arc::new("x".repeat(MAX_MCP_HTTP_RESPONSE_BYTES + 1));
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .respond_with(move |request: &Request| {
            let body: Value = request.body_json().expect("MCP request");
            match body["method"].as_str() {
                Some("server/discover") => {
                    if discovery_count.fetch_add(1, Ordering::SeqCst) == 0 {
                        ResponseTemplate::new(200).set_body_json(json!({
                            "jsonrpc": "2.0",
                            "id": body["id"],
                            "error": {"code": -32601, "message": "method not found"},
                        }))
                    } else {
                        ResponseTemplate::new(200)
                            .set_body_json(modern_discovery(&body))
                            .insert_header("mcp-session-id", "modern-session")
                    }
                }
                Some("initialize") => {
                    legacy_initialize(&body).insert_header("mcp-session-id", "legacy-session")
                }
                Some("notifications/initialized") => ResponseTemplate::new(202),
                Some("tools/list") if tool_count.fetch_add(1, Ordering::SeqCst) == 0 => {
                    ResponseTemplate::new(404)
                }
                Some("tools/list") => ResponseTemplate::new(200).set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": body["id"],
                    "result": {
                        "resultType": "complete",
                        "tools": [{
                            "name": "oversized",
                            "description": oversized.as_str(),
                            "inputSchema": {"type": "object"},
                        }],
                    },
                })),
                other => panic!("unexpected recovery method {other:?}"),
            }
        })
        .expect(6)
        .mount(&server)
        .await;

    let client = client(&server, /*headers*/ None).await?;
    initialize(&client).await?;
    let error = client
        .list_tools(/*params*/ None, Some(Duration::from_secs(10)))
        .await
        .expect_err("recovered modern sessions must retain response bounds");
    let error = format!("{error:#}");
    assert!(
        error.contains("8388608 bytes"),
        "unexpected recovered-session error: {error}"
    );
    assert_eq!(discoveries.load(Ordering::SeqCst), 2);
    assert_eq!(tool_calls.load(Ordering::SeqCst), 2);
    client.shutdown().await;
    Ok(())
}
