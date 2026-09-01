use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use codex_code_mode_protocol::grpc::MAX_APPLICATION_MESSAGE_BYTES;
use codex_code_mode_protocol::grpc::MAX_CONTENT_ITEMS;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tonic::body::Body;
use tonic::codegen::Body as HttpBody;
use tonic::codegen::Bytes;
use tonic::codegen::http::Response;
use tonic::Status;

const MAX_OPEN_RESPONSE_BODIES: usize = 6;
const MAX_SUBSCRIBE_RESPONSE_BODIES: usize = 12;
const MAX_EXECUTE_RESPONSE_BODIES: usize = 6;
const MAX_NORMAL_RESPONSE_BODIES: usize = 4;
const MAX_CRITICAL_RESPONSE_BODIES: usize = 4;
const MAX_NORMAL_REQUESTS: usize = 24;
const MAX_CRITICAL_REQUESTS: usize = 8;
const MAX_BUFFERED_BODY_BYTES: usize = MAX_APPLICATION_MESSAGE_BYTES * 2;
const MAX_DECODE_ALLOCATION_BYTES: usize = 32 * 1_024 * 1_024;
const DECODE_ALLOCATION_MULTIPLIER: usize = 16;
const GRPC_PREFIX_BYTES: usize = 5;

const EXECUTE_PATH: &str = "/codex.code_mode.v1.CodeModeHost/Execute";
const OPEN_PATH: &str = "/codex.code_mode.v1.CodeModeHost/OpenSession";
const SUBSCRIBE_PATH: &str = "/codex.code_mode.v1.CodeModeHost/SubscribeToToolCalls";
const WAIT_PATH: &str = "/codex.code_mode.v1.CodeModeHost/Wait";
const TERMINATE_PATH: &str = "/codex.code_mode.v1.CodeModeHost/Terminate";
const CLOSE_PATH: &str = "/codex.code_mode.v1.CodeModeHost/CloseSession";
const COMPLETE_PATH: &str = "/codex.code_mode.v1.CodeModeHost/CompleteToolCall";
const ACKNOWLEDGE_PATH: &str = "/codex.code_mode.v1.CodeModeHost/AcknowledgeNotification";
const CANCEL_WAIT_PATH: &str = "/codex.code_mode.v1.CodeModeHost/CancelWait";

#[derive(Clone)]
pub(super) struct ResponseAdmission {
    open_bodies: Arc<Semaphore>,
    subscribe_bodies: Arc<Semaphore>,
    execute_bodies: Arc<Semaphore>,
    normal_bodies: Arc<Semaphore>,
    critical_bodies: Arc<Semaphore>,
    normal_requests: Arc<Semaphore>,
    critical_requests: Arc<Semaphore>,
    decoded: Arc<Semaphore>,
}

impl ResponseAdmission {
    pub(super) fn new() -> Self {
        Self {
            open_bodies: Arc::new(Semaphore::new(MAX_OPEN_RESPONSE_BODIES)),
            subscribe_bodies: Arc::new(Semaphore::new(MAX_SUBSCRIBE_RESPONSE_BODIES)),
            execute_bodies: Arc::new(Semaphore::new(MAX_EXECUTE_RESPONSE_BODIES)),
            normal_bodies: Arc::new(Semaphore::new(MAX_NORMAL_RESPONSE_BODIES)),
            critical_bodies: Arc::new(Semaphore::new(MAX_CRITICAL_RESPONSE_BODIES)),
            normal_requests: Arc::new(Semaphore::new(MAX_NORMAL_REQUESTS)),
            critical_requests: Arc::new(Semaphore::new(MAX_CRITICAL_REQUESTS)),
            decoded: Arc::new(Semaphore::new(MAX_DECODE_ALLOCATION_BYTES)),
        }
    }

    pub(super) fn request_permit(&self, path: &str) -> Result<OwnedSemaphorePermit, io::Error> {
        let permits = if is_critical(path) {
            &self.critical_requests
        } else {
            &self.normal_requests
        };
        Arc::clone(permits)
            .try_acquire_owned()
            .map_err(|_| io::Error::other("gRPC code-mode request budget is exhausted"))
    }

