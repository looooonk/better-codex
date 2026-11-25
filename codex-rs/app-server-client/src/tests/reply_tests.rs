//! Real WebSocket workers retain independent replies even when request IDs collide.
use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn remote_reply_handles_keep_original_connection() {
    let mut clients = Vec::new();
    let mut replies = Vec::new();
    let mut stale_replies = Vec::new();
    let mut observed = Vec::new();
    for label in ["first", "second"] {
        let (done_tx, done_rx) = oneshot::channel();
        let websocket_url = start_test_remote_server(move |mut websocket| async move {
            expect_remote_initialize(&mut websocket).await;
            write_websocket_message(&mut websocket, JSONRPCMessage::Request(JSONRPCRequest {
                id: RequestId::Integer(1), method: "item/tool/requestUserInput".into(),
                params: Some(serde_json::json!({"threadId":"same-thread", "turnId":"same-turn", "itemId":"same-item", "questions":[], "isBlocking":true})),
                trace: None,
            })).await;
            let response = read_websocket_message(&mut websocket).await;
            done_tx.send((label, response)).unwrap();
            while websocket.next().await.is_some() {}
        }).await;
        let mut client = AppServerClient::Remote(
            RemoteAppServerClient::connect(test_remote_connect_args(websocket_url))
                .await
                .unwrap(),
        );
        let AppServerEvent::ServerRequest(request) = client.next_event().await.unwrap() else {
            panic!("expected request")
        };
        replies.push(
            client
                .request_handle()
                .server_request_reply_handle(request.id().clone()),
        );
        stale_replies.push(
            client
                .request_handle()
                .server_request_reply_handle(RequestId::Integer(2)),
        );
        clients.push(client);
        observed.push(done_rx);
    }
    replies[1]
        .reject(JSONRPCErrorError {
            code: -32000,
            message: "second".into(),
            data: None,
        })
        .await
        .unwrap();
    replies[0]
        .resolve(serde_json::json!({"first": true}))
        .await
        .unwrap();
    for receiver in observed {
        let (label, message) = timeout(Duration::from_secs(/*secs*/ 5), receiver)
            .await
            .unwrap()
            .unwrap();
        match message {
            JSONRPCMessage::Response(response) => assert_eq!(
                (label, response.id, response.result),
                (
                    "first",
                    RequestId::Integer(1),
                    serde_json::json!({"first": true})
                )
            ),
            JSONRPCMessage::Error(error) => assert_eq!(
                (label, error.id, error.error.message),
                ("second", RequestId::Integer(1), "second".into())
            ),
            _ => panic!("expected response or rejection"),
        }
    }
    for client in clients {
        client.shutdown().await.unwrap();
    }
    for reply in replies {
        assert_eq!(
            reply
                .resolve(serde_json::json!({}))
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
    }
    for reply in stale_replies {
        assert_eq!(
            reply
                .resolve(serde_json::json!({}))
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::BrokenPipe
        );
    }
}

#[tokio::test]
async fn persistent_tool_suggestion_reply_requires_a_private_settings_server() {
    for (agent, private) in [
        ("codex/1", false),
        ("codex/1 better-codex-private-config/1", true),
    ] {
        let (done_tx, done_rx) = oneshot::channel();
        let url = start_test_remote_server(move |mut websocket| async move {
            expect_remote_initialize_with_metadata(&mut websocket, serde_json::json!({"userAgent": agent})).await;
            write_websocket_message(&mut websocket, JSONRPCMessage::Request(JSONRPCRequest {
                id: RequestId::Integer(7),
                method: "mcpServer/elicitation/request".into(),
                params: Some(serde_json::json!({
                    "threadId": "thread", "serverName": "codex", "mode":"form", "message":"Install?",
                    "requestedSchema":{"type":"object","properties":{}},
                    "_meta":{"codex_approval_kind":"tool_suggestion","persist":"always"}
                })),
                trace: None,
            })).await;
            done_tx.send(read_websocket_message(&mut websocket).await).unwrap();
            while websocket.next().await.is_some() {}
        }).await;
        let mut client = RemoteAppServerClient::connect(test_remote_connect_args(url))
            .await
            .unwrap();
        let AppServerEvent::ServerRequest(request) = client.next_event().await.unwrap() else {
            panic!("expected a tool suggestion");
        };
        let shown = serde_json::to_value(request.as_ref()).unwrap();
        assert_eq!(shown["params"]["_meta"].get("persist").is_some(), private);
        let result = client
            .resolve_server_request(
                request.id().clone(),
                serde_json::json!({
                    "action":"decline", "_meta":{"persist":"always"}
                }),
            )
            .await;
        assert_eq!(
            result.err().map(|e| e.kind()),
            (!private).then_some(ErrorKind::Unsupported)
        );
        let message = done_rx.await.unwrap();
        assert_eq!(matches!(message, JSONRPCMessage::Response(_)), private);
        client.shutdown().await.unwrap();
    }
}
