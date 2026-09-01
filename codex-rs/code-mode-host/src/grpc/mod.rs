mod conversions;
mod delegate;
mod events;
mod principal;
mod routing;
mod session;
mod validation;
mod waits;

use std::future::Future;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use codex_code_mode_protocol::CellId;
use codex_code_mode_protocol::WaitRequest;
use codex_code_mode_protocol::grpc as proto;
use codex_code_mode_protocol::grpc::code_mode_host_server::CodeModeHost;
use codex_code_mode_protocol::grpc::code_mode_host_server::CodeModeHostServer;
use codex_code_mode_protocol::grpc::MAX_APPLICATION_MESSAGE_BYTES;
use futures::Stream;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Request;
use tonic::Response;
use tonic::Status;
use tonic::body::Body;
use tonic::codegen::http::Request as HttpRequest;
use tonic::codegen::http::Response as HttpResponse;
use tonic::server::NamedService;
use tower::Layer;
use tower::Service;

use self::session::GrpcHostState;
use self::session::GrpcSession;
use self::waits::WaitRegistration;
use self::principal::PrincipalPolicy;
use self::principal::RequestPrincipal;
use crate::transport_admission::GrpcAdmission;
use crate::transport_admission::GrpcAdmissionLayer;

type GrpcStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;
type GrpcFuture<'a, T> = Pin<Box<dyn Future<Output = Result<Response<T>, Status>> + Send + 'a>>;

/// Serves transport-independent, leased code-mode sessions over gRPC.
#[derive(Clone)]
pub struct GrpcCodeModeHost {
    state: Arc<GrpcHostState>,
    principal_policy: PrincipalPolicy,
}

impl GrpcCodeModeHost {
    /// Creates a trusted in-process host without transport caller checks.
    ///
    /// Embedders must not expose this service to an untrusted transport.
    /// [`loopback_grpc_service`] likewise requires an independently trusted
    /// local caller boundary. The host CLI owns the capability-authenticated
    /// production TCP listener.
    pub fn new() -> Self {
        Self {
            state: Arc::new(GrpcHostState::new()),
            principal_policy: PrincipalPolicy::InProcess,
        }
    }

    fn loopback_tcp(principal_policy: PrincipalPolicy) -> Self {
        Self {
            state: Arc::new(GrpcHostState::new()),
            principal_policy,
        }
    }

    async fn open_session_request(
        &self,
        request: proto::OpenSessionRequest,
        principal: RequestPrincipal,
    ) -> Result<Response<GrpcStream<proto::SessionEvent>>, Status> {
        let _permit = self.state.request_permit()?;
        let limits = conversions::session_limits(request.cell_execution_limits)?;
        let stream = self.state.open_session(limits, principal.identity())?;
        principal.authorize();
        Ok(Response::new(stream))
    }

    async fn close_session_request(
        &self,
        request: proto::CloseSessionRequest,
        principal: RequestPrincipal,
    ) -> Result<Response<proto::CloseSessionResponse>, Status> {
        let _permit = self.state.control_permit()?;
        self.state
            .close_session(&request.session_id, principal.identity())
            .await?;
        principal.authorize();
        Ok(Response::new(proto::CloseSessionResponse {}))
    }

    async fn subscribe_request(
        &self,
        request: proto::SubscribeToToolCallsRequest,
        principal: RequestPrincipal,
    ) -> Result<Response<GrpcStream<proto::ToolCall>>, Status> {
        let _permit = self.state.request_permit()?;
        let session = self
            .state
            .session_for_principal(&request.session_id, principal.identity())?;
        let stream = session.subscribe(request.tool_names)?;
        principal.authorize();
        Ok(Response::new(stream))
    }

    async fn complete_tool_request(
        &self,
        request: proto::CompleteToolCallRequest,
        principal: RequestPrincipal,
    ) -> Result<Response<proto::CompleteToolCallResponse>, Status> {
        let _permit = self.state.control_permit()?;
        let session = self
            .state
            .session_for_principal(&request.session_id, principal.identity())?;
        principal.authorize();
        let invocation_id = validation::uuid(&request.invocation_id, "tool invocation ID")?;
        let result = match request.outcome {
            Some(proto::complete_tool_call_request::Outcome::Succeeded(result)) => Ok(
                codex_code_mode_protocol::parse_bounded_json(&result.output_json).map_err(|error| {
                    Status::invalid_argument(format!("invalid code-mode tool output JSON: {error}"))
                })?,
            ),
            Some(proto::complete_tool_call_request::Outcome::Failed(error)) => {
                validation::bounded(
                    &error.message,
                    validation::MAX_TOOL_ERROR_BYTES,
                    "tool error message",
                )?;
                Err(error.message)
            }
            None => {
                return Err(Status::invalid_argument(
                    "tool completion is missing its outcome",
                ));
            }
        };
        session.complete_invocation(invocation_id, result)?;
        Ok(Response::new(proto::CompleteToolCallResponse {}))
    }

