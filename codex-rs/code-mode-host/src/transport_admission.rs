use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use codex_code_mode_protocol::grpc::CAPABILITY_METADATA_KEY;
use codex_code_mode_protocol::grpc::MAX_APPLICATION_MESSAGE_BYTES;
use http_body_util::BodyExt;
use http_body_util::Full;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::TryAcquireError;
use tonic::body::Body;
use tonic::codegen::Body as HttpBody;
use tonic::codegen::Bytes;
use tonic::codegen::http::HeaderValue;
use tonic::codegen::http::Request;
use tonic::codegen::http::Response;
use tonic::codegen::http::header::CONTENT_TYPE;
use tower::Layer;
use tower::Service;
use tower::ServiceExt;

use crate::grpc::principal::constant_time_matches;
use crate::grpc::validation;

const EXECUTE_BODY_BYTES: usize = MAX_APPLICATION_MESSAGE_BYTES;
const COMPLETION_BODY_BYTES: usize = MAX_APPLICATION_MESSAGE_BYTES;
const NORMAL_BODY_BYTES: usize = 256 * 1_024;
const CRITICAL_BODY_BYTES: usize = 256 * 1_024;
const EXECUTE_READERS: usize = 1;
const COMPLETION_READERS: usize = 1;
const NORMAL_READERS: usize = 2;
const CRITICAL_READERS: usize = 2;
const EXECUTE_DECODE_BYTES: usize = 16 * 1_024 * 1_024;
const COMPLETION_DECODE_BYTES: usize = 16 * 1_024 * 1_024;
const NORMAL_DECODE_BYTES: usize = 4 * 1_024 * 1_024;
const CRITICAL_DECODE_BYTES: usize = 4 * 1_024 * 1_024;
const BUDGET_UNIT_BYTES: usize = 64 * 1_024;
const BODY_ADMISSION_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 10);
const GRPC_PREFIX_BYTES: usize = 5;
pub(crate) const MAX_OPEN_RESPONSES: usize = 6;
pub(crate) const MAX_SUBSCRIBE_RESPONSES: usize = 12;
pub(crate) const MAX_EXECUTE_RESPONSES: usize = 6;
pub(crate) const MAX_STREAMING_RESPONSES: usize =
    MAX_OPEN_RESPONSES + MAX_SUBSCRIBE_RESPONSES + MAX_EXECUTE_RESPONSES;
const NORMAL_RESPONSE_PERMITS: usize = 4;
const CRITICAL_RESPONSE_PERMITS: usize = 4;
pub(crate) const MAX_UNARY_RESPONSES: usize = NORMAL_RESPONSE_PERMITS + CRITICAL_RESPONSE_PERMITS;

const OPEN_PATH: &str = "/codex.code_mode.v1.CodeModeHost/OpenSession";
const CLOSE_PATH: &str = "/codex.code_mode.v1.CodeModeHost/CloseSession";
const SUBSCRIBE_PATH: &str = "/codex.code_mode.v1.CodeModeHost/SubscribeToToolCalls";
const COMPLETE_TOOL_PATH: &str = "/codex.code_mode.v1.CodeModeHost/CompleteToolCall";
const ACKNOWLEDGE_PATH: &str = "/codex.code_mode.v1.CodeModeHost/AcknowledgeNotification";
const EXECUTE_PATH: &str = "/codex.code_mode.v1.CodeModeHost/Execute";
const WAIT_PATH: &str = "/codex.code_mode.v1.CodeModeHost/Wait";
const CANCEL_WAIT_PATH: &str = "/codex.code_mode.v1.CodeModeHost/CancelWait";
const TERMINATE_PATH: &str = "/codex.code_mode.v1.CodeModeHost/Terminate";

pub(crate) const MAX_RAW_REQUEST_BYTES: usize = EXECUTE_BODY_BYTES * EXECUTE_READERS
    + COMPLETION_BODY_BYTES * COMPLETION_READERS
    + NORMAL_BODY_BYTES * NORMAL_READERS
    + CRITICAL_BODY_BYTES * CRITICAL_READERS;
pub(crate) const MAX_RAW_REQUEST_ALLOCATION_BYTES: usize = MAX_RAW_REQUEST_BYTES * 2;
pub(crate) const MAX_DECODED_REQUEST_BYTES: usize =
    EXECUTE_DECODE_BYTES + COMPLETION_DECODE_BYTES + NORMAL_DECODE_BYTES + CRITICAL_DECODE_BYTES;
