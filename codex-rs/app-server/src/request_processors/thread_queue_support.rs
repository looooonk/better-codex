use crate::error_code::internal_error;
use crate::error_code::invalid_params;
use crate::error_code::invalid_request;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::QueuedSubmission;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_core::CodexThread;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::user_input::MAX_USER_INPUT_TEXT_CHARS;
use codex_protocol::user_input::UserInput as CoreUserInput;
use codex_rollout::RolloutItem;
use codex_state::MAX_QUEUE_IDENTIFIER_BYTES;
use codex_state::QueueTerminalDisposition;
use codex_state::QueuedSubmissionAdmissionRejection;
use codex_state::QueuedSubmissionRecord;
use codex_state::QueuedSubmissionState;
use codex_state::QueuedSubmissionTerminalStatus;
use codex_state::ThreadQueueError;
use codex_state::ThreadQueuePauseReason;

use super::turn_processor::validate_user_input_image_urls;

pub(super) const QUEUE_LIST_DEFAULT_LIMIT: usize = 25;
pub(super) const QUEUE_LIST_MAX_LIMIT: usize = 100;
const DIRECT_INPUT_TO_MULTI_AGENT_V2_SUBAGENT_ERROR: &str =
    "direct app-server input is not allowed for multi-agent v2 sub-agents";
const DIRECT_INPUT_TO_UNLOADED_SUBAGENT_ERROR: &str =
    "direct app-server input is not allowed for unloaded spawned sub-agents";

pub(super) fn ensure_direct_input_allowed(
    loaded_thread: Option<&CodexThread>,
    source: &SessionSource,
) -> Result<(), JSONRPCErrorError> {
    match loaded_thread {
        Some(thread)
            if thread.multi_agent_version() == Some(codex_protocol::protocol::MultiAgentVersion::V2)
                && matches!(source, SessionSource::SubAgent(SubAgentSource::ThreadSpawn { .. })) =>
        {
            Err(invalid_request(
                DIRECT_INPUT_TO_MULTI_AGENT_V2_SUBAGENT_ERROR,
            ))
        }
        None if matches!(source, SessionSource::SubAgent(SubAgentSource::ThreadSpawn { .. })) => {
            Err(invalid_request(DIRECT_INPUT_TO_UNLOADED_SUBAGENT_ERROR))
        }
        _ => Ok(()),
    }
}

pub(super) fn prepare_payload(input: &[UserInput]) -> Result<String, JSONRPCErrorError> {
    if input.is_empty() {
        return Err(invalid_params("queued input must not be empty"));
    }
    validate_user_input_image_urls(input)?;
    if input
        .iter()
        .any(|item| matches!(item, UserInput::LocalImage { .. } | UserInput::LocalAudio { .. }))
    {
        return Err(invalid_params(
            "local media cannot be queued durably; use an inline attachment",
        ));
    }
    let text_chars = input.iter().map(UserInput::text_char_count).sum::<usize>();
    if text_chars > MAX_USER_INPUT_TEXT_CHARS {
        return Err(invalid_params(format!(
            "queued text exceeds the {MAX_USER_INPUT_TEXT_CHARS} character limit"
        )));
    }
    serde_json::to_string(
        &input
            .iter()
            .cloned()
            .map(UserInput::into_core)
            .collect::<Vec<_>>(),
    )
    .map_err(|error| internal_error(format!("failed to serialize queued input: {error}")))
}

pub(super) fn queued_core_input(
    record: &QueuedSubmissionRecord,
) -> Result<Vec<CoreUserInput>, JSONRPCErrorError> {
    serde_json::from_str(&record.payload)
        .map_err(|error| internal_error(format!("queued submission payload is invalid: {error}")))
}

pub(super) fn api_queued_submission(
    record: QueuedSubmissionRecord,
) -> Result<QueuedSubmission, JSONRPCErrorError> {
    Ok(QueuedSubmission {
        id: record.id.clone(),
        input: queued_core_input(&record)?
            .into_iter()
            .map(Into::into)
            .collect(),
        client_user_message_id: record.client_user_message_id,
    })
}

pub(super) fn parse_cursor(cursor: Option<&str>) -> Result<usize, JSONRPCErrorError> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    if cursor.is_empty() || cursor.len() > MAX_QUEUE_IDENTIFIER_BYTES {
        return Err(invalid_params("invalid queue pagination cursor"));
    }
    cursor
        .parse()
        .map_err(|error| invalid_params(format!("invalid queue pagination cursor: {error}")))
}