    async fn acknowledge_notification_request(
        &self,
        request: proto::AcknowledgeNotificationRequest,
        principal: RequestPrincipal,
    ) -> Result<Response<proto::AcknowledgeNotificationResponse>, Status> {
        let _permit = self.state.control_permit()?;
        let session = self
            .state
            .session_for_principal(&request.session_id, principal.identity())?;
        principal.authorize();
        let notification_id = validation::uuid(&request.notification_id, "notification ID")?;
        session.acknowledge_notification(notification_id)?;
        Ok(Response::new(proto::AcknowledgeNotificationResponse {}))
    }

    async fn execute_request(
        &self,
        request: proto::ExecuteRequest,
        principal: RequestPrincipal,
    ) -> Result<Response<GrpcStream<proto::ExecuteEvent>>, Status> {
        let session = self
            .state
            .session_for_principal(&request.session_id, principal.identity())?;
        principal.authorize();
        validation::identifier(&request.execution_id, "execution ID")?;
        let request_permit = self.state.request_permit()?;
        let execution_id = request.execution_id.clone();
        let request = conversions::execute_request(request)?;
        let cell_permit = self.state.cell_permit()?;
        session.reserve_execution(&execution_id)?;
        let admission = ExecutionAdmission {
            session: Arc::clone(&session),
            execution_id: Some(execution_id.clone()),
        };
        let started = tokio::select! {
            _ = session.closed.cancelled() => {
                return Err(Status::cancelled("code-mode session is closed"));
            }
            result = session.runtime.execute(request) => {
                result.map_err(Status::failed_precondition)?
            }
        };
        let cell_id = started.cell_id.clone();
        session.admit_execution(execution_id.clone(), cell_id.to_string(), cell_permit)?;

        let (sender, receiver) = mpsc::channel(/*buffer*/ 2);
        sender
            .try_send(Ok(proto::ExecuteEvent {
                event: Some(proto::execute_event::Event::Started(
                    proto::ExecutionStarted {
                        execution_id,
                        cell_id: cell_id.to_string(),
                    },
                )),
            }))
            .map_err(|_| Status::internal("failed to publish code-mode execution admission"))?;
        let response_session = Arc::clone(&session);
        let response_task_registered = session.spawn_task(async move {
            let _request_permit = request_permit;
            tokio::select! {
                biased;
                _ = sender.closed() => {}
                response = started.initial_response() => {
                    let event = response
                        .map_err(Status::internal)
                        .and_then(conversions::execute_event);
                    let _ = sender.send(event).await;
                }
                _ = response_session.closed.cancelled() => {}
            }
        });
        if !response_task_registered {
            return Err(Status::cancelled("code-mode session is closed"));
        }

        Ok(Response::new(execution_stream(receiver, admission)))
    }

    async fn wait_request(
        &self,
        request: proto::WaitRequest,
        principal: RequestPrincipal,
    ) -> Result<Response<proto::WaitResponse>, Status> {
        let session = self
            .state
            .session_for_principal(&request.session_id, principal.identity())?;
        principal.authorize();
        validation::identifier(&request.cell_id, "cell ID")?;
        validation::identifier(&request.wait_id, "wait ID")?;
        let _permit = self.state.request_permit()?;
        let registration = WaitRegistration::new(Arc::clone(&session), request.wait_id)?;
        let request = WaitRequest {
            cell_id: CellId::new(request.cell_id),
            yield_time_ms: request.yield_time_ms,
        };
        let outcome = tokio::select! {
            biased;
            _ = registration.cancellation().cancelled() => {
                return Err(Status::cancelled("code-mode wait was cancelled"));
            }
            _ = session.closed.cancelled() => {
                return Err(Status::cancelled("code-mode session is closed"));
            }
            outcome = session.runtime.wait(request) => {
                outcome.map_err(Status::failed_precondition)?
            }
        };
        let response = conversions::wait_response(outcome)?;
        if let Some(cell_id) = terminal_wait_cell_id(&response) {
            session.terminal_outcome_observed(cell_id);
        }
        Ok(Response::new(response))
    }

