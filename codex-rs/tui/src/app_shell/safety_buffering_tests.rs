use super::*;
use codex_app_server_protocol::TurnItemsView;
use pretty_assertions::assert_eq;

fn turn(id: &str, status: TurnStatus) -> Turn {
    Turn {
        id: id.to_string(),
        items: Vec::new(),
        items_view: TurnItemsView::Full,
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

#[test]
fn safety_buffering_retry_modal_uses_neutral_waiting_copy() {
    let state = SafetyBufferingState {
        active: Some(ActiveSafetyBuffering {
            turn_id: "turn-1".to_string(),
            faster_model: Some("faster-model".to_string()),
            can_retry: true,
            selected: 0,
            visible: true,
        }),
        ..SafetyBufferingState::default()
    };

    let rendered = state
        .modal_lines()
        .expect("modal should be visible")
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered, @r"
Our systems are thinking a bit more about this request before responding.

Hang tight or retry with a faster model for a quicker response, though it may be less capable of handling complex requests.

> Retry with a faster model
  Dismiss and keep waiting
  Learn more

No action is required. Codex will keep waiting, and this menu will close when the response is ready.
↑↓ / j k select  Enter confirm  r retry  d / Esc dismiss
    ");
}

#[test]
fn safety_buffering_without_original_turn_omits_retry() {
    let mut state = SafetyBufferingState::default();
    let updated = state.update(
        Some("turn-1"),
        ModelSafetyBufferingUpdatedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            model: "slower-model".to_string(),
            use_cases: Vec::new(),
            reasons: Vec::new(),
            show_buffering_ui: true,
            faster_model: Some("faster-model".to_string()),
        },
    );

    assert!(updated);
    assert_eq!(
        SafetyBufferingState::actions(state.active.as_ref().expect("active buffering")),
        vec![
            SafetyBufferingAction::Dismiss,
            SafetyBufferingAction::LearnMore,
        ]
    );
}

#[test]
fn bio_policy_errors_are_recognized_without_matching_mutable_copy() {
    assert!(is_safety_access_error(
        "This content was flagged for possible biological risk."
    ));
    assert!(is_safety_access_error(
        &serde_json::json!({
            "error": {"code": "bio_policy", "message": "copy may change"}
        })
        .to_string()
    ));
    assert!(!is_safety_access_error("ordinary backend failure"));
}

#[test]
fn retry_rejects_a_stale_or_in_progress_turn() {
    let stale = vec![
        turn("turn-1", TurnStatus::Interrupted),
        turn("turn-2", TurnStatus::InProgress),
    ];
    let in_progress = vec![turn("turn-1", TurnStatus::InProgress)];
    let previous_in_progress = vec![
        turn("turn-1", TurnStatus::InProgress),
        turn("turn-2", TurnStatus::Interrupted),
    ];

    assert_eq!(
        [
            safety_retry_additional_inputs(&stale, "turn-1").unwrap_err(),
            safety_retry_additional_inputs(&in_progress, "turn-1").unwrap_err(),
            safety_retry_additional_inputs(&in_progress, "missing").unwrap_err(),
            safety_retry_additional_inputs(&previous_in_progress, "turn-2").unwrap_err(),
        ],
        [
            "interrupted turn turn-1 is no longer the latest turn",
            "interrupted turn turn-1 is still in progress",
            "interrupted turn missing is missing from the source thread",
            "previous turn turn-1 is still in progress",
        ]
        .map(str::to_string)
    );
}
