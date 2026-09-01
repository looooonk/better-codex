use super::*;
use codex_protocol::config_types::ModeKind;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::UserMessageEvent;
use codex_utils_string::approx_bytes_for_tokens;
use codex_utils_string::approx_token_count;
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
fn payload_validation_caps_aggregate_queued_text_tokens() {
    let empty_payload = prepare_payload(&[
        UserInput::Text {
            text: String::new(),
            text_elements: Vec::new(),
        },
        UserInput::Text {
            text: String::new(),
            text_elements: Vec::new(),
        },
    ])
    .expect("empty text blocks fit");
    let available_text_bytes =
        approx_bytes_for_tokens(MAX_QUEUED_INPUT_PAYLOAD_TOKENS) - empty_payload.len();
    let first = "x".repeat(available_text_bytes / 2);
    let second = "y".repeat(available_text_bytes - first.len());
    let payload = prepare_payload(&[
        UserInput::Text {
            text: first.clone(),
            text_elements: Vec::new(),
        },
        UserInput::Text {
            text: second.clone(),
            text_elements: Vec::new(),
        },
    ])
    .expect("aggregate text at the payload token limit fits");
    assert_eq!(
        approx_token_count(&payload),
        MAX_QUEUED_INPUT_PAYLOAD_TOKENS
    );

    assert!(
        prepare_payload(&[
            UserInput::Text {
                text: first,
                text_elements: Vec::new(),
            },
            UserInput::Text {
                text: format!("{second}x"),
                text_elements: Vec::new(),
            },
        ])
        .is_err()
    );
}

#[test]
fn payload_validation_rejects_single_oversized_multibyte_text_item() {
    let oversized = "\u{1f642}".repeat(MAX_QUEUED_INPUT_PAYLOAD_TOKENS + 1);
    assert!(approx_token_count(&oversized) > MAX_QUEUED_INPUT_PAYLOAD_TOKENS);
    assert!(
        prepare_payload(&[UserInput::Text {
            text: oversized,
            text_elements: Vec::new(),
        }])
        .is_err()
    );
}

#[test]
fn payload_validation_caps_inline_image_data_urls() {
    const DATA_URL_PREFIX: &str = "data:image/png;base64,";

    let empty_payload = prepare_payload(&[UserInput::Image {
        url: DATA_URL_PREFIX.to_string(),
        detail: None,
    }])
    .expect("empty inline image fits");
    let available_data_bytes =
        approx_bytes_for_tokens(MAX_QUEUED_INPUT_PAYLOAD_TOKENS) - empty_payload.len();
    let encoded_len = available_data_bytes - available_data_bytes % 4;
    let encoded = "A".repeat(encoded_len);
    let accepted_url = format!("{DATA_URL_PREFIX}{encoded}");
    let payload = prepare_payload(&[UserInput::Image {
        url: accepted_url.clone(),
        detail: None,
    }])
    .expect("inline image at the payload token limit fits");
    assert_eq!(
        approx_token_count(&payload),
        MAX_QUEUED_INPUT_PAYLOAD_TOKENS
    );

    assert!(
        prepare_payload(&[UserInput::Image {
            url: format!("{accepted_url}AAAA"),
            detail: None,
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
