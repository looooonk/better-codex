use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use codex_core::CodexThread;
use codex_protocol::ThreadId;
use codex_protocol::protocol::EventMsg;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::time::sleep;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QueueAdmissionResult {
    Persisted,
    PersistedInDifferentTurn,
    RejectedByHook,
    RejectedByError,
    FailedBeforePersistence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QueueAdmissionWaitResult {
    Admission(QueueAdmissionResult),
    ThreadTerminated,
    TimedOut,
}

struct PendingQueueAdmission {
    client_user_message_id: String,
    sender: oneshot::Sender<QueueAdmissionResult>,
}

#[derive(Clone, Default)]
pub(super) struct QueueAdmissionTracker {
    pending: Arc<Mutex<HashMap<(ThreadId, String), PendingQueueAdmission>>>,
}

impl QueueAdmissionTracker {
    pub(super) async fn register(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        client_user_message_id: String,
    ) -> oneshot::Receiver<QueueAdmissionResult> {
        let (sender, receiver) = oneshot::channel();
        let key = (thread_id, turn_id.to_string());
        let mut pending = self.pending.lock().await;
        if pending.contains_key(&key) {
            tracing::error!(
                %thread_id,
                %turn_id,
                "queued admission was already registered"
            );
            return receiver;
        }
        pending.insert(
            key,
            PendingQueueAdmission {
                client_user_message_id,
                sender,
            },
        );
        receiver
    }

    pub(super) async fn cancel(&self, thread_id: ThreadId, turn_id: &str) {
        self.pending
            .lock()
            .await
            .remove(&(thread_id, turn_id.to_string()));
    }

    pub(super) async fn observe(
        &self,
        thread: &CodexThread,
        thread_id: ThreadId,
        event_turn_id: &str,
        event: &EventMsg,
    ) {
        let observed_key = (thread_id, event_turn_id.to_string());
        let (key, result) = {
            let pending = self.pending.lock().await;
            if let Some(admission) = pending.get(&observed_key) {
                let result =
                    admission_result_for_event(admission.client_user_message_id.as_str(), event);
                (observed_key, result)
            } else if let EventMsg::UserMessage(event) = event
                && let Some(client_id) = event.client_id.as_deref()
                && let Some(key) = pending.iter().find_map(|(key, admission)| {
                    (key.0 == thread_id && admission.client_user_message_id == client_id)
                        .then(|| key.clone())
                })
            {
                tracing::error!(
                    %thread_id,
                    expected_turn_id = %key.1,
                    actual_turn_id = %event_turn_id,
                    %client_id,
                    "queued input was admitted into a different active turn"
                );
                (key, Some(QueueAdmissionResult::PersistedInDifferentTurn))
            } else {
                return;
            }
        };
        let Some(mut result) = result else {
            return;
        };
        if matches!(
            result,
            QueueAdmissionResult::Persisted | QueueAdmissionResult::PersistedInDifferentTurn
        ) && let Err(error) = thread.flush_rollout().await
        {
            tracing::warn!(
                %thread_id,
                %event_turn_id,
                %error,
                "failed to flush queued input before admission acknowledgement"
            );
            if result == QueueAdmissionResult::Persisted {
                result = QueueAdmissionResult::FailedBeforePersistence;
            }
        }
        if let Some(admission) = self.pending.lock().await.remove(&key) {
            let _ = admission.sender.send(result);
        }
    }
}

fn admission_result_for_event(
    client_user_message_id: &str,
    event: &EventMsg,
) -> Option<QueueAdmissionResult> {
    match event {
        EventMsg::UserMessage(event)
            if event.client_id.as_deref() == Some(client_user_message_id) =>
        {
            Some(QueueAdmissionResult::Persisted)
        }
        EventMsg::TurnComplete(_) => Some(QueueAdmissionResult::RejectedByHook),
        EventMsg::Error(_) => Some(QueueAdmissionResult::RejectedByError),
        EventMsg::TurnAborted(_) => Some(QueueAdmissionResult::FailedBeforePersistence),
        _ => None,
    }
}

pub(super) async fn wait_for_queue_admission(
    admission_rx: oneshot::Receiver<QueueAdmissionResult>,
    thread_terminated: impl Future<Output = ()>,
    admission_timeout: Duration,
) -> QueueAdmissionWaitResult {
    tokio::pin!(thread_terminated);
    tokio::select! {
        admission = admission_rx => QueueAdmissionWaitResult::Admission(
            admission.unwrap_or(QueueAdmissionResult::FailedBeforePersistence),
        ),
        () = &mut thread_terminated => QueueAdmissionWaitResult::ThreadTerminated,
        () = sleep(admission_timeout) => QueueAdmissionWaitResult::TimedOut,
    }
}

#[cfg(test)]
#[path = "thread_queue_admission_tests.rs"]
mod tests;
