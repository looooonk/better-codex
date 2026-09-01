use std::sync::Arc;

use super::GrpcStream;
use crate::MAX_ACTIVE_CELLS;
use crate::MAX_PENDING_DELEGATE_CALLS;
use codex_code_mode_protocol::grpc as proto;
use codex_code_mode_protocol::grpc::MAX_APPLICATION_MESSAGE_BYTES;
use futures::StreamExt;
use prost::Message;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::TryAcquireError;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

const MAX_BUFFERED_CONTROL_EVENTS: usize = MAX_PENDING_DELEGATE_CALLS * 2 + MAX_ACTIVE_CELLS;
pub(super) const MAX_SESSION_EVENT_BYTES: usize = MAX_APPLICATION_MESSAGE_BYTES;
pub(crate) const MAX_HOST_EVENT_BYTES: usize = MAX_APPLICATION_MESSAGE_BYTES * 4;

#[derive(Clone)]
pub(super) struct EventSender {
    sender: mpsc::UnboundedSender<QueuedEvent>,
    event_permits: Arc<Semaphore>,
    session_byte_permits: Arc<Semaphore>,
    host_byte_permits: Arc<Semaphore>,
    closed: CancellationToken,
    writer: TaskTracker,
}

struct QueuedEvent {
    message: proto::SessionEvent,
    event_permit: OwnedSemaphorePermit,
    session_byte_permit: OwnedSemaphorePermit,
    host_byte_permit: OwnedSemaphorePermit,
    cell_permit: Option<OwnedSemaphorePermit>,
}

pub(super) struct BufferedEvent {
    message: proto::SessionEvent,
    _event_permit: OwnedSemaphorePermit,
    _session_byte_permit: OwnedSemaphorePermit,
    _host_byte_permit: OwnedSemaphorePermit,
}

impl EventSender {
    pub(super) fn new(
        output: mpsc::Sender<BufferedEvent>,
        closed: CancellationToken,
        session_byte_permits: Arc<Semaphore>,
        host_byte_permits: Arc<Semaphore>,
    ) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel::<QueuedEvent>();
        let writer_closed = closed.clone();
        let writer = TaskTracker::new();
        writer.spawn(async move {
            loop {
                let event = tokio::select! {
                    _ = writer_closed.cancelled() => return,
                    event = receiver.recv() => match event {
                        Some(event) => event,
                        None => return,
                    },
                };
                let QueuedEvent {
                    message,
                    event_permit,
                    session_byte_permit,
                    host_byte_permit,
                    cell_permit,
                } = event;
                let output_event = BufferedEvent {
                    message,
                    _event_permit: event_permit,
                    _session_byte_permit: session_byte_permit,
                    _host_byte_permit: host_byte_permit,
                };
                tokio::select! {
                    _ = writer_closed.cancelled() => return,
                    result = output.send(output_event) => {
                        if result.is_err() {
                            writer_closed.cancel();
                            return;
                        }
                    }
                }
                drop(cell_permit);
            }
        });
        writer.close();
        Self {
            sender,
            event_permits: Arc::new(Semaphore::new(MAX_BUFFERED_CONTROL_EVENTS)),
            session_byte_permits,
            host_byte_permits,
            closed,
            writer,
        }
    }

    pub(super) async fn shutdown(&self) {
        self.closed.cancel();
        self.writer.wait().await;
    }

    pub(super) async fn send(
        &self,
        event: proto::session_event::Event,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        let (message, bytes) = validate_event(event)?;
        if self.closed.is_cancelled() {
            return Err("code-mode session event stream is closed".to_string());
        }
        if cancellation.is_cancelled() {
            return Err("code-mode session event was cancelled".to_string());
        }
        let (event_permit, session_byte_permit, host_byte_permit) = (|| {
            Ok::<_, String>((
                try_acquire(&self.event_permits, /*count*/ 1)?,
                try_acquire(&self.session_byte_permits, bytes)?,
                try_acquire(&self.host_byte_permits, bytes)?,
            ))
        })()?;
        self.enqueue(
            message,
            event_permit,
            session_byte_permit,
            host_byte_permit,
            /*cell_permit*/ None,
        )
    }

    pub(super) fn send_now(
        &self,
        event: proto::session_event::Event,
        cell_permit: Option<OwnedSemaphorePermit>,
    ) -> Result<(), String> {
        let (message, bytes) = validate_event(event)?;
        if self.closed.is_cancelled() {
            return Err("code-mode session event stream is closed".to_string());
        }
        let permits: Result<_, String> = (|| {
            Ok((
                try_acquire(&self.event_permits, /*count*/ 1)?,
                try_acquire(&self.session_byte_permits, bytes)?,
                try_acquire(&self.host_byte_permits, bytes)?,
            ))
        })();
        let (event_permit, session_byte_permit, host_byte_permit) =
            permits.inspect_err(|_error| {
                self.closed.cancel();
            })?;
        self.enqueue(
            message,
            event_permit,
            session_byte_permit,
            host_byte_permit,
            cell_permit,
        )
    }

    fn enqueue(
        &self,
        message: proto::SessionEvent,
        event_permit: OwnedSemaphorePermit,
        session_byte_permit: OwnedSemaphorePermit,
        host_byte_permit: OwnedSemaphorePermit,
        cell_permit: Option<OwnedSemaphorePermit>,
    ) -> Result<(), String> {
        self.sender
            .send(QueuedEvent {
                message,
                event_permit,
                session_byte_permit,
                host_byte_permit,
                cell_permit,
            })
            .map_err(|_| {
                self.closed.cancel();
                "code-mode session event stream is closed".to_string()
            })
    }
}

pub(super) fn event_stream(
    receiver: mpsc::Receiver<BufferedEvent>,
) -> GrpcStream<proto::SessionEvent> {
    Box::pin(ReceiverStream::new(receiver).map(|event| Ok(event.message)))
}

fn try_acquire(permits: &Arc<Semaphore>, count: u32) -> Result<OwnedSemaphorePermit, String> {
    Arc::clone(permits)
        .try_acquire_many_owned(count)
        .map_err(|error| match error {
            TryAcquireError::NoPermits => "code-mode session event queue is full".to_string(),
            TryAcquireError::Closed => "code-mode session event queue is closed".to_string(),
        })
}

fn validate_event(
    event: proto::session_event::Event,
) -> Result<(proto::SessionEvent, u32), String> {
    let message = proto::SessionEvent { event: Some(event) };
    let encoded_len = message.encoded_len();
    if encoded_len > MAX_APPLICATION_MESSAGE_BYTES {
        return Err(format!(
            "code-mode session event exceeds the {MAX_APPLICATION_MESSAGE_BYTES}-byte application limit"
        ));
    }
    let bytes = u32::try_from(encoded_len)
        .map_err(|_| "code-mode session event byte size exceeds this platform".to_string())?;
    Ok((message, bytes.max(1)))
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
