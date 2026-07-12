use super::*;
use pretty_assertions::assert_eq;

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
Up/Down select  Enter confirm  Esc dismiss
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
