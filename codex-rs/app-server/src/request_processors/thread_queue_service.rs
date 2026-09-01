use std::sync::Arc;
use std::sync::Weak;

use crate::error_code::internal_error;
use crate::error_code::invalid_params;
use crate::error_code::invalid_request;
use crate::outgoing_message::ConnectionRequestId;
use crate::outgoing_message::OutgoingMessageSender;
use crate::request_serialization::RequestSerializationAccess;
use crate::request_serialization::RequestSerializationQueueKey;
use crate::request_serialization::RequestSerializationQueues;
use crate::thread_state::ThreadListenerCommand;
use crate::thread_state::ThreadStateManager;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::TurnStatus;
use codex_core::CodexThread;
use codex_core::ThreadManager;
use codex_protocol::ThreadId;
use codex_protocol::protocol::QueuedTurnStartRejectionReason;
use codex_protocol::protocol::QueuedTurnStartSubmission;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::W3cTraceContext;
use codex_rollout::StateDbHandle;
use codex_state::MAX_QUEUE_IDENTIFIER_BYTES;
use codex_state::QueueClaimResult;
use codex_state::QueuedSubmissionAdmissionRejection;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::ThreadStore;
use codex_thread_store::ThreadStoreError;
use uuid::Uuid;

use super::thread_queue_support::queue_error;
use super::thread_queue_support::queued_core_input;
use super::thread_queue_support::turn_status;

#[derive(Clone)]
pub(crate) struct ThreadQueueService {
    pub(super) thread_manager: Weak<ThreadManager>,
    pub(super) thread_store: Arc<dyn ThreadStore>,
    outgoing: Arc<OutgoingMessageSender>,
    pub(super) state_db: Option<StateDbHandle>,
    request_serialization_queues: RequestSerializationQueues,
    thread_state_manager: ThreadStateManager,
}

pub(super) struct QueueStartResult {
    pub(super) turn_id: String,
    pub(super) status: TurnStatus,
    pub(super) queue_changed: bool,
}

pub(super) struct QueueStartFailure {
    pub(super) error: JSONRPCErrorError,
    pub(super) queue_changed: bool,
}

impl ThreadQueueService {
    pub(crate) fn new(
        thread_manager: Weak<ThreadManager>,
        thread_store: Arc<dyn ThreadStore>,
        outgoing: Arc<OutgoingMessageSender>,
        state_db: Option<StateDbHandle>,
        request_serialization_queues: RequestSerializationQueues,
        thread_state_manager: ThreadStateManager,
    ) -> Self {
        Self {
            thread_manager,
            thread_store,
            outgoing,
            state_db,
            request_serialization_queues,
            thread_state_manager,
        }
    }

    pub(super) fn state_db(&self) -> Result<&StateDbHandle, JSONRPCErrorError> {
        self.state_db
            .as_ref()
            .ok_or_else(|| invalid_request("user message queue is unavailable"))
    }

    pub(super) fn outgoing(&self) -> &Arc<OutgoingMessageSender> {
        &self.outgoing
    }

    pub(super) async fn require_thread(
        &self,
        raw_thread_id: &str,
    ) -> Result<(ThreadId, Option<Arc<CodexThread>>, SessionSource), JSONRPCErrorError> {
        if raw_thread_id.len() > MAX_QUEUE_IDENTIFIER_BYTES {
            return Err(invalid_params("thread id is too long"));
        }
        let thread_id = ThreadId::from_string(raw_thread_id)
            .map_err(|error| invalid_request(format!("invalid thread id: {error}")))?;
        if let Some(thread_manager) = self.thread_manager.upgrade()
            && let Ok(thread) = thread_manager.get_thread(thread_id).await
        {
            let snapshot = thread.config_snapshot().await;
            if snapshot.ephemeral {
                return Err(invalid_request(format!(
                    "ephemeral thread does not support queued submissions: {thread_id}"
                )));
            }
            return Ok((thread_id, Some(thread), snapshot.session_source));
        }

        let stored = self
            .thread_store
            .read_thread(ReadThreadParams {
                thread_id,
                include_archived: true,
                include_history: false,
            })
            .await
            .map_err(|error| match error {
                ThreadStoreError::ThreadNotFound { .. } => {
                    invalid_request(format!("thread not found: {thread_id}"))
                }
                error => internal_error(format!("failed to read thread: {error}")),
            })?;
        if stored.archived_at.is_some() {
            return Err(invalid_request(format!(
                "thread {thread_id} is archived; unarchive it before changing its queue"
            )));
        }
        Ok((thread_id, None, stored.source))
    }

