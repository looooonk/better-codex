use std::sync::Arc;

use crate::error_code::internal_error;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_core::CodexThread;
use codex_core::config::Config;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_protocol::ThreadId;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnAbortReason;
use codex_state::QueueTerminalDisposition;
use codex_state::QueuedSubmissionTerminalStatus;
use codex_thread_store::LoadThreadHistoryParams;
use tokio::sync::oneshot;

use super::thread_queue_service::ThreadQueueService;
use super::thread_queue_support::QueueRecoveryOutcome;
use super::thread_queue_support::queue_error;
use super::thread_queue_support::queue_recovery_outcome;
use super::thread_queue_support::terminal_disposition;

#[derive(Clone)]
enum ObservedQueueEvent {
    Started { turn_id: String },
    Completed { turn_id: String, failed: bool },
    Aborted { turn_id: String, reason: TurnAbortReason },
}

impl ThreadQueueService {
    pub(crate) async fn observe_event(
        &self,
        thread: Arc<CodexThread>,
        thread_id: ThreadId,
        event: &EventMsg,
    ) {
        let observed = match event {
            EventMsg::TurnStarted(event) => ObservedQueueEvent::Started {
                turn_id: event.turn_id.clone(),
            },
            EventMsg::TurnComplete(event) => ObservedQueueEvent::Completed {
                turn_id: event.turn_id.clone(),
                failed: event.error.is_some(),
            },
            EventMsg::TurnAborted(event) => {
                let Some(turn_id) = event.turn_id.clone() else {
                    return;
                };
                ObservedQueueEvent::Aborted {
                    turn_id,
                    reason: event.reason.clone(),
                }
            }
            _ => return,
        };
        let (completion_tx, completion_rx) = oneshot::channel();
        let service = self.clone();
        self.enqueue_background(thread_id, async move {
            service.process_observed_event(thread, thread_id, observed).await;
            let _ = completion_tx.send(());
        })
        .await;
        let _ = completion_rx.await;
    }

    async fn process_observed_event(
        &self,
        thread: Arc<CodexThread>,
        thread_id: ThreadId,
        event: ObservedQueueEvent,
    ) {
        let Some(state_db) = self.state_db.as_ref() else {
            return;
        };
        match event {
            ObservedQueueEvent::Started { turn_id } => {
                if let Err(error) = state_db
                    .mark_queued_submission_inflight(thread_id, &turn_id)
                    .await
                {
                    tracing::warn!(%thread_id, %error, "failed to mark queued turn inflight");
                }
            }
            ObservedQueueEvent::Completed { turn_id, failed } => {
                if !flush_terminal_rollout(&thread, thread_id).await {
                    return;
                }
                let status = match state_db
                    .queued_submission_for_turn(thread_id, &turn_id)
                    .await
                {
                    Ok(Some(record)) if record.admission_rejection.is_some() => {
                        QueuedSubmissionTerminalStatus::Failed
                    }
                    Ok(_) if failed => QueuedSubmissionTerminalStatus::Failed,
                    Ok(_) => QueuedSubmissionTerminalStatus::Completed,
                    Err(error) => {
                        tracing::warn!(%thread_id, %error, "failed to inspect queued turn completion");
                        return;
                    }
                };
                self.finish_observed(
                    thread_id,
                    &turn_id,
                    status,
                    QueueTerminalDisposition::Continue,
                )
                .await;
            }
            ObservedQueueEvent::Aborted { turn_id, reason } => {
                if !flush_terminal_rollout(&thread, thread_id).await {
                    return;
                }
                self.finish_observed(
                    thread_id,
                    &turn_id,
                    QueuedSubmissionTerminalStatus::Interrupted,
                    terminal_disposition(&reason),
                )
                .await;
            }
        }
    }

    async fn finish_observed(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        status: QueuedSubmissionTerminalStatus,
        disposition: QueueTerminalDisposition,
    ) {
        match self
            .state_db
            .as_ref()
            .expect("state database checked by caller")
            .finish_queued_submission(thread_id, turn_id, status, disposition)
            .await
        {
            Ok(true) => self.notify_changed(thread_id).await,
            Ok(false) => {}
            Err(error) => tracing::warn!(%thread_id, %error, "failed to finish queued turn"),
        }
    }

    pub(super) async fn recover(&self, thread_id: ThreadId) -> Result<(), JSONRPCErrorError> {
        let Some(active) = self
            .state_db()?
            .active_queued_submission(thread_id)
            .await
            .map_err(queue_error)?
        else {
            return Ok(());
        };
        let history = self
            .thread_store
            .load_history(LoadThreadHistoryParams {
                thread_id,
                include_archived: false,
            })
            .await
            .map_err(|error| {
                internal_error(format!("failed to read queue recovery history: {error}"))
            })?;
        let turn_id = active
            .turn_id
            .as_deref()
            .ok_or_else(|| internal_error("active queued submission is missing its turn id"))?;
        let disposition = match queue_recovery_outcome(
            &history.items,
            turn_id,
            &active.client_user_message_id,
            active.admission_rejection,
        ) {
            QueueRecoveryOutcome::Completed(status) => {
                Some((status, QueueTerminalDisposition::Continue))
            }
            QueueRecoveryOutcome::Aborted(reason) => Some((
                QueuedSubmissionTerminalStatus::Interrupted,
                terminal_disposition(&reason),
            )),
            QueueRecoveryOutcome::Incomplete | QueueRecoveryOutcome::NotStarted => None,
        };
        let Some((status, disposition)) = disposition else {
            tracing::info!(%thread_id, %turn_id, "retaining queued turn claim without terminal evidence");
            return Ok(());
        };
        if self
            .state_db()?
            .finish_queued_submission(thread_id, turn_id, status, disposition)
            .await
            .map_err(queue_error)?
        {
            self.notify_changed(thread_id).await;
        }
        Ok(())
    }
}

impl ThreadLifecycleContributor<Config> for ThreadQueueService {
    fn on_thread_idle<'a>(&'a self, input: ThreadIdleInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Ok(thread_id) = ThreadId::from_string(input.thread_store.level_id()) {
                self.schedule_dispatch(thread_id).await;
            }
        })
    }
}

async fn flush_terminal_rollout(thread: &CodexThread, thread_id: ThreadId) -> bool {
    if let Err(error) = thread.flush_rollout().await {
        tracing::warn!(%thread_id, %error, "failed to flush queued turn terminal event");
        false
    } else {
        true
    }
}
