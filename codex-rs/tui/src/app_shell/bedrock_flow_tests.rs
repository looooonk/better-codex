use super::*;
use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_client::RemoteAppServerConnectArgs;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_protocol::JSONRPCMessage;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn reopening_timed_out_bedrock_discovery_does_not_accumulate_requests() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (received_tx, received_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut received_tx = Some(received_tx);
        let mut discoveries = 0;
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let JSONRPCMessage::Request(request) = serde_json::from_str(&text).unwrap() else {
                continue;
            };
            if request.method == "initialize" {
                socket
                    .send(Message::Text(
                        json!({"id": request.id, "result": {"userAgent": "bedrock-test"}})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
            } else {
                assert_eq!(request.method, "account/bedrock/discover");
                discoveries += 1;
                if let Some(sender) = received_tx.take() {
                    sender.send(()).unwrap();
                }
            }
        }
        discoveries
    });
    let client = RemoteAppServerClient::connect(RemoteAppServerConnectArgs {
        endpoint: RemoteAppServerEndpoint::WebSocket {
            websocket_url: url,
            auth_token: None,
        },
        client_name: "bedrock-test".to_string(),
        client_version: "0.0.0".to_string(),
        experimental_api: true,
        mcp_server_openai_form_elicitation: false,
        opt_out_notification_methods: Vec::new(),
        channel_capacity: 8,
    })
    .await
    .unwrap();
    let mut flow = BedrockFlow::new(AppServerRequestHandle::Remote(client.request_handle()));
    received_rx.await.unwrap();
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(/*secs*/ 16)).await;
    assert!(!flow.complete().await);
    tokio::time::resume();
    assert_eq!(
        flow.error.as_deref(),
        Some("Unable to check AWS credentials: Credential discovery timed out")
    );
    drop(flow);
    for _ in 0..3 {
        let mut flow = BedrockFlow::new(AppServerRequestHandle::Remote(client.request_handle()));
        assert!(!flow.complete().await);
        assert!(
            flow.error
                .as_deref()
                .is_some_and(|error| error.contains("duplicate remote app-server request id"))
        );
    }
    client.shutdown().await.unwrap();
    assert_eq!(server.await.unwrap(), 1);
}

#[tokio::test]
async fn native_bedrock_requests_retry_credentials_and_require_govcloud_acknowledgement() {
    for (credential, method, params) in [
        (
            BedrockCredential::Profile("work".to_string()),
            "account/bedrock/setup",
            json!({"type": "profile", "profile": "work", "region": "us-gov-west-1"}),
        ),
        (
            BedrockCredential::Environment,
            "account/bedrock/setup",
            json!({"type": "environment", "region": "us-gov-west-1"}),
        ),
        (
            BedrockCredential::ApiKey("test-api-key".to_string()),
            "account/login/start",
            json!({"type": "amazonBedrock", "apiKey": "test-api-key", "region": "us-gov-west-1"}),
        ),
        (
            BedrockCredential::AccessKeys {
                access_key_id: "test-access-key".to_string(),
                secret_access_key: "test-secret".to_string(),
                session_token: Some("test-token".to_string()),
            },
            "account/login/start",
            json!({"type": "amazonBedrockAccessKeys", "accessKeyId": "test-access-key", "secretAccessKey": "test-secret", "sessionToken": "test-token", "region": "us-gov-west-1"}),
        ),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut writes = 0;
            let mut checks = 0;
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let JSONRPCMessage::Request(request) = serde_json::from_str(&text).unwrap() else {
                    continue;
                };
                let result = match request.method.as_str() {
                    "initialize" => json!({"userAgent": "bedrock-test"}),
                    "account/bedrock/discover" => {
                        json!({"profiles": [], "environmentCredentials": []})
                    }
                    "account/bedrock/checkGovCloudRequirements" => {
                        checks += 1;
                        json!({"isGovCloud": true, "shouldWarn": true})
                    }
                    _ => {
                        assert_eq!(
                            (request.method.as_str(), request.params),
                            (method, Some(params.clone()))
                        );
                        writes += 1;
                        if writes == 1 {
                            socket.send(Message::Text(json!({"id": request.id, "error": {"code": -32600, "message": "Try again"}}).to_string().into())).await.unwrap();
                            continue;
                        }
                        if method == "account/login/start" {
                            json!({"type": "amazonBedrock"})
                        } else {
                            json!({})
                        }
                    }
                };
                socket
                    .send(Message::Text(
                        json!({"id": request.id, "result": result})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
            }
            (writes, checks)
        });
        let client = RemoteAppServerClient::connect(RemoteAppServerConnectArgs {
            endpoint: RemoteAppServerEndpoint::WebSocket {
                websocket_url: url,
                auth_token: None,
            },
            client_name: "bedrock-test".to_string(),
            client_version: "0.0.0".to_string(),
            experimental_api: true,
            mcp_server_openai_form_elicitation: false,
            opt_out_notification_methods: Vec::new(),
            channel_capacity: 8,
        })
        .await
        .unwrap();
        let mut flow = BedrockFlow::new(AppServerRequestHandle::Remote(client.request_handle()));
        assert!(!flow.complete().await);
        flow.state
            .enter_region(credential, "us-gov-west-1".to_string());
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert!(flow.handle_key(&enter).is_none());
        assert!(
            flow.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .is_none()
        );
        assert!(!flow.complete().await);
        assert!(
            matches!(&flow.state.view, BedrockView::RegionEntry { value, .. } if value == "us-gov-west-1")
        );
        assert!(
            flow.error
                .as_deref()
                .is_some_and(|error| error.contains("Try again"))
        );
        assert!(flow.handle_key(&enter).is_none());
        assert!(!flow.complete().await);
        assert!(!flow.complete().await);
        assert!(matches!(flow.state.view, BedrockView::GovCloudWarning(_)));
        assert!(matches!(
            flow.handle_key(&enter),
            Some(FlowAction::Configured)
        ));
        flow.pending = Some(tokio::spawn(async { Completion::SetupTimedOut }));
        assert!(!flow.complete().await);
        assert!(flow.handle_key(&enter).is_none());
        assert!(matches!(
            flow.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(FlowAction::Exit)
        ));
        let area = Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 72, /*height*/ 10,
        );
        let mut buffer = Buffer::empty(area);
        flow.render(area, &mut buffer);
        let text = buffer
            .content
            .chunks(usize::from(area.width))
            .map(|row| {
                row.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!("bedrock_setup_timeout", text);
        drop(flow);
        client.shutdown().await.unwrap();
        assert_eq!(server.await.unwrap(), (2, 1));
    }
}