    pub(super) async fn send_changed_response(
        &self,
        request_id: ConnectionRequestId,
        response: ClientResponsePayload,
        thread_id: ThreadId,
    ) {
        self.outgoing.send_response_as(request_id, response).await;
        self.notify_changed(thread_id).await;
    }

    pub(crate) async fn notify_changed(&self, thread_id: ThreadId) {
        let Some(command_tx) = self
            .thread_state_manager
            .current_listener_command_tx(thread_id)
        else {
            return;
        };
        if command_tx
            .send(ThreadListenerCommand::EmitThreadQueueChanged)
            .is_err()
        {
            tracing::debug!(%thread_id, "thread queue notification listener is closed");
        }
    }

    pub(super) async fn wake_if_loaded(&self, thread_id: ThreadId) {
        let Some(thread_manager) = self.thread_manager.upgrade() else {
            return;
        };
        if let Ok(thread) = thread_manager.get_thread(thread_id).await {
            thread.emit_thread_idle_lifecycle_if_idle().await;
        }
    }

    pub(super) async fn schedule_dispatch(&self, thread_id: ThreadId) {
        let service = self.clone();
        self.enqueue_background(thread_id, async move {
            if let Err(error) = service.recover(thread_id).await {
                tracing::warn!(%thread_id, message = %error.message, "failed to recover thread queue");
                return;
            }
            if let Err(error) = service.dispatch_next(thread_id).await {
                tracing::warn!(%thread_id, message = %error.message, "failed to dispatch thread queue");
            }
        })
        .await;
    }

    pub(super) async fn enqueue_background(
        &self,
        thread_id: ThreadId,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        self.request_serialization_queues
            .enqueue_background(
                RequestSerializationQueueKey::Thread {
                    thread_id: thread_id.to_string(),
                },
                RequestSerializationAccess::Exclusive,
                future,
            )
            .await;
    }

    async fn dispatch_next(&self, thread_id: ThreadId) -> Result<(), JSONRPCErrorError> {
        let state_db = self.state_db()?;
        if state_db
            .thread_queue_pause_reason(thread_id)
            .await
            .map_err(queue_error)?
            .is_some()
        {
            return Ok(());
        }
        if state_db
            .active_queued_submission(thread_id)
            .await
            .map_err(queue_error)?
            .is_some()
        {
            return Ok(());
        }
        if state_db
            .list_queued_submissions(thread_id, /*offset*/ 0, /*limit*/ 1)
            .await
            .map_err(queue_error)?
            .is_empty()
        {
            return Ok(());
        }
        let Some(thread_manager) = self.thread_manager.upgrade() else {
            return Ok(());
        };
        let Ok(thread) = thread_manager.get_thread(thread_id).await else {
            return Ok(());
        };
        thread.flush_rollout().await.map_err(|error| {
            internal_error(format!("failed to flush thread before queue dispatch: {error}"))
        })?;
        match self
            .start_claimed(thread_id, thread.as_ref(), None, /*trace*/ None)
            .await
        {
            Ok(started) if started.queue_changed => self.notify_changed(thread_id).await,
            Ok(_) => {}
            Err(failure) => {
                if failure.queue_changed {
                    self.notify_changed(thread_id).await;
                }
                return Err(failure.error);
            }
        }
        Ok(())
    }

    pub(super) async fn start_explicit(
        &self,
        thread_id: ThreadId,
        thread: &CodexThread,
        queued_submission_id: Option<&str>,
        trace: Option<W3cTraceContext>,
    ) -> Result<QueueStartResult, QueueStartFailure> {
        let proposed_turn_id = Uuid::now_v7().to_string();
        let claim = self
            .state_db()
            .map_err(QueueStartFailure::unchanged)?
            .claim_queued_submission_and_resume(
                thread_id,
                queued_submission_id,
                &proposed_turn_id,
            )
            .await
            .map_err(queue_error)
            .map_err(QueueStartFailure::unchanged)?;
        let resumed = claim.resumed;
        match self
            .start_claim(thread_id, thread, queued_submission_id, trace, claim.claim)
            .await
        {
            Ok(mut started) => {
                started.queue_changed |= resumed;
                Ok(started)
            }
            Err(mut failure) => {
                failure.queue_changed |= resumed;
                Err(failure)
            }
        }
    }

