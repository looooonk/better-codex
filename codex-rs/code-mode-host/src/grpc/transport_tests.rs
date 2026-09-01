use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use codex_code_mode::CellId;
use codex_code_mode::CodeModeNestedToolCall;
use codex_code_mode::CodeModeSessionDelegate;
use codex_code_mode::CodeModeSessionProvider;
use codex_code_mode::ExecuteRequest;
use codex_code_mode::FunctionCallOutputContentItem;
use codex_code_mode::GrpcCodeModeSessionProvider;
use codex_code_mode::NotificationFuture;
use codex_code_mode::RuntimeResponse;
use codex_code_mode::ToolInvocationFuture;
use codex_code_mode_protocol::grpc as proto;
use codex_code_mode_protocol::grpc::code_mode_host_client::CodeModeHostClient;
use codex_code_mode_protocol::grpc::bounded_code_mode_host_client;
use codex_code_mode_protocol::grpc::CLIENT_ID_METADATA_KEY;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tonic::Code;
use tonic::Request;
use tonic::transport::Channel;
use tonic::transport::Endpoint;
use tonic::transport::Server;
use tonic::transport::server::TcpConnectInfo;
use tonic::transport::server::TcpIncoming;
use uuid::Uuid;

use super::loopback_grpc_service;
use super::principal::PrincipalPolicy;

#[derive(Debug, Eq, PartialEq)]
enum DelegateEvent {
    Notification(String),
    NotificationCancelled,
    CellClosed(CellId),
}

struct RecordingDelegate {
    events: mpsc::Sender<DelegateEvent>,
    hold_notifications: bool,
}

impl CodeModeSessionDelegate for RecordingDelegate {
    fn invoke_tool<'a>(
        &'a self,
        _invocation: CodeModeNestedToolCall,
        _cancellation_token: CancellationToken,
    ) -> ToolInvocationFuture<'a> {
        Box::pin(async { Err("unexpected tool call".to_string()) })
    }

    fn notify<'a>(
        &'a self,
        _call_id: String,
        _cell_id: CellId,
        text: String,
        cancellation_token: CancellationToken,
    ) -> NotificationFuture<'a> {
        Box::pin(async move {
            self.events
                .send(DelegateEvent::Notification(text))
                .await
                .map_err(|_| "delegate event receiver closed".to_string())?;
            if self.hold_notifications {
                cancellation_token.cancelled().await;
                self.events
                    .send(DelegateEvent::NotificationCancelled)
                    .await
                    .map_err(|_| "delegate event receiver closed".to_string())?;
            }
            Ok(())
        })
    }

    fn cell_closed(&self, cell_id: &CellId) {
        let _ = self
            .events
            .try_send(DelegateEvent::CellClosed(cell_id.clone()));
    }
}

async fn start_server() -> (
    String,
    CancellationToken,
    tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
) {
    let incoming = TcpIncoming::bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let endpoint = format!("http://{}", incoming.local_addr().unwrap());
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let server = tokio::spawn(
        Server::builder()
            .add_service(loopback_grpc_service())
            .serve_with_incoming_shutdown(incoming, server_shutdown.cancelled_owned()),
    );
    (endpoint, shutdown, server)
}

async fn connect(endpoint: &str) -> CodeModeHostClient<Channel> {
    let channel = Endpoint::from_shared(endpoint.to_string())
        .unwrap()
        .connect()
        .await
        .unwrap();
    bounded_code_mode_host_client(channel)
}

fn identified_request<T>(client_id: Uuid, message: T) -> Request<T> {
    let mut request = Request::new(message);
    request.metadata_mut().insert(
        CLIENT_ID_METADATA_KEY,
        client_id.to_string().parse().unwrap(),
    );
    request
}