pub(super) fn queue_error(error: ThreadQueueError) -> JSONRPCErrorError {
    match error {
        ThreadQueueError::QueueFull
        | ThreadQueueError::InputBytesExceeded
        | ThreadQueueError::InvalidReorder
        | ThreadQueueError::InvalidIdentifier
        | ThreadQueueError::ClientMessageConflict => invalid_params(error.to_string()),
        ThreadQueueError::Storage(_) => {
            internal_error(format!("queued submission operation failed: {error}"))
        }
    }
}

pub(super) fn queue_turn(id: String, status: TurnStatus) -> Turn {
    Turn {
        id,
        items: Vec::new(),
        items_view: TurnItemsView::NotLoaded,
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

pub(super) fn turn_status(record: &QueuedSubmissionRecord) -> TurnStatus {
    match (record.state, record.terminal_status) {
        (QueuedSubmissionState::Terminal, Some(QueuedSubmissionTerminalStatus::Completed)) => {
            TurnStatus::Completed
        }
        (QueuedSubmissionState::Terminal, Some(QueuedSubmissionTerminalStatus::Failed)) => {
            TurnStatus::Failed
        }
        (QueuedSubmissionState::Terminal, Some(QueuedSubmissionTerminalStatus::Interrupted)) => {
            TurnStatus::Interrupted
        }
        _ => TurnStatus::InProgress,
    }
}

pub(super) fn terminal_disposition(reason: &TurnAbortReason) -> QueueTerminalDisposition {
    match reason {
        TurnAbortReason::Interrupted => {
            QueueTerminalDisposition::Pause(ThreadQueuePauseReason::Interrupted)
        }
        TurnAbortReason::BudgetLimited => {
            QueueTerminalDisposition::Pause(ThreadQueuePauseReason::BudgetLimited)
        }
        TurnAbortReason::Replaced | TurnAbortReason::ReviewEnded => QueueTerminalDisposition::Continue,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum QueueRecoveryOutcome {
    Completed(QueuedSubmissionTerminalStatus),
    Aborted(TurnAbortReason),
    Incomplete,
    NotStarted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QueueRecoveryAction {
    Retry,
    Finish {
        status: QueuedSubmissionTerminalStatus,
        disposition: QueueTerminalDisposition,
    },
}

pub(super) fn queue_recovery_action(
    state: QueuedSubmissionState,
    outcome: QueueRecoveryOutcome,
) -> QueueRecoveryAction {
    match outcome {
        QueueRecoveryOutcome::NotStarted if state == QueuedSubmissionState::Starting => {
            QueueRecoveryAction::Retry
        }
        QueueRecoveryOutcome::Completed(status) => QueueRecoveryAction::Finish {
            status,
            disposition: QueueTerminalDisposition::Continue,
        },
        QueueRecoveryOutcome::Aborted(reason) => QueueRecoveryAction::Finish {
            status: QueuedSubmissionTerminalStatus::Interrupted,
            disposition: terminal_disposition(&reason),
        },
        QueueRecoveryOutcome::Incomplete | QueueRecoveryOutcome::NotStarted => {
            QueueRecoveryAction::Finish {
                status: QueuedSubmissionTerminalStatus::Interrupted,
                disposition: QueueTerminalDisposition::Pause(ThreadQueuePauseReason::Interrupted),
            }
        }
    }
}

pub(super) fn queue_recovery_outcome(
    items: &[RolloutItem],
    turn_id: &str,
    client_user_message_id: &str,
    admission_rejection: Option<QueuedSubmissionAdmissionRejection>,
) -> QueueRecoveryOutcome {
    let mut started = false;
    let mut input_persisted = false;
    for item in items {
        let RolloutItem::EventMsg(event) = item else {
            continue;
        };
        match event {
            EventMsg::TurnStarted(event) if event.turn_id == turn_id => started = true,
            EventMsg::UserMessage(event)
                if started && event.client_id.as_deref() == Some(client_user_message_id) =>
            {
                input_persisted = true;
            }
            EventMsg::TurnComplete(event) if event.turn_id == turn_id => {
                let failed =
                    admission_rejection.is_some() || event.error.is_some() || !input_persisted;
                return QueueRecoveryOutcome::Completed(if failed {
                    QueuedSubmissionTerminalStatus::Failed
                } else {
                    QueuedSubmissionTerminalStatus::Completed
                });
            }
            EventMsg::TurnAborted(event) if event.turn_id.as_deref() == Some(turn_id) => {
                return QueueRecoveryOutcome::Aborted(event.reason.clone());
            }
            _ => {}
        }
    }
    if started || input_persisted {
        QueueRecoveryOutcome::Incomplete
    } else {
        QueueRecoveryOutcome::NotStarted
    }
}

#[cfg(test)]
#[path = "thread_queue_processor_tests.rs"]
mod tests;
