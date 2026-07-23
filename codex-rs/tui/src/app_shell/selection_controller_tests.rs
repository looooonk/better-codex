use super::*;
use pretty_assertions::assert_eq;

#[test]
fn copy_shortcut_supports_control_and_command() {
    for modifiers in [KeyModifiers::CONTROL, KeyModifiers::SUPER] {
        assert!(is_text_copy_shortcut(KeyEvent::new(
            KeyCode::Char('c'),
            modifiers,
        )));
    }
    assert!(!is_text_copy_shortcut(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::NONE,
    )));
}

#[test]
fn copies_exact_message_selection_and_retains_it() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.set_text("alpha\tbeta");
    shell.composer.set_selection(/*anchor*/ 5, /*cursor*/ 10);
    let mut copied = None;

    assert!(shell.copy_text_selection_with(|text| {
        copied = Some(text.to_string());
        Ok(Some(ClipboardLease::test()))
    }));

    assert_eq!(copied, Some("\tbeta".to_string()));
    assert_eq!(shell.composer.selected_text(), Some("\tbeta"));
    assert!(shell.clipboard_lease.is_some());
    assert_eq!(
        shell.transcript.back().map(|line| line.text.as_str()),
        Some("copied selected text")
    );
}

#[test]
fn selected_text_copy_shortcut_takes_priority_during_an_active_turn() {
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = Some("turn-active".to_string());
    shell.composer.set_text("copy me");
    shell.composer.set_selection(/*anchor*/ 0, /*cursor*/ 4);
    let mut copied = None;

    assert!(shell.handle_text_copy_shortcut_with(
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        |text| {
            copied = Some(text.to_string());
            Ok(None)
        },
    ));

    assert_eq!(copied, Some("copy".to_string()));
    assert_eq!(shell.active_turn_id.as_deref(), Some("turn-active"));
    assert!(!shell.exit_confirmation_pending);
}

#[test]
fn command_copy_without_selection_is_consumed_but_control_copy_is_not() {
    let mut shell = ShellState::snapshot_fixture();

    assert!(shell.handle_text_copy_shortcut_with(
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::SUPER),
        |_: &str| -> Result<Option<ClipboardLease>, String> {
            panic!("clipboard backend should not be called")
        },
    ));
    assert!(!shell.handle_text_copy_shortcut_with(
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        |_: &str| -> Result<Option<ClipboardLease>, String> {
            panic!("clipboard backend should not be called")
        },
    ));
}

#[test]
fn transcript_selection_is_used_when_message_has_no_selection() {
    let mut shell = ShellState::snapshot_fixture();
    let anchor = VisualGraphemeHit::new(/*row*/ 2, /*column*/ 4, /*width*/ 1);
    let focus = VisualGraphemeHit::new(/*row*/ 3, /*column*/ 2, /*width*/ 1);
    shell.text_selection.transcript = Some(TranscriptTextSelection {
        range: NormalizedVisualRange::from_hits(anchor, focus),
        text: "selected\nconversation".to_string(),
    });
    let mut copied = None;

    assert!(shell.copy_text_selection_with(|text| {
        copied = Some(text.to_string());
        Ok(None)
    }));

    assert_eq!(copied, Some("selected\nconversation".to_string()));
    assert!(shell.transcript_text_selection().is_some());
}

#[test]
fn copy_failure_is_handled_without_clearing_selection() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.set_text("selected");
    shell.composer.set_selection(/*anchor*/ 0, /*cursor*/ 8);

    assert!(shell.copy_text_selection_with(|_| Err("clipboard offline".to_string())));

    assert_eq!(shell.composer.selected_text(), Some("selected"));
    assert_eq!(
        shell.transcript.back().map(|line| line.text.as_str()),
        Some("Copy failed: clipboard offline")
    );
}

#[test]
fn no_selection_does_not_invoke_clipboard_backend() {
    let mut shell = ShellState::snapshot_fixture();

    assert!(
        !shell.copy_text_selection_with(|_| { panic!("clipboard backend should not be called") })
    );
}