async fn open_session(
    client: &mut CodeModeHostClient<Channel>,
    client_id: Uuid,
) -> (String, tonic::Streaming<proto::SessionEvent>) {
    let mut events = client
        .open_session(identified_request(
            client_id,
            proto::OpenSessionRequest::default(),
        ))
        .await
        .unwrap()
        .into_inner();
    let opened = events.message().await.unwrap().unwrap();
    let Some(proto::session_event::Event::Opened(opened)) = opened.event else {
        panic!("expected session opened event");
    };
    (opened.session_id, events)
}

#[tokio::test]
async fn canonical_transport_accepts_messages_above_tonic_default() {
    let (endpoint, shutdown, server) = start_server().await;
    let mut client = connect(&endpoint).await;
    let client_id = Uuid::new_v4();
    let (session_id, mut events) = open_session(&mut client, client_id).await;
    let oversized_json = serde_json::to_vec(&"x".repeat(5 * 1_024 * 1_024)).unwrap();

    let error = client
        .complete_tool_call(identified_request(client_id, proto::CompleteToolCallRequest {
            session_id: session_id.clone(),
            invocation_id: Uuid::new_v4().to_string(),
            outcome: Some(proto::complete_tool_call_request::Outcome::Succeeded(
                proto::ToolCallSucceeded {
                    output_json: oversized_json,
                },
            )),
        }))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::NotFound);

    let mut execution = client
        .execute(identified_request(client_id, proto::ExecuteRequest {
            session_id: session_id.clone(),
            execution_id: "large-notification".to_string(),
            tool_call_id: "outer-call".to_string(),
            source: r#"notify("x".repeat(5 * 1024 * 1024)); text("done");"#.to_string(),
            enabled_tools: Vec::new(),
            yield_time_ms: Some(60_000),
            max_output_tokens: None,
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        execution.message().await.unwrap().unwrap().event,
        Some(proto::execute_event::Event::Started(_))
    ));
    let notification = events.message().await.unwrap().unwrap();
    let Some(proto::session_event::Event::Notification(notification)) = notification.event else {
        panic!("expected notification event");
    };
    assert_eq!(notification.text.len(), 5 * 1_024 * 1_024);

    client
        .acknowledge_notification(identified_request(client_id, proto::AcknowledgeNotificationRequest {
            session_id: session_id.clone(),
            notification_id: notification.notification_id,
        }))
        .await
        .unwrap();
    assert!(matches!(
        execution.message().await.unwrap().unwrap().event,
        Some(proto::execute_event::Event::Outcome(
            proto::ExecutionOutcome {
                outcome: Some(proto::execution_outcome::Outcome::Completed(_)),
                ..
            }
        ))
    ));
    assert!(matches!(
        events.message().await.unwrap().unwrap().event,
        Some(proto::session_event::Event::CellClosed(_))
    ));
    client
        .close_session(identified_request(client_id, proto::CloseSessionRequest { session_id }))
        .await
        .unwrap();
    shutdown.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn sessions_are_bound_to_client_identity_across_tcp_connections() {
    let (endpoint, shutdown, server) = start_server().await;
    let mut owner = connect(&endpoint).await;
    let mut other = connect(&endpoint).await;
    let mut reconnected_owner = connect(&endpoint).await;
    let owner_id = Uuid::new_v4();
    let other_id = Uuid::new_v4();
    let (session_id, _events) = open_session(&mut owner, owner_id).await;

    let error = other
        .close_session(identified_request(other_id, proto::CloseSessionRequest {
            session_id: session_id.clone(),
        }))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::PermissionDenied);
    reconnected_owner
        .close_session(identified_request(owner_id, proto::CloseSessionRequest { session_id }))
        .await
        .unwrap();

    shutdown.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn production_provider_acknowledges_notifications_before_cell_closure() {
    let (endpoint, shutdown, server) = start_server().await;
    let (events, mut event_rx) = mpsc::channel(/*buffer*/ 4);
    let session = GrpcCodeModeSessionProvider::new(endpoint)
        .create_session(Arc::new(RecordingDelegate {
            events,
            hold_notifications: false,
        }))
        .await
        .unwrap();
    let started = session
        .execute(ExecuteRequest {
            tool_call_id: "outer-call".to_string(),
            enabled_tools: Vec::new(),
            source: r#"notify("notice"); text("done");"#.to_string(),
            yield_time_ms: Some(1_000),
            max_output_tokens: None,
        })
        .await
        .unwrap();
    let cell_id = started.cell_id.clone();

    assert_eq!(
        timeout(Duration::from_secs(/*secs*/ 5), started.initial_response())
            .await
            .unwrap(),
        Ok(RuntimeResponse::Result {
            cell_id: cell_id.clone(),
            content_items: vec![FunctionCallOutputContentItem::InputText {
                text: "done".to_string(),
            }],
            error_text: None,
        })
    );
    assert_eq!(
        timeout(Duration::from_secs(/*secs*/ 5), event_rx.recv())
            .await
            .unwrap(),
        Some(DelegateEvent::Notification("notice".to_string()))
    );
    assert_eq!(
        timeout(Duration::from_secs(/*secs*/ 5), event_rx.recv())
            .await
            .unwrap(),
        Some(DelegateEvent::CellClosed(cell_id))
    );
    session.shutdown().await.unwrap();
    shutdown.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn production_provider_cancels_pending_notifications_on_termination() {
    let (endpoint, shutdown, server) = start_server().await;
    let (events, mut event_rx) = mpsc::channel(/*buffer*/ 4);
    let session = GrpcCodeModeSessionProvider::new(endpoint)
        .create_session(Arc::new(RecordingDelegate {
            events,
            hold_notifications: true,
        }))
        .await
        .unwrap();
    let started = session
        .execute(ExecuteRequest {
            tool_call_id: "outer-call".to_string(),
            enabled_tools: Vec::new(),
            source: r#"notify("pending"); await new Promise(() => {});"#.to_string(),
            yield_time_ms: Some(1),
            max_output_tokens: None,
        })
        .await
        .unwrap();
    let cell_id = started.cell_id.clone();

    assert_eq!(
        timeout(Duration::from_secs(/*secs*/ 5), event_rx.recv())
            .await
            .unwrap(),
        Some(DelegateEvent::Notification("pending".to_string()))
    );
    assert!(matches!(
        timeout(Duration::from_secs(/*secs*/ 5), started.initial_response())
            .await
            .unwrap()
            .unwrap(),
        RuntimeResponse::Yielded { .. }
    ));
    assert!(session.terminate(cell_id.clone()).await.is_ok());
    assert_eq!(
        timeout(Duration::from_secs(/*secs*/ 5), event_rx.recv())
            .await
            .unwrap(),
        Some(DelegateEvent::NotificationCancelled)
    );
    assert_eq!(
        timeout(Duration::from_secs(/*secs*/ 5), event_rx.recv())
            .await
            .unwrap(),
        Some(DelegateEvent::CellClosed(cell_id))
    );
    session.shutdown().await.unwrap();
    shutdown.cancel();
    server.await.unwrap().unwrap();
}

#[test]
fn loopback_policy_rejects_non_loopback_peers() {
    let mut request = Request::new(());
    request.extensions_mut().insert(TcpConnectInfo {
        local_addr: Some("192.0.2.1:4000".parse::<SocketAddr>().unwrap()),
        remote_addr: Some("198.51.100.2:5000".parse::<SocketAddr>().unwrap()),
    });

    assert_eq!(
        PrincipalPolicy::LoopbackTcp
            .principal(&request)
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
}

#[test]
fn loopback_policy_rejects_missing_client_identity() {
    let mut request = Request::new(());
    request.extensions_mut().insert(TcpConnectInfo {
        local_addr: Some("127.0.0.1:4000".parse::<SocketAddr>().unwrap()),
        remote_addr: Some("127.0.0.1:5000".parse::<SocketAddr>().unwrap()),
    });

    assert_eq!(
        PrincipalPolicy::LoopbackTcp
            .principal(&request)
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
}
