use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use codex_api::ApiError;
use codex_api::ResponseEvent;
use codex_api::ResponsesApiRequest;
use codex_api::ResponsesWebsocketClient;
use codex_api::ResponsesWebsocketConnection;
use codex_api::ResponsesWsRequest;
use codex_api::build_session_headers;
use codex_http_client::HttpClientFactory;
use codex_login::AgentIdentityAuthPolicy;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::default_client::default_headers;
use codex_model_provider::AgentIdentitySessionFallback;
use codex_model_provider::ProviderAuthScope;
use codex_model_provider::SharedModelProvider;
use codex_protocol::error::CodexErr;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SessionSource;
use http::HeaderValue;
use thiserror::Error;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::watch;
use tokio::time::Instant as TokioInstant;

const MODEL: &str = "gpt-5.6-luna";
const MAX_OUTPUT_BYTES: usize = 4 * 1024;
const INITIAL_WEBSOCKET_CONNECTIONS: usize = 2;
const MAX_WEBSOCKET_CONNECTIONS: usize = 8;
const MAX_WEBSOCKET_AGE: Duration = Duration::from_secs(55 * 60);
const MAX_AUTH_CHANGE_CONNECT_RETRIES: usize = 3;
const RESPONSES_WEBSOCKETS_BETA: &str = "responses_websockets=2026-02-06";

/// Host-owned provider, authentication, and attribution for Luna connections.
#[derive(Clone)]
pub struct LunaSamplerConfig {
    /// Provider and credentials selected for the owning thread.
    pub provider: SharedModelProvider,
    /// Effective proxy, custom-CA, and cookie configuration.
    pub http_client_factory: HttpClientFactory,
    /// Agent-identity policy selected for the owning thread.
    pub agent_identity_policy: AgentIdentityAuthPolicy,
    /// Host-resolved source used to scope agent-identity authentication.
    pub session_source: SessionSource,
    /// Owning runtime session identifier.
    pub session_id: String,
    /// Owning thread identifier.
    pub thread_id: String,
    /// Optional host-resolved request originator.
    pub originator: Option<String>,
    /// Optional inference service tier.
    pub service_tier: Option<String>,
}

pub(crate) struct LunaSamplingRequest {
    pub(crate) request: ResponsesApiRequest,
    pub(crate) deadline: TokioInstant,
}

