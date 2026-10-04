use super::*;
use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_client::RemoteAppServerConnectArgs;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_protocol::JSONRPCMessage;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn stalled_metadata_commands_keep_a_bounded_request_namespace() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let websocket_url = format!("ws://{}", listener.local_addr().unwrap());
    let (received_tx, mut received_rx) = tokio::sync::mpsc::channel(8);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut commands = 0;
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let JSONRPCMessage::Request(request) = serde_json::from_str(&text).unwrap() else {
                continue;
            };
            if request.method == "initialize" {
                socket
                    .send(Message::Text(
                        json!({"id": request.id, "result": {"userAgent": "metadata-test"}})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
            } else {
                assert_eq!(request.method, "command/exec");
                let params: CommandExecParams =
                    serde_json::from_value(request.params.unwrap()).unwrap();
                assert_eq!(
                    (
                        params.output_bytes_cap,
                        params.timeout_ms,
                        params.disable_timeout
                    ),
                    (Some(131072), Some(5000), false)
                );
                commands += 1;
                received_tx.send(()).await.unwrap();
            }
        }
        commands
    });
    let client = RemoteAppServerClient::connect(RemoteAppServerConnectArgs {
        endpoint: RemoteAppServerEndpoint::WebSocket {
            websocket_url,
            auth_token: None,
        },
        client_name: "metadata-test".into(),
        client_version: "0.0.0".into(),
        experimental_api: true,
        mcp_server_openai_form_elicitation: false,
        opt_out_notification_methods: Vec::new(),
        channel_capacity: 32,
    })
    .await
    .unwrap();
    let runner = Arc::new(MetadataRunner {
        handle: AppServerRequestHandle::Remote(client.request_handle()),
        next_slot: AtomicUsize::new(0),
    });
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let runner = runner.clone();
        tasks.spawn(async move {
            let mut command = WorkspaceCommand::new(["git", "status"]);
            command.timeout = Duration::from_secs(60);
            command.output_bytes_cap = 1_000_000;
            tokio::time::timeout(Duration::from_secs(10), runner.run(command)).await
        });
    }
    for _ in 0..8 {
        received_rx.recv().await.unwrap();
    }
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(11)).await;
    let mut expired = 0;
    let mut duplicates = 0;
    while let Some(result) = tasks.join_next().await {
        match result.unwrap() {
            Err(_) => expired += 1,
            Ok(Err(error))
                if error
                    .to_string()
                    .contains("duplicate remote app-server request id") =>
            {
                duplicates += 1
            }
            value => panic!("unexpected metadata result: {value:?}"),
        }
    }
    tokio::time::resume();
    assert_eq!((expired, duplicates), (8, 8));
    client.shutdown().await.unwrap();
    assert_eq!(server.await.unwrap(), 8);
}

#[test]
fn stale_metadata_cannot_replace_the_active_workspace() {
    let mut shell = ShellState::snapshot_fixture();
    let old = StatusMetadata {
        cwd: "/old".into(),
        branch: Some("old-branch".into()),
        ..StatusMetadata::default()
    };
    let thread_id = shell.thread_id;
    shell.complete_status_metadata(thread_id, "/old".into(), Some(old));
    assert!(shell.status_surfaces.metadata.for_cwd(&shell.cwd).is_none());
    let fresh = StatusMetadata {
        cwd: shell.cwd.clone(),
        branch: Some("active-branch".into()),
        ..StatusMetadata::default()
    };
    shell.complete_status_metadata(thread_id, shell.cwd.clone(), Some(fresh));
    assert_eq!(
        shell.status_surface_value(StatusLineItem::GitBranch),
        Some("active-branch".into())
    );
    shell.complete_status_metadata(
        codex_protocol::ThreadId::new(),
        shell.cwd.clone(),
        Some(StatusMetadata::default()),
    );
    assert_eq!(
        shell.status_surface_value(StatusLineItem::GitBranch),
        Some("active-branch".into())
    );
}