pub(crate) const MAX_OUTBOUND_RESPONSE_BYTES: usize =
    (MAX_STREAMING_RESPONSES + MAX_UNARY_RESPONSES) * MAX_APPLICATION_MESSAGE_BYTES;

#[derive(Clone)]
pub(crate) struct GrpcAdmissionLayer {
    execute: AdmissionPool,
    completion: AdmissionPool,
    normal: AdmissionPool,
    critical: AdmissionPool,
    open_responses: Arc<Semaphore>,
    subscribe_responses: Arc<Semaphore>,
    execute_responses: Arc<Semaphore>,
    normal_responses: Arc<Semaphore>,
    critical_responses: Arc<Semaphore>,
    capability: Option<Arc<str>>,
}

#[derive(Clone)]
struct AdmissionPool {
    readers: Arc<Semaphore>,
    decoded: Arc<Semaphore>,
    maximum_body_bytes: usize,
    maximum_decoded_bytes: usize,
    multiplier: usize,
}

impl AdmissionPool {
    fn new(
        readers: usize,
        maximum_body_bytes: usize,
        maximum_decoded_bytes: usize,
        multiplier: usize,
    ) -> Self {
        Self {
            readers: Arc::new(Semaphore::new(readers)),
            decoded: Arc::new(Semaphore::new(maximum_decoded_bytes / BUDGET_UNIT_BYTES)),
            maximum_body_bytes,
            maximum_decoded_bytes,
            multiplier,
        }
    }
}

impl GrpcAdmissionLayer {
    pub(crate) fn new() -> Self {
        Self {
            execute: AdmissionPool::new(
                EXECUTE_READERS,
                EXECUTE_BODY_BYTES,
                EXECUTE_DECODE_BYTES,
                /*multiplier*/ 16,
            ),
            completion: AdmissionPool::new(
                COMPLETION_READERS,
                COMPLETION_BODY_BYTES,
                COMPLETION_DECODE_BYTES,
                /*multiplier*/ 16,
            ),
            normal: AdmissionPool::new(
                NORMAL_READERS,
                NORMAL_BODY_BYTES,
                NORMAL_DECODE_BYTES,
                /*multiplier*/ 8,
            ),
            critical: AdmissionPool::new(
                CRITICAL_READERS,
                CRITICAL_BODY_BYTES,
                CRITICAL_DECODE_BYTES,
                /*multiplier*/ 8,
            ),
            open_responses: Arc::new(Semaphore::new(MAX_OPEN_RESPONSES)),
            subscribe_responses: Arc::new(Semaphore::new(MAX_SUBSCRIBE_RESPONSES)),
            execute_responses: Arc::new(Semaphore::new(MAX_EXECUTE_RESPONSES)),
            normal_responses: Arc::new(Semaphore::new(NORMAL_RESPONSE_PERMITS)),
            critical_responses: Arc::new(Semaphore::new(CRITICAL_RESPONSE_PERMITS)),
            capability: None,
        }
    }

    pub(crate) fn authenticated(capability: Arc<str>) -> Self {
        Self {
            capability: Some(capability),
            ..Self::new()
        }
    }

    fn authorize(&self, request: &Request<Body>) -> Result<(), AdmissionError> {
        let Some(expected) = self.capability.as_deref() else {
            return Ok(());
        };
        let actual = request
            .headers()
            .get(CAPABILITY_METADATA_KEY)
            .map(HeaderValue::as_bytes)
            .unwrap_or_default();
        if constant_time_matches(expected.as_bytes(), actual) {
            Ok(())
        } else {
            Err(AdmissionError::unauthenticated())
        }
    }

    fn pool(&self, path: &str) -> &AdmissionPool {
        if path == EXECUTE_PATH {
            &self.execute
        } else if path == COMPLETE_TOOL_PATH {
            &self.completion
        } else if is_critical(path) {
            &self.critical
        } else {
            &self.normal
        }
    }

