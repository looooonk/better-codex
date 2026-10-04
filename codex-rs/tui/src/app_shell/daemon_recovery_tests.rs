use super::*;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn shared_restart_needs_a_selection_then_explicit_confirmation() {
    let mut state = RecoveryState {
        selected: 2,
        restart_available: true,
    };
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    assert_eq!(state.key(key(KeyCode::Enter)), Some(DaemonRecovery::Cancel));
    assert_eq!(state.key(key(KeyCode::Char('2'))), None);
    assert_eq!(
        state.key(key(KeyCode::Enter)),
        Some(DaemonRecovery::Restart)
    );
    assert_eq!(
        state.key(key(KeyCode::Char('1'))),
        Some(DaemonRecovery::Independent)
    );
    state.restart_available = false;
    assert_eq!(state.key(key(KeyCode::Enter)), None);
    assert_eq!(state.key(key(KeyCode::Esc)), Some(DaemonRecovery::Cancel));
}

#[test]
fn daemon_recovery_shows_shared_effects_before_confirmation() {
    let issue = CompatibilityError {
        reason: "This session requires auth_elicitation to be disabled".to_string(),
        restart_features: Some(std::collections::BTreeMap::from([(
            "auth_elicitation".to_string(),
            false,
        )])),
    };
    let state = RecoveryState {
        selected: 2,
        restart_available: true,
    };
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 24,
    );
    let mut buffer = Buffer::empty(area);
    modal_view::render_modal(area, "Background server", state.lines(&issue), &mut buffer);
    let screen = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(screen);
}
