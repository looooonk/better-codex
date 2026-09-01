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
use crate::thread_status::ThreadWatchManager;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ThreadStatus;
use codex_core::CodexThread;
use codex_core::ThreadManager;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::SessionSource;
use codex_rollout::StateDbHandle;
use codex_state::MAX_QUEUE_IDENTIFIER_BYTES;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::ThreadStore;
use codex_thread_store::ThreadStoreError;

use super::thread_queue_admission::QueueAdmissionTracker;
use super::thread_queue_support::queue_error;

#[derive(Clone)]
pub(crate) struct ThreadQueueService {
    pub(super) thread_manager: Weak<ThreadManager>,
    pub(super) thread_store: Arc<dyn ThreadStore>,
    outgoing: Arc<OutgoingMessageSender>,
    pub(super) state_db: Option<StateDbHandle>,
    request_serialization_queues: RequestSerializationQueues,
    pub(super) thread_state_manager: ThreadStateManager,
    thread_watch_manager: ThreadWatchManager,
    pub(super) admission_tracker: QueueAdmissionTracker,
}

impl ThreadQueueService {
    pub(crate) fn new(
        thread_manager: Weak<ThreadManager>,
        thread_store: Arc<dyn ThreadStore>,
        outgoing: Arc<OutgoingMessageSender>,
        state_db: Option<StateDbHandle>,
        request_serialization_queues: RequestSerializationQueues,
        thread_state_manager: ThreadStateManager,
        thread_watch_manager: ThreadWatchManager,
    ) -> Self {
        Self {
            thread_manager,
            thread_store,
            outgoing,
            state_db,
            request_serialization_queues,
            thread_state_manager,
            thread_watch_manager,
            admission_tracker: QueueAdmissionTracker::default(),
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

    pub(super) async fn recover_before_queue_access(
        &self,
        thread_id: ThreadId,
        loaded_thread: Option<&CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        let disposition = if let Some(thread) = loaded_thread {
            let watch_status = self
                .thread_watch_manager
                .loaded_status_for_thread(&thread_id.to_string())
                .await;
            match thread.agent_status().await {
                AgentStatus::PendingInit if !matches!(watch_status, ThreadStatus::Idle) => {
                    return Ok(());
                }
                AgentStatus::PendingInit | AgentStatus::Interrupted | AgentStatus::Completed(_) => {
                    thread.flush_rollout().await.map_err(|error| {
                        internal_error(format!(
                            "failed to flush idle thread before queue recovery: {error}"
                        ))
                    })?;
                    self.recover_serialized(thread_id).await?
                }
                AgentStatus::Errored(_) => {
                    thread.flush_rollout().await.map_err(|error| {
                        internal_error(format!(
                            "failed to flush errored thread before terminal queue recovery: {error}"
                        ))
                    })?;
                    self.recover_errored_serialized(thread_id).await?
                }
                AgentStatus::Running | AgentStatus::Shutdown | AgentStatus::NotFound => {
                    return Ok(());
                }
            }
        } else {
            self.recover_serialized(thread_id).await?
        };
        if matches!(
            disposition,
            Some(codex_state::QueueTerminalDisposition::Continue)
        ) {
            self.schedule_dispatch(thread_id).await;
        }
        Ok(())
    }

    pub(super) fn schedule_dispatch(
        &self,
        thread_id: ThreadId,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let service = self.clone();
            self.enqueue_background(thread_id, async move {
                if let Err(error) = service.dispatch_next(thread_id).await {
                    tracing::warn!(%thread_id, message = %error.message, "failed to dispatch thread queue");
                }
            })
            .await;
        })
    }

    pub(super) async fn recover_and_dispatch_serialized(
        &self,
        thread_id: ThreadId,
    ) -> Result<(), JSONRPCErrorError> {
        let service = self.clone();
        self.request_serialization_queues
            .run_exclusive_or_enqueue_and_wait(thread_serialization_key(thread_id), async move {
                service.recover_and_dispatch(thread_id).await
            })
            .await
            .map_err(|_| {
                internal_error("serialized thread queue task ended before dispatch completed")
            })?
    }

    pub(super) async fn recover_serialized(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<codex_state::QueueTerminalDisposition>, JSONRPCErrorError> {
        let service = self.clone();
        self.request_serialization_queues
            .run_exclusive_or_enqueue_and_wait(thread_serialization_key(thread_id), async move {
                service.recover(thread_id).await
            })
            .await
            .map_err(|_| {
                internal_error("serialized thread queue task ended before recovery completed")
            })?
    }

    async fn recover_errored_serialized(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<codex_state::QueueTerminalDisposition>, JSONRPCErrorError> {
        let service = self.clone();
        self.request_serialization_queues
            .run_exclusive_or_enqueue_and_wait(thread_serialization_key(thread_id), async move {
                service.recover_errored(thread_id).await
            })
            .await
            .map_err(|_| {
                internal_error("serialized thread queue task ended before recovery completed")
            })?
    }

    async fn recover_and_dispatch(&self, thread_id: ThreadId) -> Result<(), JSONRPCErrorError> {
        if let Some(thread_manager) = self.thread_manager.upgrade()
            && let Ok(thread) = thread_manager.get_thread(thread_id).await
        {
            match thread.agent_status().await {
                AgentStatus::Running | AgentStatus::Shutdown | AgentStatus::NotFound => {
                    return Ok(());
                }
                AgentStatus::Errored(_) => {
                    let state_db = self.state_db()?;
                    if state_db
                        .active_queued_submission(thread_id)
                        .await
                        .map_err(queue_error)?
                        .is_some()
                    {
                        thread.flush_rollout().await.map_err(|error| {
                            internal_error(format!(
                                "failed to flush errored thread before terminal queue recovery: {error}"
                            ))
                        })?;
                        let _ = self.recover_errored(thread_id).await?;
                        if state_db
                            .active_queued_submission(thread_id)
                            .await
                            .map_err(queue_error)?
                            .is_some()
                        {
                            return Ok(());
                        }
                    }
                    return self.dispatch_next(thread_id).await;
                }
                AgentStatus::PendingInit | AgentStatus::Interrupted | AgentStatus::Completed(_) => {
                }
            }
            thread.flush_rollout().await.map_err(|error| {
                internal_error(format!(
                    "failed to flush loaded thread before queue recovery: {error}"
                ))
            })?;
        }
        let _ = self.recover(thread_id).await?;
        self.dispatch_next(thread_id).await
    }

    pub(super) async fn enqueue_background(
        &self,
        thread_id: ThreadId,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        self.request_serialization_queues
            .enqueue_background(
                thread_serialization_key(thread_id),
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
        if self
            .thread_state_manager
            .current_listener_command_tx(thread_id)
            .is_none()
        {
            return Ok(());
        }
        thread.flush_rollout().await.map_err(|error| {
            internal_error(format!(
                "failed to flush thread before queue dispatch: {error}"
            ))
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
}

fn thread_serialization_key(thread_id: ThreadId) -> RequestSerializationQueueKey {
    RequestSerializationQueueKey::Thread {
        thread_id: thread_id.to_string(),
    }
}