    pub(super) fn wrap(
        &self,
        path: &str,
        response: Response<Body>,
    ) -> Result<Response<Body>, io::Error> {
        let permits = if path == OPEN_PATH {
            &self.open_bodies
        } else if path == SUBSCRIBE_PATH {
            &self.subscribe_bodies
        } else if path == EXECUTE_PATH {
            &self.execute_bodies
        } else if is_critical(path) {
            &self.critical_bodies
        } else {
            &self.normal_bodies
        };
        let body_permit = Arc::clone(permits)
            .try_acquire_owned()
            .map_err(|_| io::Error::other("gRPC code-mode response body budget is exhausted"))?;
        let shape = ResponseShape::for_path(path);
        Ok(response.map(|inner| {
            Body::new(PreflightBody {
                inner,
                shape,
                buffer: Vec::with_capacity(8 * 1_024),
                decoded: Arc::clone(&self.decoded),
                decoded_permit: None,
                _body_permit: body_permit,
                failed: false,
            })
        }))
    }
}

fn is_critical(path: &str) -> bool {
    matches!(
        path,
        CLOSE_PATH | COMPLETE_PATH | ACKNOWLEDGE_PATH | CANCEL_WAIT_PATH | TERMINATE_PATH
    )
}

#[derive(Clone, Copy)]
enum ResponseShape {
    Execute,
    Wait,
    Other,
}

impl ResponseShape {
    fn for_path(path: &str) -> Self {
        if path == EXECUTE_PATH {
            Self::Execute
        } else if matches!(path, WAIT_PATH | TERMINATE_PATH) {
            Self::Wait
        } else {
            Self::Other
        }
    }
}

struct PreflightBody {
    inner: Body,
    shape: ResponseShape,
    buffer: Vec<u8>,
    decoded: Arc<Semaphore>,
    decoded_permit: Option<OwnedSemaphorePermit>,
    _body_permit: OwnedSemaphorePermit,
    failed: bool,
}

impl PreflightBody {
    fn take_message(&mut self) -> Result<Option<Bytes>, Status> {
        if self.buffer.len() < GRPC_PREFIX_BYTES {
            return Ok(None);
        }
        if self.buffer[0] != 0 {
            return Err(Status::invalid_argument(
                "compressed code-mode responses are not supported",
            ));
        }
        let declared = u32::from_be_bytes(
            self.buffer[1..5]
                .try_into()
                .map_err(|_| Status::invalid_argument("invalid code-mode response prefix"))?,
        );
        let declared = usize::try_from(declared)
            .map_err(|_| Status::resource_exhausted("code-mode response length overflowed"))?;
        if declared > MAX_APPLICATION_MESSAGE_BYTES {
            return Err(Status::resource_exhausted(format!(
                "code-mode response exceeds the {MAX_APPLICATION_MESSAGE_BYTES}-byte application limit"
            )));
        }
        let frame_bytes = declared
            .checked_add(GRPC_PREFIX_BYTES)
            .ok_or_else(|| Status::resource_exhausted("code-mode response length overflowed"))?;
        if self.buffer.len() < frame_bytes {
            return Ok(None);
        }
        preflight_response(self.shape, &self.buffer[GRPC_PREFIX_BYTES..frame_bytes])?;
        let allocation = frame_bytes
            .checked_mul(DECODE_ALLOCATION_MULTIPLIER)
            .filter(|bytes| *bytes <= MAX_DECODE_ALLOCATION_BYTES)
            .ok_or_else(|| {
                Status::resource_exhausted("code-mode response decode budget is exhausted")
            })?;
        let allocation = u32::try_from(allocation.max(1)).map_err(|_| {
            Status::resource_exhausted("code-mode response allocation exceeds this platform")
        })?;
        self.decoded_permit = Some(
            Arc::clone(&self.decoded)
                .try_acquire_many_owned(allocation)
                .map_err(|_| {
                    Status::resource_exhausted("code-mode response decode budget is exhausted")
                })?,
        );
        Ok(Some(Bytes::from(
            self.buffer.drain(..frame_bytes).collect::<Vec<_>>(),
        )))
    }
}