    async fn admit(&self, path: &str, body: Body) -> Result<AdmittedBody, AdmissionError> {
        let pool = self.pool(path);
        let reader =
            Arc::clone(&pool.readers)
                .try_acquire_owned()
                .map_err(|error| match error {
                    TryAcquireError::NoPermits => AdmissionError::exhausted(),
                    TryAcquireError::Closed => AdmissionError::internal(),
                })?;
        let body = tokio::time::timeout(
            BODY_ADMISSION_TIMEOUT,
            read_body(body, pool.maximum_body_bytes),
        )
        .await
        .map_err(|_| AdmissionError::deadline())??;
        let message = grpc_message(&body, pool.maximum_body_bytes)?;
        preflight_message(path, message)?;
        let weighted_bytes = message
            .len()
            .checked_mul(pool.multiplier)
            .and_then(|bytes| bytes.checked_add(BUDGET_UNIT_BYTES - 1))
            .ok_or_else(AdmissionError::exhausted)?;
        if weighted_bytes > pool.maximum_decoded_bytes {
            return Err(AdmissionError::exhausted());
        }
        let units = u32::try_from((weighted_bytes / BUDGET_UNIT_BYTES).max(1))
            .map_err(|_| AdmissionError::exhausted())?;
        let decoded = tokio::time::timeout(
            BODY_ADMISSION_TIMEOUT,
            Arc::clone(&pool.decoded).acquire_many_owned(units),
        )
        .await
        .map_err(|_| AdmissionError::deadline())?
        .map_err(|_| AdmissionError::internal())?;
        drop(reader);
        Ok(AdmittedBody { body, decoded })
    }

    fn response_permit(&self, path: &str) -> Result<OwnedSemaphorePermit, AdmissionError> {
        let permits = if path == OPEN_PATH {
            &self.open_responses
        } else if path == SUBSCRIBE_PATH {
            &self.subscribe_responses
        } else if path == EXECUTE_PATH {
            &self.execute_responses
        } else if is_critical(path) {
            &self.critical_responses
        } else {
            &self.normal_responses
        };
        Arc::clone(permits)
            .try_acquire_owned()
            .map_err(|error| match error {
                TryAcquireError::NoPermits => AdmissionError::exhausted(),
                TryAcquireError::Closed => AdmissionError::internal(),
            })
    }
}

impl<S> Layer<S> for GrpcAdmissionLayer {
    type Service = GrpcAdmission<S>;

    fn layer(&self, inner: S) -> Self::Service {
        GrpcAdmission {
            inner,
            layer: self.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct GrpcAdmission<S> {
    inner: S,
    layer: GrpcAdmissionLayer,
}

impl<S> Service<Request<Body>> for GrpcAdmission<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let inner = self.inner.clone();
        let layer = self.layer.clone();
        Box::pin(async move {
            if let Err(error) = layer.authorize(&request) {
                return Ok(error.response());
            }
            let (parts, body) = request.into_parts();
            let path = parts.uri.path().to_string();
            if !is_known_path(&path) {
                return Ok(AdmissionError::unimplemented().response());
            }
            let AdmittedBody { body, decoded } = match layer.admit(&path, body).await {
                Ok(admitted) => admitted,
                Err(error) => return Ok(error.response()),
            };
            let response_permit = match layer.response_permit(&path) {
                Ok(permit) => permit,
                Err(error) => return Ok(error.response()),
            };
            let request = Request::from_parts(parts, Body::new(Full::new(body)));
            let response = inner.oneshot(request).await?;
            drop(decoded);
            Ok(response.map(|body| {
                Body::new(PermittedBody {
                    inner: body,
                    _permit: response_permit,
                })
            }))
        })
    }
}

struct AdmittedBody {
    body: Bytes,
    decoded: OwnedSemaphorePermit,
}

async fn read_body(mut body: Body, maximum_message_bytes: usize) -> Result<Bytes, AdmissionError> {
    let maximum = maximum_message_bytes + GRPC_PREFIX_BYTES;
    let mut output = Vec::with_capacity(maximum.min(8 * 1_024));
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| AdmissionError::invalid())?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        if data.len() > maximum.saturating_sub(output.len()) {
            return Err(AdmissionError::exhausted());
        }
        output.extend_from_slice(&data);
    }
    Ok(Bytes::from(output))
}

fn grpc_message(body: &[u8], maximum_message_bytes: usize) -> Result<&[u8], AdmissionError> {
    if body.len() < GRPC_PREFIX_BYTES || body[0] != 0 {
        return Err(AdmissionError::invalid());
    }
    let declared = u32::from_be_bytes(
        body[1..5]
            .try_into()
            .map_err(|_| AdmissionError::invalid())?,
    );
    let declared = usize::try_from(declared).map_err(|_| AdmissionError::exhausted())?;
    if declared > maximum_message_bytes || body.len() != declared + GRPC_PREFIX_BYTES {
        return Err(AdmissionError::exhausted());
    }
    Ok(&body[GRPC_PREFIX_BYTES..])
}

