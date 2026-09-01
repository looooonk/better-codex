use std::sync::Arc;
use std::sync::Weak;

use codex_code_mode_protocol::CellId;
use codex_code_mode_protocol::CodeModeNestedToolCall;
use codex_code_mode_protocol::CodeModeSessionDelegate;
use codex_code_mode_protocol::NotificationFuture;
use codex_code_mode_protocol::ToolInvocationFuture;
use codex_code_mode_protocol::encode_bounded_json;
use codex_code_mode_protocol::grpc as proto;
use codex_code_mode_protocol::grpc::MAX_APPLICATION_MESSAGE_BYTES;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::session::GrpcSession;

pub(super) struct GrpcDelegate {
    session: Weak<GrpcSession>,
}

impl GrpcDelegate {
    pub(super) fn new(session: Weak<GrpcSession>) -> Self {
        Self { session }
    }
}

impl CodeModeSessionDelegate for GrpcDelegate {
    fn invoke_tool<'a>(
        &'a self,
        invocation: CodeModeNestedToolCall,
        cancellation: CancellationToken,
    ) -> ToolInvocationFuture<'a> {
        Box::pin(async move {
            let session = self
                .session
                .upgrade()
                .ok_or_else(|| "code-mode session is closed".to_string())?;
            let _permit = session.delegate_permit()?;
            let execution_id = session
                .execution_id(invocation.cell_id.as_str(), &cancellation)
                .await?;
            let byte_reservation =
                session.reserve_tool_bytes(MAX_APPLICATION_MESSAGE_BYTES)?;
            let input_json = invocation
                .input
                .as_ref()
                .map(|input| encode_bounded_json(input, MAX_APPLICATION_MESSAGE_BYTES))
                .transpose()
                .map_err(|error| format!("failed to encode code-mode tool input: {error}"))?;
            let invocation_id = Uuid::new_v4();
            let (response, receiver) = oneshot::channel();
            session
                .dispatch_tool_reserved(
                    invocation,
                    execution_id,
                    invocation_id,
                    input_json,
                    response,
                    byte_reservation,
                    &cancellation,
                )
                .await?;
            let mut pending = PendingToolCall {
                session: Arc::clone(&session),
                id: Some(invocation_id),
            };
            let result = tokio::select! {
                biased;
                result = receiver => result
                    .map_err(|_| "code-mode client closed before returning tool output".to_string())?,
                _ = cancellation.cancelled() => {
                    Err("code mode delegate request cancelled".to_string())
                }
                _ = session.closed.cancelled() => {
                    Err("code-mode session closed before returning tool output".to_string())
                }
            };
            if result.is_ok() {
                pending.id = None;
            }
            result
        })
    }

    fn notify<'a>(
        &'a self,
        call_id: String,
        cell_id: CellId,
        text: String,
        cancellation: CancellationToken,
    ) -> NotificationFuture<'a> {
        Box::pin(async move {
            let session = self
                .session
                .upgrade()
                .ok_or_else(|| "code-mode session is closed".to_string())?;
            let _permit = session.delegate_permit()?;
            let execution_id = session
                .execution_id(cell_id.as_str(), &cancellation)
                .await?;
            let notification_id = Uuid::new_v4();
            let (acknowledgement, receiver) = oneshot::channel();
            session.register_notification(notification_id, acknowledgement)?;
            let mut pending = PendingNotification {
                session: Arc::clone(&session),
                id: Some(notification_id),
                publication: NotificationPublication::Unpublished,
            };
            session
                .send_event(
                    proto::session_event::Event::Notification(proto::Notification {
                        notification_id: notification_id.to_string(),
                        execution_id,
                        cell_id: cell_id.to_string(),
                        call_id,
                        text,
                    }),
                    &cancellation,
                )
                .await?;
            pending.publication = NotificationPublication::Published;
            let result = tokio::select! {
                biased;
                result = receiver => result.map_err(|_| {
                    "code-mode session closed before acknowledging notification".to_string()
                }),
                _ = cancellation.cancelled() => {
                    pending.cancel();
                    Err("code mode notification was cancelled".to_string())
                }
                _ = session.closed.cancelled() => {
                    pending.cancel();
                    Err("code-mode session closed before acknowledging notification".to_string())
                }
            };
            if result.is_ok() {
                pending.id = None;
            }
            result
        })
    }

    fn cell_closed(&self, cell_id: &CellId) {
        if let Some(session) = self.session.upgrade() {
            session.close_cell(cell_id.as_str());
        }
    }
}

struct PendingToolCall {
    session: Arc<GrpcSession>,
    id: Option<Uuid>,
}

impl Drop for PendingToolCall {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            self.session.cancel_invocation(id);
        }
    }
}

struct PendingNotification {
    session: Arc<GrpcSession>,
    id: Option<Uuid>,
    publication: NotificationPublication,
}

enum NotificationPublication {
    Unpublished,
    Published,
}

impl PendingNotification {
    fn cancel(&mut self) {
        if let Some(id) = self.id.take() {
            match self.publication {
                NotificationPublication::Unpublished => self.session.discard_notification(id),
                NotificationPublication::Published => self.session.cancel_notification(id),
            }
        }
    }
}

impl Drop for PendingNotification {
    fn drop(&mut self) {
        self.cancel();
    }
}