    async fn cancel_wait_request(
        &self,
        request: proto::CancelWaitRequest,
        principal: RequestPrincipal,
    ) -> Result<Response<proto::CancelWaitResponse>, Status> {
        let _permit = self.state.control_permit()?;
        let session = self
            .state
            .session_for_principal(&request.session_id, principal.identity())?;
        principal.authorize();
        validation::identifier(&request.wait_id, "wait ID")?;
        session.cancel_wait(&request.wait_id).await?;
        Ok(Response::new(proto::CancelWaitResponse {}))
    }

    async fn terminate_request(
        &self,
        request: proto::TerminateRequest,
        principal: RequestPrincipal,
    ) -> Result<Response<proto::WaitResponse>, Status> {
        let session = self
            .state
            .session_for_principal(&request.session_id, principal.identity())?;
        principal.authorize();
        validation::identifier(&request.cell_id, "cell ID")?;
        let _permit = self.state.request_permit()?;
        let result = session.terminate(CellId::new(request.cell_id)).await?;
        let response = conversions::wait_response(result)?;
        if let Some(cell_id) = terminal_wait_cell_id(&response) {
            session.terminal_outcome_observed(cell_id);
        }
        Ok(Response::new(response))
    }
}

fn execution_stream(
    receiver: mpsc::Receiver<Result<proto::ExecuteEvent, Status>>,
    mut admission: ExecutionAdmission,
) -> GrpcStream<proto::ExecuteEvent> {
    Box::pin(ReceiverStream::new(receiver).inspect(move |event| {
        admission.observe(event);
    }))
}

fn terminal_wait_cell_id(response: &proto::WaitResponse) -> Option<&str> {
    let Some(proto::wait_response::State::LiveCell(outcome)) = &response.state else {
        return None;
    };
    matches!(
        outcome.outcome.as_ref(),
        Some(
            proto::execution_outcome::Outcome::Terminated(_)
                | proto::execution_outcome::Outcome::Completed(_)
        )
    )
    .then_some(outcome.cell_id.as_str())
}

/// A routable code-mode service with transport admission applied before protobuf decoding.
#[derive(Clone)]
pub struct LoopbackGrpcService {
    inner: GrpcAdmission<CodeModeHostServer<GrpcCodeModeHost>>,
}

impl Service<HttpRequest<Body>> for LoopbackGrpcService {
    type Response = HttpResponse<Body>;
    type Error = Infallible;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: HttpRequest<Body>) -> Self::Future {
        Box::pin(self.inner.call(request))
    }
}

impl NamedService for LoopbackGrpcService {
    const NAME: &'static str =
        <CodeModeHostServer<GrpcCodeModeHost> as NamedService>::NAME;
}

/// Builds a bounded local service for a transport with its own trusted caller boundary.
///
/// This compatibility constructor does not authenticate TCP peers. Production
/// loopback listeners must use the capability-bound service.
pub fn loopback_grpc_service() -> LoopbackGrpcService {
    loopback_service(PrincipalPolicy::TrustedLocalTransport)
}

pub(crate) fn authenticated_loopback_grpc_service(
    capability: Arc<str>,
) -> LoopbackGrpcService {
    loopback_service(PrincipalPolicy::AuthenticatedLocalTransport(capability))
}

fn loopback_service(principal_policy: PrincipalPolicy) -> LoopbackGrpcService {
    let service = CodeModeHostServer::new(GrpcCodeModeHost::loopback_tcp(principal_policy))
        .max_decoding_message_size(MAX_APPLICATION_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_APPLICATION_MESSAGE_BYTES);
    LoopbackGrpcService {
        inner: GrpcAdmissionLayer::new().layer(service),
    }
}

impl CodeModeHost for GrpcCodeModeHost {
    type OpenSessionStream = GrpcStream<proto::SessionEvent>;
    type SubscribeToToolCallsStream = GrpcStream<proto::ToolCall>;
    type ExecuteStream = GrpcStream<proto::ExecuteEvent>;

    fn open_session<'a, 'async_trait>(
        &'a self,
        request: Request<proto::OpenSessionRequest>,
    ) -> GrpcFuture<'async_trait, Self::OpenSessionStream>
    where
        'a: 'async_trait,
        Self: 'async_trait,
    {
        let principal = self.principal_policy.principal(&request);
        Box::pin(async move {
            self.open_session_request(request.into_inner(), principal?)
                .await
        })
    }

