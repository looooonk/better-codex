use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_client::RemoteAppServerConnectArgs;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_protocol::JSONRPCMessage;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

pub(super) async fn scripted_server(responses: Vec<(&'static str, Value)>) -> (RemoteAppServerClient, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let websocket_url = format!("ws://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut responses = responses.into_iter();
        let mut requests = Vec::new();
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let JSONRPCMessage::Request(request) = serde_json::from_str(&text).unwrap() else { continue; };
            let result = if request.method == "initialize" {
                json!({"userAgent":"parity-test"})
            } else {
                let (method, response) = responses.next().expect("unexpected request");
                assert_eq!(request.method, method);
                requests.push(request.params.unwrap_or(Value::Null));
                response
            };
            socket.send(Message::Text(json!({"id":request.id,"result":result}).to_string().into())).await.unwrap();
        }
        assert!(responses.next().is_none(), "expected request was never sent");
        requests
    });
    let client = RemoteAppServerClient::connect(RemoteAppServerConnectArgs {
        endpoint: RemoteAppServerEndpoint::WebSocket { websocket_url, auth_token: None },
        client_name: "parity-test".into(), client_version: "0.0.0".into(),
        experimental_api: true, required_human_approval: true,
        mcp_server_openai_form_elicitation: false, opt_out_notification_methods: Vec::new(), channel_capacity: 16,
    }).await.unwrap();
    (client, server)
}