    async fn start_claimed(
        &self,
        thread_id: ThreadId,
        thread: &CodexThread,
        queued_submission_id: Option<&str>,
        trace: Option<W3cTraceContext>,
    ) -> Result<QueueStartResult, QueueStartFailure> {
        let proposed_turn_id = Uuid::now_v7().to_string();
        let claim = self
            .state_db()
            .map_err(QueueStartFailure::unchanged)?
            .claim_queued_submission(thread_id, queued_submission_id, &proposed_turn_id)
            .await
            .map_err(queue_error)
            .map_err(QueueStartFailure::unchanged)?;
        self.start_claim(thread_id, thread, queued_submission_id, trace, claim)
            .await
    }

    async fn start_claim(
        &self,
        thread_id: ThreadId,
        thread: &CodexThread,
        queued_submission_id: Option<&str>,
        trace: Option<W3cTraceContext>,
        claim: QueueClaimResult,
    ) -> Result<QueueStartResult, QueueStartFailure> {
        let record = match claim {
            QueueClaimResult::Claimed(record) => record,
            QueueClaimResult::Existing(record) => {
                let turn_id = record.turn_id.clone().ok_or_else(|| {
                    QueueStartFailure::unchanged(internal_error(
                        "claimed queue item is missing its turn id",
                    ))
                })?;
                return Ok(QueueStartResult {
                    turn_id,
                    status: turn_status(&record),
                    queue_changed: false,
                });
            }
            QueueClaimResult::Busy(_) => {
                return Err(QueueStartFailure::unchanged(invalid_request(
                    "thread already has an active or pending turn",
                )));
            }
            QueueClaimResult::Empty => {
                return Err(QueueStartFailure::unchanged(invalid_request(
                    match queued_submission_id {
                        Some(id) => format!("queued submission not found: {id}"),
                        None => "thread queue is empty".to_string(),
                    },
                )));
            }
        };
        let turn_id = record.turn_id.clone().ok_or_else(|| {
            QueueStartFailure::changed(internal_error(
                "claimed queue item is missing its turn id",
            ))
        })?;
        let submission = thread
            .start_queued_turn(
                turn_id.clone(),
                queued_core_input(&record).map_err(QueueStartFailure::changed)?,
                record.client_user_message_id.clone(),
                trace,
            )
            .await;
        match submission {
            Ok(QueuedTurnStartSubmission::Persisted) => {
                self.state_db()
                    .map_err(QueueStartFailure::changed)?
                    .mark_queued_submission_inflight(thread_id, &turn_id)
                    .await
                    .map_err(queue_error)
                    .map_err(QueueStartFailure::changed)?;
                Ok(QueueStartResult {
                    turn_id,
                    status: TurnStatus::InProgress,
                    queue_changed: true,
                })
            }
            Ok(QueuedTurnStartSubmission::NotSubmitted { reason }) => {
                match reason {
                    QueuedTurnStartRejectionReason::Busy
                    | QueuedTurnStartRejectionReason::PendingTriggerTurn => {
                        self.state_db()
                            .map_err(QueueStartFailure::changed)?
                            .release_queued_submission_claim(thread_id, &record.id, &turn_id)
                            .await
                            .map_err(queue_error)
                            .map_err(QueueStartFailure::changed)?;
                        return Err(QueueStartFailure::unchanged(invalid_request(format!(
                            "queued submission was not started: {reason:?}"
                        ))));
                    }
                    QueuedTurnStartRejectionReason::RejectedByHook => {
                        self.state_db()
                            .map_err(QueueStartFailure::changed)?
                            .mark_queued_submission_admission_rejected(
                                thread_id,
                                &turn_id,
                                QueuedSubmissionAdmissionRejection::Hook,
                            )
                            .await
                            .map_err(queue_error)
                            .map_err(QueueStartFailure::changed)?;
                    }
                }
                Err(QueueStartFailure::changed(invalid_request(format!(
                    "queued submission was not started: {reason:?}"
                ))))
            }
            Err(error) => Err(QueueStartFailure::changed(internal_error(format!(
                "failed to admit queued submission: {error}"
            )))),
        }
    }
}

impl QueueStartFailure {
    fn changed(error: JSONRPCErrorError) -> Self {
        Self {
            error,
            queue_changed: true,
        }
    }

    fn unchanged(error: JSONRPCErrorError) -> Self {
        Self {
            error,
            queue_changed: false,
        }
    }
}
