use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use codex_code_mode::CellId;
use codex_code_mode::CodeModeNestedToolCall;
use codex_code_mode::CodeModeSessionDelegate;
use codex_code_mode::CodeModeSessionProvider;
use codex_code_mode::ExecuteRequest;
use codex_code_mode::FunctionCallOutputContentItem;
use codex_code_mode::GrpcCodeModeSessionProvider;
use codex_code_mode::GrpcCodeModeHostCapability;
use codex_code_mode::NotificationFuture;
use codex_code_mode::NoopCodeModeSessionDelegate;
use codex_code_mode::RuntimeResponse;
use codex_code_mode::ToolInvocationFuture;
use codex_code_mode_protocol::grpc as proto;
use codex_code_mode_protocol::grpc::code_mode_host_client::CodeModeHostClient;
use codex_code_mode_protocol::grpc::bounded_code_mode_host_client;
use codex_code_mode_protocol::grpc::CLIENT_ID_METADATA_KEY;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc;
use tokio::sync::Notify;
use tokio::time::timeout;
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(unix)]
use tokio_stream::wrappers::UnixListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::Code;
use tonic::Request;
use tonic::transport::Channel;
use tonic::transport::Endpoint;
use tonic::transport::Server;
use tonic::transport::server::TcpConnectInfo;
use tonic::transport::server::TcpIncoming;
use tower::Layer;
use tower::Service;
use uuid::Uuid;

use super::loopback_grpc_service;
use super::authenticated_loopback_grpc_service;
use super::principal::PrincipalPolicy;

const TEST_CAPABILITY: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn capability() -> GrpcCodeModeHostCapability {
    GrpcCodeModeHostCapability::new(TEST_CAPABILITY).unwrap()
}

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

#[derive(Clone)]
struct DelayAcknowledgementLayer {
    committed: mpsc::Sender<()>,
    release: Arc<Notify>,
}

impl<S> Layer<S> for DelayAcknowledgementLayer {
    type Service = DelayAcknowledgement<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DelayAcknowledgement {
            inner,
            committed: self.committed.clone(),
            release: Arc::clone(&self.release),
        }
    }
}

#[derive(Clone)]
struct DelayAcknowledgement<S> {
    inner: S,
    committed: mpsc::Sender<()>,
    release: Arc<Notify>,
}

#[derive(Clone)]
struct OversizedHeaderLayer;

impl<S> Layer<S> for OversizedHeaderLayer {
    type Service = OversizedHeader<S>;

    fn layer(&self, inner: S) -> Self::Service {
        OversizedHeader { inner }
    }
}

#[derive(Clone)]
struct OversizedHeader<S> {
    inner: S,
}

impl<S, B, R> Service<tonic::codegen::http::Request<B>> for OversizedHeader<S>
where
    S: Service<
            tonic::codegen::http::Request<B>,
            Response = tonic::codegen::http::Response<R>,
        > + Send
        + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    R: Send + 'static,
{
    type Response = tonic::codegen::http::Response<R>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: tonic::codegen::http::Request<B>) -> Self::Future {
        let response = self.inner.call(request);
        Box::pin(async move {
            let mut response = response.await?;
            response.headers_mut().insert(
                "x-oversized-code-mode-header",
                "x".repeat(16 * 1_024).parse().expect("valid test header"),
            );
            Ok(response)
        })
    }
}

impl<S, B> Service<tonic::codegen::http::Request<B>> for DelayAcknowledgement<S>
where
    S: Service<tonic::codegen::http::Request<B>> + Send + 'static,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: tonic::codegen::http::Request<B>) -> Self::Future {
        let delay = request.uri().path().ends_with("/AcknowledgeNotification");
        let response = self.inner.call(request);
        let committed = self.committed.clone();
        let release = Arc::clone(&self.release);
        Box::pin(async move {
            let response = response.await?;
            if delay {
                let _ = committed.send(()).await;
                release.notified().await;
            }
            Ok(response)
        })
    }
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

async fn start_authenticated_server() -> (
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
            .add_service(authenticated_loopback_grpc_service(TEST_CAPABILITY.into()))
            .serve_with_incoming_shutdown(incoming, server_shutdown.cancelled_owned()),
    );
    (endpoint, shutdown, server)
}

async fn start_server_with_delayed_acknowledgement(
    committed: mpsc::Sender<()>,
    release: Arc<Notify>,
) -> (
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
            .layer(DelayAcknowledgementLayer { committed, release })
            .add_service(authenticated_loopback_grpc_service(TEST_CAPABILITY.into()))
            .serve_with_incoming_shutdown(incoming, server_shutdown.cancelled_owned()),
    );
    (endpoint, shutdown, server)
}

