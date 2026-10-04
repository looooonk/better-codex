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
async fn native_workspace_requests_bound_unresponsive_and_oversized_servers() {
    for scenario in [
        "read_timeout",
        "write_timeout",
        "reset_timeout",
        "cycle",
        "oversized_page",
        "oversized_notice",
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let (received_tx, received_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut received_tx = Some(received_tx);
            let mut requests = 0;
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let JSONRPCMessage::Request(request) = serde_json::from_str(&text).unwrap() else {
                    continue;
                };
                let result = if request.method == "initialize" {
                    json!({"userAgent": "workspace-test"})
                } else {
                    requests += 1;
                    if let Some(sender) = received_tx.take() {
                        sender.send(()).unwrap();
                    }
                    if scenario.ends_with("timeout") {
                        continue;
                    }
                    assert_eq!(request.method, "thread/backgroundTerminals/list");
                    let terminal = json!({
                        "itemId": "item", "processId": "process", "cwd": "/workspace",
                        "command": if scenario == "oversized_notice" { "界".repeat(100_000) } else { "sleep 1".to_string() },
                        "osPid": null, "cpuPercent": null, "rssKb": null,
                    });
                    json!({
                        "data": vec![terminal; if scenario == "oversized_page" { 101 } else { 1 }],
                        "nextCursor": if scenario == "cycle" { Some("same-cursor") } else { None },
                    })
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
            requests
        });
        let client = RemoteAppServerClient::connect(RemoteAppServerConnectArgs {
            endpoint: RemoteAppServerEndpoint::WebSocket {
                websocket_url: url,
                auth_token: None,
            },
            client_name: "workspace-test".to_string(),
            client_version: "0.0.0".to_string(),
            experimental_api: true,
            mcp_server_openai_form_elicitation: false,
            opt_out_notification_methods: Vec::new(),
            channel_capacity: 8,
        })
        .await
        .unwrap();
        let request = match scenario {
            "write_timeout" => WorkspaceRequest::Rename("new name".to_string()),
            "reset_timeout" => WorkspaceRequest::Extension(
                super::super::extension_commands::ExtensionCommand::Usage { reset: true },
            ),
            _ => WorkspaceRequest::Background,
        };
        let thread_id = ThreadId::new();
        let task = tokio::spawn(execute(
            AppServerRequestHandle::Remote(client.request_handle()),
            thread_id,
            request.clone(),
        ));
        received_rx.await.unwrap();
        if scenario.ends_with("timeout") {
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(/*secs*/ 61)).await;
        }
        let result = task.await.unwrap();
        if scenario.ends_with("timeout") {
            tokio::time::resume();
        }
        match scenario {
            "oversized_notice" => {
                let WorkspaceResponse::Notice(message) = result.unwrap() else {
                    panic!("expected terminal list");
                };
                assert!(message.starts_with("Background terminals\nprocess: 界"));
                assert!(message.ends_with("\n[output truncated]"));
                assert!(message.len() <= 256 * 1024);
            }
            _ => assert_eq!(
                result.unwrap_err().to_string(),
                match scenario {
                    "read_timeout" => "Workspace request timed out",
                    "write_timeout" | "reset_timeout" =>
                        "Workspace action timed out. It may still complete; check the current state before retrying.",
                    "cycle" => "Background terminal pagination repeated a cursor",
                    "oversized_page" => "Background terminal page exceeds the requested limit",
                    _ => unreachable!(),
                }
            ),
        }
        if scenario.ends_with("timeout") {
            for _ in 0..3 {
                let error = execute(
                    AppServerRequestHandle::Remote(client.request_handle()),
                    thread_id,
                    request.clone(),
                )
                .await
                .unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("duplicate remote app-server request id"),
                    "{error}"
                );
            }
        }
        client.shutdown().await.unwrap();
        assert_eq!(
            server.await.unwrap(),
            if scenario == "cycle" { 2 } else { 1 }
        );
    }
}
