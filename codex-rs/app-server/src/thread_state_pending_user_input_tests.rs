use codex_protocol::config_types::ModeKind;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;

use super::*;

#[test]
fn markers_close_on_acknowledgement_and_terminal_events() {
    let mut pending = PendingUserInputSubmissions::default();
    pending.mark("turn-1".to_string());
    pending.mark("turn-2".to_string());

    pending.observe(
        "turn-1",
        &EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".to_string(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: ModeKind::Default,
        }),
    );
    assert!(pending.is_pending());

    pending.observe(
        "turn-2",
        &EventMsg::Error(ErrorEvent {
            message: "rejected".to_string(),
            codex_error_info: None,
        }),
    );
    assert!(!pending.is_pending());

    pending.mark("turn-3".to_string());
    pending.observe(
        "turn-other",
        &EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-other".to_string(),
            last_agent_message: None,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    );
    assert!(!pending.is_pending());
}

#[test]
fn clear_removes_all_markers() {
    let mut pending = PendingUserInputSubmissions::default();
    pending.mark("turn-1".to_string());

    pending.clear();

    assert!(!pending.is_pending());
}
