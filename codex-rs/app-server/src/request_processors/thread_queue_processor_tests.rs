use super::*;
use codex_protocol::config_types::ModeKind;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::UserMessageEvent;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn started() -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: "turn-1".to_string(),
        trace_id: None,
        started_at: None,
        model_context_window: None,
        collaboration_mode_kind: ModeKind::Default,
    }))
}

fn user_message() -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
        client_id: Some("client-1".to_string()),
        message: "queued".to_string(),
        ..Default::default()
    }))
}

fn completed(error: Option<ErrorEvent>) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
        turn_id: "turn-1".to_string(),
        last_agent_message: None,
        error,
        started_at: None,
        completed_at: None,
        duration_ms: None,
        time_to_first_token_ms: None,
    }))
}

#[test]
fn recovery_distinguishes_unpersisted_inflight_and_terminal_work() {
    assert_eq!(
        queue_recovery_outcome(&[], "turn-1", "client-1", /*admission_rejection*/ None),
        QueueRecoveryOutcome::NotStarted
    );
    assert_eq!(
        queue_recovery_outcome(
            &[started(), user_message()],
            "turn-1",
            "client-1",
            /*admission_rejection*/ None,
        ),
        QueueRecoveryOutcome::Incomplete
    );
    assert_eq!(
        queue_recovery_outcome(
            &[started(), user_message(), completed(/*error*/ None)],
            "turn-1",
            "client-1",
            /*admission_rejection*/ None,
        ),
        QueueRecoveryOutcome::Completed(QueuedSubmissionTerminalStatus::Completed)
    );
    assert_eq!(
        queue_recovery_outcome(
            &[started(), completed(/*error*/ None)],
            "turn-1",
            "client-1",
            Some(QueuedSubmissionAdmissionRejection::Hook),
        ),
        QueueRecoveryOutcome::Completed(QueuedSubmissionTerminalStatus::Failed)
    );
    assert_eq!(
        queue_recovery_outcome(
            &[started(), completed(/*error*/ None)],
            "turn-1",
            "client-1",
            /*admission_rejection*/ None,
        ),
        QueueRecoveryOutcome::Completed(QueuedSubmissionTerminalStatus::Failed)
    );
    assert_eq!(
        queue_recovery_outcome(
            &[
                started(),
                user_message(),
                RolloutItem::EventMsg(EventMsg::TurnAborted(TurnAbortedEvent {
                    turn_id: Some("turn-1".to_string()),
                    reason: TurnAbortReason::Interrupted,
                    started_at: None,
                    completed_at: None,
                    duration_ms: None,
                })),
            ],
            "turn-1",
            "client-1",
            /*admission_rejection*/ None,
        ),
        QueueRecoveryOutcome::Aborted(TurnAbortReason::Interrupted)
    );
}

#[test]
fn recovery_retries_only_definitely_unstarted_claims() {
    assert_eq!(
        queue_recovery_action(
            QueuedSubmissionState::Starting,
            QueueRecoveryOutcome::NotStarted,
        ),
        QueueRecoveryAction::Retry
    );
    let interrupted = QueueRecoveryAction::Finish {
        status: QueuedSubmissionTerminalStatus::Interrupted,
        disposition: QueueTerminalDisposition::Pause(ThreadQueuePauseReason::Interrupted),
    };
    assert_eq!(
        queue_recovery_action(
            QueuedSubmissionState::Starting,
            QueueRecoveryOutcome::Incomplete,
        ),
        interrupted
    );
    assert_eq!(
        queue_recovery_action(
            QueuedSubmissionState::Inflight,
            QueueRecoveryOutcome::NotStarted,
        ),
        interrupted
    );
}

#[test]
fn payload_validation_rejects_nondurable_or_unbounded_input() {
    assert!(prepare_payload(&[]).is_err());
    assert!(
        prepare_payload(&[UserInput::LocalImage {
            path: PathBuf::from("image.png"),
            detail: None,
        }])
        .is_err()
    );
    assert!(
        prepare_payload(&[UserInput::Text {
            text: "x".repeat(MAX_USER_INPUT_TEXT_CHARS + 1),
            text_elements: Vec::new(),
        }])
        .is_err()
    );
}

#[test]
fn pagination_cursor_is_bounded_and_numeric() {
    assert_eq!(parse_cursor(None).expect("missing cursor is valid"), 0);
    assert_eq!(parse_cursor(Some("42")).expect("numeric cursor is valid"), 42);
    assert!(parse_cursor(Some("")).is_err());
    assert!(parse_cursor(Some("not-a-number")).is_err());
    assert!(parse_cursor(Some(&"1".repeat(MAX_QUEUE_IDENTIFIER_BYTES + 1))).is_err());
}