fn is_critical(path: &str) -> bool {
    matches!(
        path,
        CLOSE_PATH | COMPLETE_TOOL_PATH | ACKNOWLEDGE_PATH | CANCEL_WAIT_PATH | TERMINATE_PATH
    )
}

fn is_known_path(path: &str) -> bool {
    matches!(
        path,
        OPEN_PATH
            | CLOSE_PATH
            | SUBSCRIBE_PATH
            | COMPLETE_TOOL_PATH
            | ACKNOWLEDGE_PATH
            | EXECUTE_PATH
            | WAIT_PATH
            | CANCEL_WAIT_PATH
            | TERMINATE_PATH
    )
}

pub(crate) fn preflight_message(path: &str, message: &[u8]) -> Result<(), AdmissionError> {
    if path == EXECUTE_PATH {
        count_field(
            message,
            /*field_number*/ 5,
            validation::MAX_TOOL_DEFINITIONS,
        )?;
    } else if path == SUBSCRIBE_PATH {
        count_field(
            message,
            /*field_number*/ 2,
            validation::MAX_TOOL_FILTERS,
        )?;
    }
    Ok(())
}

fn count_field(message: &[u8], target_field: u64, maximum: usize) -> Result<(), AdmissionError> {
    let mut cursor = 0usize;
    let mut count = 0usize;
    while cursor < message.len() {
        let key = read_varint(message, &mut cursor)?;
        let field = key >> 3;
        let wire_type = key & 7;
        if field == 0 {
            return Err(AdmissionError::invalid());
        }
        match wire_type {
            0 => {
                read_varint(message, &mut cursor)?;
            }
            1 => skip(message, &mut cursor, /*bytes*/ 8)?,
            2 => {
                let length = read_varint(message, &mut cursor)?;
                let length = usize::try_from(length).map_err(|_| AdmissionError::invalid())?;
                if field == target_field {
                    count += 1;
                    if count > maximum {
                        return Err(AdmissionError::exhausted());
                    }
                }
                skip(message, &mut cursor, length)?;
            }
            5 => skip(message, &mut cursor, /*bytes*/ 4)?,
            3 | 4 | 6 | 7 => return Err(AdmissionError::invalid()),
            8..=u64::MAX => return Err(AdmissionError::invalid()),
        }
    }
    Ok(())
}

fn read_varint(message: &[u8], cursor: &mut usize) -> Result<u64, AdmissionError> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *message.get(*cursor).ok_or_else(AdmissionError::invalid)?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(AdmissionError::invalid());
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(AdmissionError::invalid())
}

fn skip(message: &[u8], cursor: &mut usize, bytes: usize) -> Result<(), AdmissionError> {
    *cursor = (*cursor)
        .checked_add(bytes)
        .filter(|end| *end <= message.len())
        .ok_or_else(AdmissionError::invalid)?;
    Ok(())
}

struct PermittedBody {
    inner: Body,
    _permit: OwnedSemaphorePermit,
}

impl HttpBody for PermittedBody {
    type Data = Bytes;
    type Error = tonic::Status;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

#[derive(Debug)]
pub(crate) struct AdmissionError {
    status: &'static str,
    message: &'static str,
}

impl AdmissionError {
    fn exhausted() -> Self {
        Self {
            status: "8",
            message: "code-mode-request-exhausted",
        }
    }

    fn invalid() -> Self {
        Self {
            status: "3",
            message: "invalid-code-mode-request",
        }
    }

    fn deadline() -> Self {
        Self {
            status: "4",
            message: "code-mode-request-deadline-exceeded",
        }
    }

    fn internal() -> Self {
        Self {
            status: "13",
            message: "code-mode-admission-unavailable",
        }
    }

    fn unimplemented() -> Self {
        Self {
            status: "12",
            message: "unknown-code-mode-request",
        }
    }

    fn unauthenticated() -> Self {
        Self {
            status: "16",
            message: "code-mode-capability-missing-or-invalid",
        }
    }

    fn response(self) -> Response<Body> {
        let mut response = Response::new(Body::empty());
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/grpc"));
        response
            .headers_mut()
            .insert("grpc-status", HeaderValue::from_static(self.status));
        response
            .headers_mut()
            .insert("grpc-message", HeaderValue::from_static(self.message));
        response
    }
}

#[cfg(test)]
#[path = "transport_admission_tests.rs"]
mod tests;