async fn start_server_with_oversized_response_header() -> (
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
            .layer(OversizedHeaderLayer)
            .add_service(authenticated_loopback_grpc_service(TEST_CAPABILITY.into()))
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

async fn connect_with_oversized_test_encoding(endpoint: &str) -> CodeModeHostClient<Channel> {
    let channel = Endpoint::from_shared(endpoint.to_string())
        .unwrap()
        .connect()
        .await
        .unwrap();
    CodeModeHostClient::new(channel)
        .max_decoding_message_size(proto::MAX_APPLICATION_MESSAGE_BYTES)
        .max_encoding_message_size(6 * 1_024 * 1_024)
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

async fn assert_provider_executes(provider: GrpcCodeModeSessionProvider) {
    let session = provider
        .create_session(Arc::new(NoopCodeModeSessionDelegate))
        .await
        .unwrap();
    let started = session
        .execute(ExecuteRequest {
            tool_call_id: "outer-call".to_string(),
            enabled_tools: Vec::new(),
            source: r#"text("connected");"#.to_string(),
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
            cell_id,
            content_items: vec![FunctionCallOutputContentItem::InputText {
                text: "connected".to_string(),
            }],
            error_text: None,
        })
    );
    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn canonical_transport_applies_the_application_cap_above_tonic_default() {
    let (endpoint, shutdown, server) = start_server().await;
    let mut client = connect_with_oversized_test_encoding(&endpoint).await;
    let client_id = Uuid::new_v4();
    let (session_id, mut events) = open_session(&mut client, client_id).await;
    let repeated = client
        .execute(identified_request(
            client_id,
            proto::ExecuteRequest {
                session_id: session_id.clone(),
                execution_id: "repeated-tools".to_string(),
                tool_call_id: "outer-call".to_string(),
                source: String::new(),
                enabled_tools: vec![
                    proto::ToolDefinition::default();
                    super::validation::MAX_TOOL_DEFINITIONS + 1
                ],
                yield_time_ms: None,
                max_output_tokens: None,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(repeated.code(), Code::ResourceExhausted);
    assert_eq!(repeated.message(), "code-mode-request-exhausted");
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
    assert_eq!(error.code(), Code::ResourceExhausted);
    assert_eq!(error.message(), "code-mode-request-exhausted");

    let mut execution = client
        .execute(identified_request(client_id, proto::ExecuteRequest {
            session_id: session_id.clone(),
            execution_id: "bounded-notification".to_string(),
            tool_call_id: "outer-call".to_string(),
            source: r#"notify("bounded"); text("done");"#.to_string(),
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
    assert_eq!(notification.text, "bounded");

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
async fn production_provider_rejects_oversized_response_headers() {
    let (endpoint, shutdown, server) = start_server_with_oversized_response_header().await;
    let provider = GrpcCodeModeSessionProvider::with_capability(endpoint, capability());

    let result = timeout(
        Duration::from_secs(/*secs*/ 5),
        provider.create_session(Arc::new(NoopCodeModeSessionDelegate)),
    )
    .await
    .expect("oversized response headers must fail promptly");
    assert!(result.is_err());

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
async fn existing_channels_support_grpc_code_mode_sessions() {
    let (endpoint, shutdown, server) = start_server().await;
    let channel = Endpoint::from_shared(endpoint)
        .unwrap()
        .connect()
        .await
        .unwrap();

    assert_provider_executes(GrpcCodeModeSessionProvider::with_channel(channel)).await;

    shutdown.cancel();
    server.await.unwrap().unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_socket_endpoints_support_grpc_code_mode_sessions() {
    let directory = tempfile::tempdir().unwrap();
    let socket_path = directory.path().join("grpc.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let server = tokio::spawn(
        Server::builder()
            .add_service(loopback_grpc_service())
            .serve_with_incoming_shutdown(
                UnixListenerStream::new(listener),
                server_shutdown.cancelled_owned(),
            ),
    );

    for endpoint in [
        format!("unix://{}", socket_path.display()),
        format!("unix:{}", socket_path.display()),
    ] {
        assert_provider_executes(GrpcCodeModeSessionProvider::new(endpoint)).await;
    }

    shutdown.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn production_provider_acknowledges_notifications_before_cell_closure() {
    let (endpoint, shutdown, server) = start_authenticated_server().await;
    let (events, mut event_rx) = mpsc::channel(/*buffer*/ 4);
    let session = GrpcCodeModeSessionProvider::with_capability(endpoint, capability())
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
    let (endpoint, shutdown, server) = start_authenticated_server().await;
    let (events, mut event_rx) = mpsc::channel(/*buffer*/ 4);
    let session = GrpcCodeModeSessionProvider::with_capability(endpoint, capability())
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

#[tokio::test]
async fn accepted_acknowledgement_retires_after_concurrent_termination() {
    let (committed_tx, mut committed_rx) = mpsc::channel(/*buffer*/ 1);
    let release = Arc::new(Notify::new());
    let (endpoint, shutdown, server) =
        start_server_with_delayed_acknowledgement(committed_tx, Arc::clone(&release)).await;
    let (events, mut event_rx) = mpsc::channel(/*buffer*/ 4);
    let session = GrpcCodeModeSessionProvider::with_capability(endpoint, capability())
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
    timeout(Duration::from_secs(/*secs*/ 5), committed_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(/*secs*/ 5), started.initial_response())
            .await
            .unwrap()
            .unwrap(),
        RuntimeResponse::Yielded { .. }
    ));
    assert!(session.terminate(cell_id.clone()).await.is_ok());
    release.notify_one();
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
        PrincipalPolicy::TrustedLocalTransport
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
        PrincipalPolicy::TrustedLocalTransport
            .principal(&request)
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
}
