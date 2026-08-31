use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use codex_config::types::AuthKeyringBackendKind;
use codex_config::types::OAuthCredentialsStoreMode;
use codex_exec_server::Environment;
use codex_rmcp_client::Elicitation;
use codex_rmcp_client::ElicitationAction;
use codex_rmcp_client::ElicitationResponse;
use codex_rmcp_client::McpProtocolMode;
use codex_rmcp_client::RmcpClient;
use futures::FutureExt;
use pretty_assertions::assert_eq;
use rmcp::model::ClientCapabilities;
use rmcp::model::ElicitRequestParams;
use rmcp::model::ElicitationCapability;
use rmcp::model::FormElicitationCapability;
use rmcp::model::Implementation;
use rmcp::model::InitializeRequestParams;
use rmcp::model::ProtocolVersion;
use rmcp::model::ReadResourceRequestParams;
use rmcp::model::ServerPeerInfo;
use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn modern_discovery(body: &Value) -> ResponseTemplate {
    rpc_result(
        body,
        json!({
            "resultType": "complete",
            "supportedVersions": ["2026-07-28"],
            "capabilities": {"tools": {}, "resources": {}},
            "serverInfo": {"name": "mrtr-test", "version": "1.0.0"},
            "ttlMs": 0,
            "cacheScope": "private",
        }),
    )
}

fn legacy_initialize(body: &Value) -> ResponseTemplate {
    rpc_result(
        body,
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {"resources": {}},
            "serverInfo": {"name": "legacy-mrtr-test", "version": "1.0.0"},
        }),
    )
}

fn rpc_result(body: &Value, result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "jsonrpc": "2.0",
        "id": body["id"],
        "result": result,
    }))
}

fn input_required(body: &Value) -> ResponseTemplate {
    rpc_result(
        body,
        json!({
            "resultType": "input_required",
            "inputRequests": {
                "confirmation": {
                    "method": "elicitation/create",
                    "params": {
                        "mode": "form",
                        "message": "Confirm the MCP request.",
                        "requestedSchema": {
                            "type": "object",
                            "properties": {"confirmed": {"type": "boolean"}},
                            "required": ["confirmed"],
                        },
                    },
                },
            },
            "requestState": "opaque-state",
        }),
    )
}

async fn create_client(
    server: &MockServer,
    elicitations: Arc<Mutex<Vec<String>>>,
) -> anyhow::Result<(RmcpClient, ServerPeerInfo)> {
    let client = RmcpClient::new_streamable_http_client_with_protocol_mode(
        "mrtr-test",
        &format!("{}/mcp", server.uri()),
        /*bearer_token*/ None,
        /*http_headers*/ None,
        /*env_http_headers*/ None,
        OAuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
        Environment::default_for_tests().get_http_client(),
        /*auth_provider*/ None,
        McpProtocolMode::V20260728,
    )
    .await?;
    let mut capabilities = ClientCapabilities::default();
    capabilities.elicitation =
        Some(ElicitationCapability::new().with_form(FormElicitationCapability::new()));
    let peer = client
        .initialize(
            InitializeRequestParams::new(
                capabilities,
                Implementation::new("codex-mrtr-test", "1.0.0"),
            )
            .with_protocol_version(ProtocolVersion::V_2025_06_18),
            Some(Duration::from_secs(5)),
            Box::new(move |request_id, request| {
                let elicitations = Arc::clone(&elicitations);
                async move {
                    let Elicitation::Mcp(ElicitRequestParams::FormElicitationParams { .. }) =
                        request
                    else {
                        anyhow::bail!("expected a standard form elicitation");
                    };
                    elicitations
                        .lock()
                        .map_err(|_| anyhow::anyhow!("elicitation lock poisoned"))?
                        .push(request_id.to_string());
                    Ok(ElicitationResponse {
                        action: ElicitationAction::Accept,
                        content: Some(json!({"confirmed": true})),
                        meta: None,
                    })
                }
                .boxed()
            }),
        )
        .await?;
    Ok((client, peer))
}

#[tokio::test]
async fn modern_tool_input_required_drives_a_second_round() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorded = Arc::clone(&calls);
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .respond_with(move |request: &Request| {
            let body: Value = request.body_json().expect("valid JSON-RPC request");
            match body["method"].as_str() {
                Some("server/discover") => modern_discovery(&body),
                Some("tools/call") => {
                    let attempt = {
                        let mut calls = recorded.lock().expect("calls lock");
                        calls.push(body.clone());
                        calls.len()
                    };
                    match attempt {
                        1 => input_required(&body),
                        2 => {
                            assert_eq!(
                                body.pointer("/params/requestState"),
                                Some(&json!("opaque-state"))
                            );
                            assert_eq!(
                                body.pointer("/params/inputResponses/confirmation/content"),
                                Some(&json!({"confirmed": true}))
                            );
                            rpc_result(
                                &body,
                                json!({
                                    "resultType": "complete",
                                    "content": [{"type": "text", "text": "completed"}],
                                }),
                            )
                        }
                        other => panic!("unexpected tools/call attempt {other}"),
                    }
                }
                other => panic!("unexpected modern MCP method {other:?}"),
            }
        })
        .expect(3)
        .mount(&server)
        .await;

    let elicitations = Arc::new(Mutex::new(Vec::new()));
    let (client, peer) = create_client(&server, Arc::clone(&elicitations)).await?;
    assert_eq!(peer.protocol_version, ProtocolVersion::V_2026_07_28);
    let result = client
        .call_tool(
            "confirm".to_string(),
            Some(json!({})),
            /*meta*/ None,
            Some(Duration::from_secs(5)),
        )
        .await?;

    assert_eq!(
        result.content[0].as_text().map(|text| text.text.as_str()),
        Some("completed")
    );
    assert_eq!(
        *elicitations.lock().expect("elicitation lock"),
        vec!["confirmation"]
    );
    assert_eq!(calls.lock().expect("calls lock").len(), 2);
    client.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn requested_modern_legacy_peer_rejects_resource_input_required() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .respond_with(|request: &Request| {
            let body: Value = request.body_json().expect("valid JSON-RPC request");
            match body["method"].as_str() {
                Some("server/discover") => ResponseTemplate::new(200).set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": body["id"],
                    "error": {"code": -32601, "message": "method not found"},
                })),
                Some("initialize") => legacy_initialize(&body),
                Some("notifications/initialized") => ResponseTemplate::new(202),
                Some("resources/read") => input_required(&body),
                other => panic!("unexpected legacy MCP method {other:?}"),
            }
        })
        .expect(4)
        .mount(&server)
        .await;

    let elicitations = Arc::new(Mutex::new(Vec::new()));
    let (client, peer) = create_client(&server, Arc::clone(&elicitations)).await?;
    assert_eq!(peer.protocol_version, ProtocolVersion::V_2025_06_18);
    client
        .read_resource(
            ReadResourceRequestParams::new("memo://legacy"),
            Some(Duration::from_secs(5)),
        )
        .await
        .expect_err("legacy peers must not enter the modern multi-round flow");

    assert!(elicitations.lock().expect("elicitation lock").is_empty());
    client.shutdown().await;
    Ok(())
}