/// Failures returned while connecting or sampling the Luna model.
#[derive(Debug, Error)]
pub enum LunaSamplerError {
    /// The thread's provider or scoped credentials could not be resolved.
    #[error("could not resolve the Luna model provider: {0}")]
    Provider(#[source] CodexErr),
    /// The Responses WebSocket could not be opened or streamed.
    #[error("Luna Responses WebSocket failed: {0}")]
    Api(#[source] ApiError),
    /// The configured review deadline elapsed.
    #[error("Luna review deadline elapsed")]
    Deadline,
    /// The provider's WebSocket connect deadline elapsed.
    #[error("Luna Responses WebSocket connection timed out")]
    ConnectionTimeout,
    /// Authentication changed while a connection or review was in flight.
    #[error("Luna authentication changed during the review")]
    AuthenticationChanged,
    /// The selected first-party provider has no current authentication.
    #[error("Luna authentication is unavailable")]
    MissingAuthentication,
    /// The response did not contain an assistant text value.
    #[error("Luna response did not contain assistant output")]
    MissingOutput,
    /// The response exceeded the bounded output limit.
    #[error("Luna response exceeded the output limit")]
    OutputTooLarge,
}

struct PooledConnection {
    connection: ResponsesWebsocketConnection,
    connected_at: Instant,
    auth_generation: Option<u64>,
}

struct ConnectionLease {
    connection: PooledConnection,
    idle_connections: Arc<Mutex<Vec<PooledConnection>>>,
    auth_generation: AuthGeneration,
    _permit: OwnedSemaphorePermit,
}

impl ConnectionLease {
    fn reuse(self) {
        if self.connection.auth_generation == self.auth_generation.current() {
            self.idle_connections
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(self.connection);
        }
    }
}

#[derive(Clone)]
struct AuthGeneration {
    manager: Option<Arc<AuthManager>>,
}

impl AuthGeneration {
    fn from_provider(provider: &SharedModelProvider) -> Self {
        Self {
            manager: provider.auth_manager(),
        }
    }

    fn current(&self) -> Option<u64> {
        self.manager.as_ref().map(|manager| {
            let changes = manager.auth_change_receiver();
            let revision = *changes.borrow();
            revision
        })
    }

    fn watch(&self) -> AuthGenerationWatch {
        AuthGenerationWatch {
            changes: self
                .manager
                .as_ref()
                .map(|manager| manager.auth_change_receiver()),
        }
    }
}

struct AuthGenerationWatch {
    changes: Option<watch::Receiver<u64>>,
}

impl AuthGenerationWatch {
    fn current(&self) -> Option<u64> {
        self.changes
            .as_ref()
            .map(|changes| *changes.borrow())
    }

    async fn changed(&mut self) {
        match self.changes.as_mut() {
            Some(changes) => {
                let _ = changes.changed().await;
            }
            None => std::future::pending().await,
        }
    }
}

/// A bounded pool of authenticated Responses WebSockets for Luna reviews.
pub struct LunaSampler {
    config: LunaSamplerConfig,
    idle_connections: Arc<Mutex<Vec<PooledConnection>>>,
    capacity: Arc<Semaphore>,
    auth_generation: AuthGeneration,
}

impl LunaSampler {
    /// Opens the initial WebSockets before any review is requested.
    pub async fn connect(config: LunaSamplerConfig) -> Result<Self, LunaSamplerError> {
        let auth_generation = AuthGeneration::from_provider(&config.provider);
        let sampler = Self {
            config,
            idle_connections: Arc::new(Mutex::new(Vec::with_capacity(MAX_WEBSOCKET_CONNECTIONS))),
            capacity: Arc::new(Semaphore::new(MAX_WEBSOCKET_CONNECTIONS)),
            auth_generation,
        };
        for index in 0..INITIAL_WEBSOCKET_CONNECTIONS {
            let connection = match sampler.open_connection().await {
                Ok(connection) => connection,
                Err(error) if index == 0 => return Err(error),
                Err(_) => break,
            };
            sampler
                .idle_connections
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(connection);
        }
        Ok(sampler)
    }

    async fn open_connection(&self) -> Result<PooledConnection, LunaSamplerError> {
        for _ in 0..MAX_AUTH_CHANGE_CONNECT_RETRIES {
            let auth_generation = self.auth_generation.current();
            let connection = self.open_connection_for_generation(auth_generation).await?;
            if auth_generation == self.auth_generation.current() {
                return Ok(connection);
            }
        }
        Err(LunaSamplerError::AuthenticationChanged)
    }

    async fn open_connection_for_generation(
        &self,
        auth_generation: Option<u64>,
    ) -> Result<PooledConnection, LunaSamplerError> {
        let provider_info = self.config.provider.info();
        let requires_host_auth = provider_info.requires_openai_auth
            && provider_info.env_key.is_none()
            && provider_info.experimental_bearer_token.is_none()
            && provider_info.auth.is_none()
            && provider_info.aws.is_none();
        let provider_auth = self.config.provider.auth().await;
        if requires_host_auth && provider_auth.is_none() {
            return Err(LunaSamplerError::MissingAuthentication);
        }
        let provider = self
            .config
            .provider
            .api_provider()
            .await
            .map_err(LunaSamplerError::Provider)?;
        let auth = self
            .config
            .provider
            .api_auth_for_scope(ProviderAuthScope {
                agent_identity_policy: self.config.agent_identity_policy,
                session_source: self.config.session_source.clone(),
                agent_identity_session_fallback: AgentIdentitySessionFallback::default(),
            })
            .await
            .map_err(LunaSamplerError::Provider)?
            .auth;
        let mut headers = build_session_headers(
            Some(self.config.session_id.clone()),
            Some(self.config.thread_id.clone()),
        );
        headers.insert(
            "openai-beta",
            HeaderValue::from_static(RESPONSES_WEBSOCKETS_BETA),
        );
        if let Some(originator) = self.config.originator.as_deref()
            && let Ok(originator) = HeaderValue::from_str(originator)
        {
            headers.insert("originator", originator);
        }
        if let Ok(request_id) = HeaderValue::from_str(&self.config.thread_id) {
            headers.insert("x-client-request-id", request_id);
        }

        if provider_auth
            .as_ref()
            .is_some_and(CodexAuth::uses_codex_backend)
            && provider_info.is_openai()
            && requires_host_auth
        {
            let routing_hint = match self.config.service_tier.as_deref() {
                Some(tier) => format!("model={MODEL};tier={tier}"),
                None => format!("model={MODEL}"),
            };
            if let Ok(value) = HeaderValue::from_str(&routing_hint) {
                headers.insert("x-codex-routing-hint", value);
            }
        }

        let client = ResponsesWebsocketClient::new(provider, auth);
        let connect = client.connect(
            &self.config.http_client_factory,
            headers,
            default_headers(),
            /*turn_state*/ None,
            /*telemetry*/ None,
        );
        let connection = tokio::time::timeout(provider_info.websocket_connect_timeout(), connect)
            .await
            .map_err(|_| LunaSamplerError::ConnectionTimeout)?
            .map_err(LunaSamplerError::Api)?;

        Ok(PooledConnection {
            connection,
            connected_at: Instant::now(),
            auth_generation,
        })
    }

    async fn lease_connection(&self) -> Result<ConnectionLease, LunaSamplerError> {
        let permit = Arc::clone(&self.capacity)
            .acquire_owned()
            .await
            .map_err(|_| LunaSamplerError::ConnectionTimeout)?;
        let connection = loop {
            let idle = self
                .idle_connections
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop();
            match idle {
                Some(connection)
                    if connection.connected_at.elapsed() < MAX_WEBSOCKET_AGE
                        && connection.auth_generation == self.auth_generation.current()
                        && !connection.connection.is_closed().await =>
                {
                    break connection;
                }
                Some(_) => {}
                None => break self.open_connection().await?,
            }
        };
        Ok(ConnectionLease {
            connection,
            idle_connections: Arc::clone(&self.idle_connections),
            auth_generation: self.auth_generation.clone(),
            _permit: permit,
        })
    }

    pub(crate) async fn sample(
        &self,
        request: LunaSamplingRequest,
    ) -> Result<String, LunaSamplerError> {
        let deadline = request.deadline;
        tokio::time::timeout_at(deadline, self.sample_before_deadline(request.request))
            .await
            .map_err(|_| LunaSamplerError::Deadline)?
    }

    pub(crate) fn client_metadata(&self, turn_id: &str) -> HashMap<String, String> {
        request_metadata(
            &self.config.session_id,
            &self.config.thread_id,
            turn_id,
        )
    }

    pub(crate) fn service_tier(&self) -> Option<String> {
        self.config.service_tier.clone()
    }

    pub(crate) fn thread_id(&self) -> &str {
        &self.config.thread_id
    }

    async fn sample_before_deadline(
        &self,
        request: ResponsesApiRequest,
    ) -> Result<String, LunaSamplerError> {
        let mut retried = false;
        'retry: loop {
            let lease = self.lease_connection().await?;
            let mut auth_generation = self.auth_generation.watch();
            if lease.connection.auth_generation != auth_generation.current() {
                return Err(LunaSamplerError::AuthenticationChanged);
            }
            let stream = lease
                .connection
                .connection
                .stream_request(
                    ResponsesWsRequest::ResponseCreate((&request).into()),
                    /*connection_reused*/ true,
                    /*turn_state*/ None,
                )
                .await;
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(error) if !retried && is_retryable(&error) => {
                    retried = true;
                    continue 'retry;
                }
                Err(error) => return Err(LunaSamplerError::Api(error)),
            };

            let mut output = String::new();
            let mut deltas = String::new();
            loop {
                let event = tokio::select! {
                    event = stream.rx_event.recv() => event,
                    _ = auth_generation.changed() => {
                        return Err(LunaSamplerError::AuthenticationChanged);
                    }
                };
                let Some(event) = event else {
                    return Err(LunaSamplerError::MissingOutput);
                };
                let event = match event {
                    Ok(event) => event,
                    Err(error) if !retried && is_retryable(&error) => {
                        retried = true;
                        continue 'retry;
                    }
                    Err(error) => return Err(LunaSamplerError::Api(error)),
                };
                match event {
                    ResponseEvent::OutputTextDelta(delta) => {
                        append_bounded_output(&mut deltas, &delta)?;
                    }
                    ResponseEvent::OutputItemDone(ResponseItem::Message {
                        role, content, ..
                    }) if role == "assistant" => {
                        for item in content {
                            if let ContentItem::OutputText { text } = item {
                                append_bounded_output(&mut output, &text)?;
                            }
                        }
                    }
                    ResponseEvent::Completed { .. } => {
                        if lease.connection.auth_generation != auth_generation.current() {
                            return Err(LunaSamplerError::AuthenticationChanged);
                        }
                        lease.reuse();
                        if !output.is_empty() {
                            return Ok(output);
                        }
                        if !deltas.is_empty() {
                            return Ok(deltas);
                        }
                        return Err(LunaSamplerError::MissingOutput);
                    }
                    ResponseEvent::Created
                    | ResponseEvent::SafetyBuffering(_)
                    | ResponseEvent::OutputItemDone(_)
                    | ResponseEvent::OutputItemAdded(_)
                    | ResponseEvent::ServerModel(_)
                    | ResponseEvent::ModelVerifications(_)
                    | ResponseEvent::TurnModerationMetadata(_)
                    | ResponseEvent::ServerReasoningIncluded(_)
                    | ResponseEvent::ToolCallInputDelta { .. }
                    | ResponseEvent::ReasoningSummaryDelta { .. }
                    | ResponseEvent::ReasoningSummaryDone { .. }
                    | ResponseEvent::ReasoningContentDelta { .. }
                    | ResponseEvent::ReasoningSummaryPartAdded { .. }
                    | ResponseEvent::RateLimits(_)
                    | ResponseEvent::ModelsEtag(_) => {}
                }
            }
        }
    }
}

fn append_bounded_output(output: &mut String, chunk: &str) -> Result<(), LunaSamplerError> {
    if chunk.len() > MAX_OUTPUT_BYTES.saturating_sub(output.len()) {
        return Err(LunaSamplerError::OutputTooLarge);
    }
    output.push_str(chunk);
    Ok(())
}

fn is_retryable(error: &ApiError) -> bool {
    matches!(error, ApiError::Retryable { .. } | ApiError::Stream(_))
}

pub(crate) fn request_metadata(
    session_id: &str,
    thread_id: &str,
    turn_id: &str,
) -> HashMap<String, String> {
    HashMap::from([
        ("session_id".to_owned(), session_id.to_owned()),
        ("thread_id".to_owned(), thread_id.to_owned()),
        ("turn_id".to_owned(), turn_id.to_owned()),
        (
            "ws_request_header_x_openai_internal_codex_responses_lite".to_owned(),
            "true".to_owned(),
        ),
    ])
}

pub(crate) fn model() -> &'static str {
    MODEL
}

#[cfg(test)]
#[path = "sampler_tests.rs"]
mod tests;
