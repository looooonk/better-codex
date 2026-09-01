use std::time::Duration;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::TurnStatus;
use codex_core::CodexThread;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::Submission;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::protocol::W3cTraceContext;
use codex_state::BlockedSubmissionRetryPolicy;
use codex_state::QueueClaimResult;
use codex_state::QueuedSubmissionAdmissionRejection;
use uuid::Uuid;

use super::thread_queue_admission::QueueAdmissionResult;
use super::thread_queue_admission::QueueAdmissionWaitResult;
use super::thread_queue_admission::wait_for_queue_admission;
use super::thread_queue_service::ThreadQueueService;
use super::thread_queue_support::queue_error;
use super::thread_queue_support::queued_core_input;
use super::thread_queue_support::turn_status;

const QUEUE_ADMISSION_TIMEOUT: Duration = Duration::from_secs(10);

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
    pub(super) async fn start_explicit(
        &self,
        thread_id: ThreadId,
        thread: &CodexThread,
        queued_submission_id: Option<&str>,
        trace: Option<W3cTraceContext>,
    ) -> Result<QueueStartResult, QueueStartFailure> {
        if self
            .thread_state_manager
            .current_listener_command_tx(thread_id)
            .is_none()
        {
            return Err(QueueStartFailure::unchanged(invalid_request(
                "resume/subscribe the thread before starting a queued message",
            )));
        }
        let proposed_turn_id = Uuid::now_v7().to_string();
        let claim = self
            .state_db()
            .map_err(QueueStartFailure::unchanged)?
            .claim_queued_submission_and_resume(thread_id, queued_submission_id, &proposed_turn_id)
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

    pub(super) async fn start_claimed(
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
            QueueClaimResult::Blocked {
                owner_id,
                retry_policy,
            } => {
                let message = match retry_policy {
                    BlockedSubmissionRetryPolicy::Allowed => format!(
                        "queued submission {owner_id} must be retried or deleted before another queued message can start"
                    ),
                    BlockedSubmissionRetryPolicy::Forbidden => format!(
                        "queued submission {owner_id} is blocked because its input is already durable; delete it to acknowledge and discard it"
                    ),
                };
                return Err(QueueStartFailure::unchanged(invalid_request(message)));
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
            QueueStartFailure::changed(internal_error("claimed queue item is missing its turn id"))
        })?;
        let queued_input = queued_core_input(&record).map_err(QueueStartFailure::changed)?;
        let thread_state = self.thread_state_manager.thread_state(thread_id).await;
        let busy = {
            let mut thread_state = thread_state.lock().await;
            let busy = thread_state.has_pending_user_input_submission()
                || matches!(
                    thread.agent_status().await,
                    AgentStatus::Running | AgentStatus::Shutdown | AgentStatus::NotFound
                );
            if !busy {
                thread_state.mark_queued_turn_awaiting_terminal(turn_id.clone());
            }
            busy
        };
        if busy {
            let released = self
                .state_db()
                .map_err(QueueStartFailure::changed)?
                .release_queued_submission_claim(thread_id, &record.id, &turn_id)
                .await
                .map_err(queue_error)
                .map_err(QueueStartFailure::changed)?;
            let error = invalid_request("thread already has an active or pending turn");
            return Err(if released {
                QueueStartFailure::unchanged(error)
            } else {
                QueueStartFailure::changed(error)
            });
        }

        let admission_rx = self
            .admission_tracker
            .register(thread_id, &turn_id, record.client_user_message_id.clone())
            .await;
        // Keep admission on the standard core path while correlating its events to the durable claim.
        let submission = thread
            .submit_with_id(Submission {
                id: turn_id.clone(),
                op: Op::UserInput {
                    items: queued_input,
                    final_output_json_schema: None,
                    responsesapi_client_metadata: None,
                    additional_context: Default::default(),
                    thread_settings: ThreadSettingsOverrides::default(),
                },
                client_user_message_id: Some(record.client_user_message_id.clone()),
                trace,
            })
            .await;
        if let Err(error) = submission {
            self.admission_tracker.cancel(thread_id, &turn_id).await;
            thread_state
                .lock()
                .await
                .clear_queued_turn_awaiting_terminal(&turn_id);
            let admission_error =
                internal_error(format!("failed to admit queued submission: {error}"));
            let released = self
                .state_db()
                .map_err(QueueStartFailure::changed)?
                .release_queued_submission_claim(thread_id, &record.id, &turn_id)
                .await
                .map_err(queue_error)
                .map_err(QueueStartFailure::changed)?;
            return Err(if released {
                QueueStartFailure::unchanged(admission_error)
            } else {
                QueueStartFailure::changed(admission_error)
            });
        }
        let admission = wait_for_queue_admission(
            admission_rx,
            thread.wait_until_terminated(),
            QUEUE_ADMISSION_TIMEOUT,
        )
        .await;
        let admission = match admission {
            QueueAdmissionWaitResult::Admission(admission) => admission,
            QueueAdmissionWaitResult::ThreadTerminated | QueueAdmissionWaitResult::TimedOut => {
                self.admission_tracker.cancel(thread_id, &turn_id).await;
                let blocked = self
                    .state_db()
                    .map_err(QueueStartFailure::changed)?
                    .block_indeterminate_queued_submission(
                        thread_id,
                        &record.id,
                        &turn_id,
                        BlockedSubmissionRetryPolicy::Forbidden,
                    )
                    .await
                    .map_err(queue_error)
                    .map_err(QueueStartFailure::changed)?;
                thread_state
                    .lock()
                    .await
                    .clear_queued_turn_awaiting_terminal(&turn_id);
                if !blocked {
                    return Err(QueueStartFailure::changed(internal_error(format!(
                        "indeterminate queued submission {} could not be blocked",
                        record.id
                    ))));
                }
                let message = match admission {
                    QueueAdmissionWaitResult::TimedOut => {
                        "timed out waiting for queued submission admission"
                    }
                    QueueAdmissionWaitResult::ThreadTerminated => {
                        "thread terminated while queued submission admission was pending"
                    }
                    QueueAdmissionWaitResult::Admission(_) => unreachable!(),
                };
                return Err(QueueStartFailure::changed(internal_error(message)));
            }
        };
        match admission {
            QueueAdmissionResult::Persisted => {
                let state_db = self.state_db().map_err(QueueStartFailure::changed)?;
                match state_db
                    .mark_queued_submission_inflight(thread_id, &turn_id)
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        match state_db
                            .queued_submission_for_turn(thread_id, &turn_id)
                            .await
                        {
                            Ok(Some(observed)) => tracing::error!(
                                %thread_id,
                                queued_submission_id = %record.id,
                                %turn_id,
                                observed_submission_id = %observed.id,
                                observed_state = ?observed.state,
                                observed_turn_id = ?observed.turn_id,
                                "admitted queued turn did not transition from starting to inflight"
                            ),
                            Ok(None) => tracing::error!(
                                %thread_id,
                                queued_submission_id = %record.id,
                                %turn_id,
                                "admitted queued turn disappeared before its inflight transition"
                            ),
                            Err(error) => tracing::error!(
                                %thread_id,
                                queued_submission_id = %record.id,
                                %turn_id,
                                %error,
                                "admitted queued turn state could not be read after its inflight transition failed"
                            ),
                        }
                    }
                    Err(error) => tracing::error!(
                        %thread_id,
                        queued_submission_id = %record.id,
                        %turn_id,
                        %error,
                        "failed to persist admitted queued turn as inflight"
                    ),
                }
                Ok(QueueStartResult {
                    turn_id,
                    status: TurnStatus::InProgress,
                    queue_changed: true,
                })
            }
            QueueAdmissionResult::PersistedInDifferentTurn => {
                let blocked = self
                    .state_db()
                    .map_err(QueueStartFailure::changed)?
                    .block_indeterminate_queued_submission(
                        thread_id,
                        &record.id,
                        &turn_id,
                        BlockedSubmissionRetryPolicy::Forbidden,
                    )
                    .await
                    .map_err(queue_error)
                    .map_err(QueueStartFailure::changed)?;
                self.thread_state_manager
                    .thread_state(thread_id)
                    .await
                    .lock()
                    .await
                    .clear_queued_turn_awaiting_terminal(&turn_id);
                if !blocked {
                    return Err(QueueStartFailure::changed(internal_error(format!(
                        "misrouted queued submission {} could not be blocked",
                        record.id
                    ))));
                }
                Err(QueueStartFailure::changed(invalid_request(
                    "queued input was admitted into a different active turn and was blocked from retry",
                )))
            }
            QueueAdmissionResult::FailedBeforePersistence => {
                self.recover_ambiguous_start(thread_id, thread, record)
                    .await
                    .map_err(QueueStartFailure::changed)?;
                Err(QueueStartFailure::changed(invalid_request(
                    "queued submission failed before its input became durable",
                )))
            }
            QueueAdmissionResult::RejectedByError => {
                thread_state
                    .lock()
                    .await
                    .clear_queued_turn_awaiting_terminal(&turn_id);
                let released = self
                    .state_db()
                    .map_err(QueueStartFailure::changed)?
                    .release_queued_submission_claim(thread_id, &record.id, &turn_id)
                    .await
                    .map_err(queue_error)
                    .map_err(QueueStartFailure::changed)?;
                let error = invalid_request(
                    "queued submission was rejected before its input became durable",
                );
                Err(if released {
                    QueueStartFailure::unchanged(error)
                } else {
                    QueueStartFailure::changed(error)
                })
            }
            QueueAdmissionResult::RejectedByHook => {
                let state_db = self.state_db().map_err(QueueStartFailure::changed)?;
                match state_db
                    .mark_queued_submission_admission_rejected(
                        thread_id,
                        &turn_id,
                        QueuedSubmissionAdmissionRejection::Hook,
                    )
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => tracing::error!(
                        %thread_id,
                        queued_submission_id = %record.id,
                        %turn_id,
                        "rejected queued turn did not persist its admission marker"
                    ),
                    Err(error) => tracing::error!(
                        %thread_id,
                        queued_submission_id = %record.id,
                        %turn_id,
                        %error,
                        "failed to persist queued turn admission rejection"
                    ),
                }
                Err(QueueStartFailure::changed(invalid_request(
                    "queued submission was rejected by a hook",
                )))
            }
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