    fn close_session<'a, 'async_trait>(
        &'a self,
        request: Request<proto::CloseSessionRequest>,
    ) -> GrpcFuture<'async_trait, proto::CloseSessionResponse>
    where
        'a: 'async_trait,
        Self: 'async_trait,
    {
        let principal = self.principal_policy.principal(&request);
        Box::pin(async move {
            self.close_session_request(request.into_inner(), principal?)
                .await
        })
    }

    fn subscribe_to_tool_calls<'a, 'async_trait>(
        &'a self,
        request: Request<proto::SubscribeToToolCallsRequest>,
    ) -> GrpcFuture<'async_trait, Self::SubscribeToToolCallsStream>
    where
        'a: 'async_trait,
        Self: 'async_trait,
    {
        let principal = self.principal_policy.principal(&request);
        Box::pin(async move {
            self.subscribe_request(request.into_inner(), principal?)
                .await
        })
    }

    fn complete_tool_call<'a, 'async_trait>(
        &'a self,
        request: Request<proto::CompleteToolCallRequest>,
    ) -> GrpcFuture<'async_trait, proto::CompleteToolCallResponse>
    where
        'a: 'async_trait,
        Self: 'async_trait,
    {
        let principal = self.principal_policy.principal(&request);
        Box::pin(async move {
            self.complete_tool_request(request.into_inner(), principal?)
                .await
        })
    }

    fn acknowledge_notification<'a, 'async_trait>(
        &'a self,
        request: Request<proto::AcknowledgeNotificationRequest>,
    ) -> GrpcFuture<'async_trait, proto::AcknowledgeNotificationResponse>
    where
        'a: 'async_trait,
        Self: 'async_trait,
    {
        let principal = self.principal_policy.principal(&request);
        Box::pin(async move {
            self.acknowledge_notification_request(request.into_inner(), principal?)
                .await
        })
    }

    fn execute<'a, 'async_trait>(
        &'a self,
        request: Request<proto::ExecuteRequest>,
    ) -> GrpcFuture<'async_trait, Self::ExecuteStream>
    where
        'a: 'async_trait,
        Self: 'async_trait,
    {
        let principal = self.principal_policy.principal(&request);
        Box::pin(async move { self.execute_request(request.into_inner(), principal?).await })
    }

    fn wait<'a, 'async_trait>(
        &'a self,
        request: Request<proto::WaitRequest>,
    ) -> GrpcFuture<'async_trait, proto::WaitResponse>
    where
        'a: 'async_trait,
        Self: 'async_trait,
    {
        let principal = self.principal_policy.principal(&request);
        Box::pin(async move { self.wait_request(request.into_inner(), principal?).await })
    }

    fn cancel_wait<'a, 'async_trait>(
        &'a self,
        request: Request<proto::CancelWaitRequest>,
    ) -> GrpcFuture<'async_trait, proto::CancelWaitResponse>
    where
        'a: 'async_trait,
        Self: 'async_trait,
    {
        let principal = self.principal_policy.principal(&request);
        Box::pin(async move {
            self.cancel_wait_request(request.into_inner(), principal?)
                .await
        })
    }

    fn terminate<'a, 'async_trait>(
        &'a self,
        request: Request<proto::TerminateRequest>,
    ) -> GrpcFuture<'async_trait, proto::WaitResponse>
    where
        'a: 'async_trait,
        Self: 'async_trait,
    {
        let principal = self.principal_policy.principal(&request);
        Box::pin(async move {
            self.terminate_request(request.into_inner(), principal?)
                .await
        })
    }
}

struct ExecutionAdmission {
    session: Arc<GrpcSession>,
    execution_id: Option<String>,
}

impl ExecutionAdmission {
    fn observe(&mut self, event: &Result<proto::ExecuteEvent, Status>) {
        let Ok(proto::ExecuteEvent {
            event: Some(proto::execute_event::Event::Outcome(outcome)),
        }) = event
        else {
            return;
        };
        if matches!(
            outcome.outcome.as_ref(),
            Some(
                proto::execution_outcome::Outcome::Terminated(_)
                    | proto::execution_outcome::Outcome::Completed(_)
            )
        ) {
            self.session.terminal_outcome_observed(&outcome.cell_id);
        }
        self.disarm();
    }

    fn disarm(&mut self) {
        self.execution_id = None;
    }
}

impl Drop for ExecutionAdmission {
    fn drop(&mut self) {
        if let Some(execution_id) = self.execution_id.take() {
            self.session.abandon_execution(&execution_id);
        }
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "robustness_tests.rs"]
mod robustness_tests;

#[cfg(test)]
#[path = "transport_tests.rs"]
mod transport_tests;