impl HttpBody for PreflightBody {
    type Data = Bytes;
    type Error = Status;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        self.decoded_permit = None;
        loop {
            if self.failed {
                return Poll::Ready(None);
            }
            match self.take_message() {
                Ok(Some(message)) => {
                    return Poll::Ready(Some(Ok(http_body::Frame::data(message))));
                }
                Ok(None) => {}
                Err(error) => {
                    self.failed = true;
                    return Poll::Ready(Some(Err(error)));
                }
            }
            match Pin::new(&mut self.inner).poll_frame(cx) {
                Poll::Ready(Some(Ok(frame))) => match frame.into_data() {
                    Ok(data) => {
                        if data.len() > MAX_BUFFERED_BODY_BYTES.saturating_sub(self.buffer.len()) {
                            self.failed = true;
                            return Poll::Ready(Some(Err(Status::resource_exhausted(
                                "code-mode response buffering budget is exhausted",
                            ))));
                        }
                        self.buffer.extend_from_slice(&data);
                    }
                    Err(frame) => {
                        if !self.buffer.is_empty() {
                            self.failed = true;
                            return Poll::Ready(Some(Err(Status::invalid_argument(
                                "code-mode response ended with a partial message",
                            ))));
                        }
                        return Poll::Ready(Some(Ok(frame)));
                    }
                },
                Poll::Ready(Some(Err(error))) => return Poll::Ready(Some(Err(error))),
                Poll::Ready(None) if self.buffer.is_empty() => return Poll::Ready(None),
                Poll::Ready(None) => {
                    self.failed = true;
                    return Poll::Ready(Some(Err(Status::invalid_argument(
                        "code-mode response ended with a partial message",
                    ))));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.failed || (self.buffer.is_empty() && self.inner.is_end_stream())
    }

    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::default()
    }
}

fn preflight_response(shape: ResponseShape, message: &[u8]) -> Result<(), Status> {
    let outer_fields: &[u64] = match shape {
        ResponseShape::Execute => &[2],
        ResponseShape::Wait => &[1, 2],
        ResponseShape::Other => return Ok(()),
    };
    let mut content_items = 0usize;
    visit_fields(message, |field, wire_type, value| {
        if wire_type == 2 && outer_fields.contains(&field) {
            count_fields(value, /*target_field*/ 2, &mut content_items)?;
        }
        Ok(())
    })?;
    if content_items > MAX_CONTENT_ITEMS {
        return Err(Status::resource_exhausted(format!(
            "code-mode response exceeds {MAX_CONTENT_ITEMS} content items"
        )));
    }
    Ok(())
}

fn count_fields(message: &[u8], target_field: u64, count: &mut usize) -> Result<(), Status> {
    visit_fields(message, |field, wire_type, _| {
        if field == target_field && wire_type == 2 {
            *count = count
                .checked_add(1)
                .ok_or_else(|| Status::resource_exhausted("content item count overflowed"))?;
        }
        Ok(())
    })
}

fn visit_fields(
    message: &[u8],
    mut visit: impl FnMut(u64, u64, &[u8]) -> Result<(), Status>,
) -> Result<(), Status> {
    let mut cursor = 0usize;
    while cursor < message.len() {
        let key = read_varint(message, &mut cursor)?;
        let field = key >> 3;
        let wire_type = key & 7;
        if field == 0 {
            return Err(Status::invalid_argument("invalid code-mode response field"));
        }
        match wire_type {
            0 => {
                read_varint(message, &mut cursor)?;
                visit(field, wire_type, &[])?;
            }
            1 => {
                let value = take(message, &mut cursor, /*bytes*/ 8)?;
                visit(field, wire_type, value)?;
            }
            2 => {
                let length = read_varint(message, &mut cursor)?;
                let length = usize::try_from(length)
                    .map_err(|_| Status::invalid_argument("invalid code-mode response length"))?;
                let value = take(message, &mut cursor, length)?;
                visit(field, wire_type, value)?;
            }
            5 => {
                let value = take(message, &mut cursor, /*bytes*/ 4)?;
                visit(field, wire_type, value)?;
            }
            3 | 4 | 6 | 7 => {
                return Err(Status::invalid_argument(
                    "invalid code-mode response wire type",
                ));
            }
            8..=u64::MAX => {
                return Err(Status::invalid_argument(
                    "invalid code-mode response wire type",
                ));
            }
        }
    }
    Ok(())
}

fn read_varint(message: &[u8], cursor: &mut usize) -> Result<u64, Status> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *message
            .get(*cursor)
            .ok_or_else(|| Status::invalid_argument("truncated code-mode response varint"))?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(Status::invalid_argument(
                "overflowing code-mode response varint",
            ));
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(Status::invalid_argument(
        "overflowing code-mode response varint",
    ))
}

fn take<'a>(message: &'a [u8], cursor: &mut usize, bytes: usize) -> Result<&'a [u8], Status> {
    let end = (*cursor)
        .checked_add(bytes)
        .filter(|end| *end <= message.len())
        .ok_or_else(|| Status::invalid_argument("truncated code-mode response field"))?;
    let value = &message[*cursor..end];
    *cursor = end;
    Ok(value)
}

#[cfg(test)]
#[path = "response_admission_tests.rs"]
mod tests;
