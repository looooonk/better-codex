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

fn user_message(client_id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
        client_id: Some(client_id.to_string()),
        message: "queued".to_string(),
        ..Default::default()
    }))
}

fn completed_event(error: Option<ErrorEvent>) -> EventMsg {
    EventMsg::TurnComplete(TurnCompleteEvent {
        turn_id: "turn-1".to_string(),
        last_agent_message: None,
        error,
        started_at: None,
        completed_at: None,
        duration_ms: None,
        time_to_first_token_ms: None,
    })
}

fn completed(error: Option<ErrorEvent>) -> RolloutItem {
    RolloutItem::EventMsg(completed_event(error))
}

#[test]
fn recovery_distinguishes_unpersisted_inflight_and_terminal_work() {
    assert_eq!(
        queue_recovery_outcome(&[], "turn-1", "client-1", /*admission_rejection*/ None),
        QueueRecoveryOutcome::NotStarted
    );
    assert_eq!(
        queue_recovery_outcome(
            &[started(), user_message("client-1")],
            "turn-1",
            "client-1",
            /*admission_rejection*/ None,
        ),
        QueueRecoveryOutcome::Incomplete {
            input_persisted: true,
        }
    );
    assert_eq!(
        queue_recovery_outcome(
            &[
                started(),
                user_message("client-1"),
                completed(/*error*/ None),
            ],
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
        QueueRecoveryOutcome::TerminalWithoutInput
    );
    assert_eq!(
        queue_recovery_outcome(
            &[
                started(),
                user_message("different-client"),
                completed(/*error*/ None),
            ],
            "turn-1",
            "client-1",
            /*admission_rejection*/ None,
        ),
        QueueRecoveryOutcome::TerminalWithoutInput
    );
    assert_eq!(
        queue_recovery_outcome(
            &[
                started(),
                user_message("client-1"),
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
fn recovery_never_automatically_retries_an_ambiguous_claim() {
    assert_eq!(
        queue_recovery_action(QueueRecoveryOutcome::NotStarted),
        QueueRecoveryAction::Indeterminate {
            input_persisted: false,
        }
    );
    assert_eq!(
        queue_recovery_action(QueueRecoveryOutcome::Incomplete {
            input_persisted: true,
        }),
        QueueRecoveryAction::Indeterminate {
            input_persisted: true,
        }
    );
    assert_eq!(
        queue_recovery_action(QueueRecoveryOutcome::Incomplete {
            input_persisted: false,
        }),
        QueueRecoveryAction::Indeterminate {
            input_persisted: false,
        }
    );
    assert_eq!(
        queue_recovery_action(QueueRecoveryOutcome::TerminalWithoutInput),
        QueueRecoveryAction::Indeterminate {
            input_persisted: false,
        }
    );
}

#[test]
fn missing_paginated_turn_respects_durable_hook_rejection() {
    assert_eq!(
        missing_turn_recovery_outcome(/*admission_rejection*/ None),
        QueueRecoveryOutcome::NotStarted
    );
    assert_eq!(
        missing_turn_recovery_outcome(Some(QueuedSubmissionAdmissionRejection::Hook)),
        QueueRecoveryOutcome::Completed(QueuedSubmissionTerminalStatus::Failed)
    );
}

#[test]
fn terminal_recovery_continues_only_for_replace_and_review_end() {
    for (reason, disposition) in [
        (
            TurnAbortReason::Interrupted,
            QueueTerminalDisposition::Pause(ThreadQueuePauseReason::Interrupted),
        ),
        (
            TurnAbortReason::BudgetLimited,
            QueueTerminalDisposition::Pause(ThreadQueuePauseReason::BudgetLimited),
        ),
        (
            TurnAbortReason::Replaced,
            QueueTerminalDisposition::Continue,
        ),
        (
            TurnAbortReason::ReviewEnded,
            QueueTerminalDisposition::Continue,
        ),
    ] {
        assert_eq!(
            queue_recovery_action(observed_queue_recovery_outcome(
                &ThreadTerminalEvent::Aborted {
                    turn_id: "turn-1".to_string(),
                    reason: reason.clone(),
                },
                /*input_persisted*/ true,
                /*admission_rejection*/ None,
            ),),
            QueueRecoveryAction::Finish {
                status: QueuedSubmissionTerminalStatus::Interrupted,
                disposition,
            }
        );
        assert_eq!(
            queue_recovery_action(paginated_queue_recovery_outcome(
                StoredTurnStatus::Interrupted,
                Some(reason),
                /*input_persisted*/ true,
                /*admission_rejection*/ None,
            ),),
            QueueRecoveryAction::Finish {
                status: QueuedSubmissionTerminalStatus::Interrupted,
                disposition,
            }
        );
    }
}

#[test]
fn observed_and_paginated_terminal_recovery_require_persisted_input() {
    assert_eq!(
        observed_queue_recovery_outcome(
            &ThreadTerminalEvent::from_event(&completed_event(/*error*/ None))
                .expect("terminal event"),
            /*input_persisted*/ true,
            /*admission_rejection*/ None,
        ),
        QueueRecoveryOutcome::Completed(QueuedSubmissionTerminalStatus::Completed)
    );
    assert_eq!(
        observed_queue_recovery_outcome(
            &ThreadTerminalEvent::from_event(&completed_event(/*error*/ None))
                .expect("terminal event"),
            /*input_persisted*/ false,
            /*admission_rejection*/ None,
        ),
        QueueRecoveryOutcome::TerminalWithoutInput
    );
    assert_eq!(
        paginated_queue_recovery_outcome(
            StoredTurnStatus::Completed,
            /*abort_reason*/ None,
            /*input_persisted*/ false,
            /*admission_rejection*/ None,
        ),
        QueueRecoveryOutcome::TerminalWithoutInput
    );
    assert_eq!(
        paginated_queue_recovery_outcome(
            StoredTurnStatus::Interrupted,
            /*abort_reason*/ None,
            /*input_persisted*/ true,
            /*admission_rejection*/ None,
        ),
        QueueRecoveryOutcome::Aborted(TurnAbortReason::Interrupted)
    );
}

#[test]
fn terminal_without_queued_input_is_indeterminate_for_every_abort_reason() {
    for reason in [
        TurnAbortReason::Interrupted,
        TurnAbortReason::Replaced,
        TurnAbortReason::ReviewEnded,
        TurnAbortReason::BudgetLimited,
    ] {
        assert_eq!(
            queue_recovery_outcome(
                &[
                    started(),
                    RolloutItem::EventMsg(EventMsg::TurnAborted(TurnAbortedEvent {
                        turn_id: Some("turn-1".to_string()),
                        reason: reason.clone(),
                        started_at: None,
                        completed_at: None,
                        duration_ms: None,
                    })),
                ],
                "turn-1",
                "client-1",
                /*admission_rejection*/ None,
            ),
            QueueRecoveryOutcome::TerminalWithoutInput
        );
        assert_eq!(
            paginated_queue_recovery_outcome(
                StoredTurnStatus::Interrupted,
                Some(reason),
                /*input_persisted*/ false,
                /*admission_rejection*/ None,
            ),
            QueueRecoveryOutcome::TerminalWithoutInput
        );
    }
}

#[test]
fn durable_hook_rejection_can_terminalize_without_queued_input() {
    assert_eq!(
        observed_queue_recovery_outcome(
            &ThreadTerminalEvent::from_event(&completed_event(/*error*/ None))
                .expect("terminal event"),
            /*input_persisted*/ false,
            Some(QueuedSubmissionAdmissionRejection::Hook),
        ),
        QueueRecoveryOutcome::Completed(QueuedSubmissionTerminalStatus::Failed)
    );
    assert_eq!(
        paginated_queue_recovery_outcome(
            StoredTurnStatus::Interrupted,
            Some(TurnAbortReason::Interrupted),
            /*input_persisted*/ false,
            Some(QueuedSubmissionAdmissionRejection::Hook),
        ),
        QueueRecoveryOutcome::Completed(QueuedSubmissionTerminalStatus::Failed)
    );
}

#[test]
fn persisted_input_must_match_the_queued_turn_and_client_id() {
    let items = vec![
        started(),
        completed(/*error*/ None),
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-2".to_string(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: ModeKind::Default,
        })),
        user_message("client-1"),
    ];

    assert!(!queue_input_persisted(&items, "turn-1", "client-1"));
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
    assert_eq!(
        parse_cursor(Some("42")).expect("numeric cursor is valid"),
        42
    );
    assert!(parse_cursor(Some("")).is_err());
    assert!(parse_cursor(Some("not-a-number")).is_err());
    assert!(parse_cursor(Some(&"1".repeat(MAX_QUEUE_IDENTIFIER_BYTES + 1))).is_err());
}
