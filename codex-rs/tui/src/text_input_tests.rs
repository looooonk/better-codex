use super::*;
use crossterm::event::KeyEventState;
use crossterm::event::ModifierKeyCode;
use pretty_assertions::assert_eq;

fn action(code: KeyCode, modifiers: KeyModifiers) -> Option<TextInputAction> {
    text_input_action_from_key(KeyEvent::new(code, modifiers))
}

#[test]
fn macos_shortcuts_map_to_text_actions() {
    let cases = [
        (
            KeyCode::Left,
            KeyModifiers::SUPER,
            TextInputAction::MoveLineStart,
        ),
        (
            KeyCode::Right,
            KeyModifiers::SUPER,
            TextInputAction::MoveLineEnd,
        ),
        (
            KeyCode::Backspace,
            KeyModifiers::SUPER,
            TextInputAction::DeleteToLineStart,
        ),
        (
            KeyCode::Left,
            KeyModifiers::ALT,
            TextInputAction::MoveWordLeft,
        ),
        (
            KeyCode::Right,
            KeyModifiers::ALT,
            TextInputAction::MoveWordRight,
        ),
        (
            KeyCode::Backspace,
            KeyModifiers::ALT,
            TextInputAction::DeleteWordLeft,
        ),
        (
            KeyCode::Left,
            KeyModifiers::CONTROL,
            TextInputAction::MoveWordLeft,
        ),
        (
            KeyCode::Right,
            KeyModifiers::CONTROL,
            TextInputAction::MoveWordRight,
        ),
        (
            KeyCode::Backspace,
            KeyModifiers::CONTROL,
            TextInputAction::DeleteWordLeft,
        ),
    ];

    for (code, modifiers, expected) in cases {
        assert_eq!(action(code, modifiers), Some(expected));
    }
}

#[test]
fn compatibility_encodings_map_to_the_same_actions() {
    for (code, modifiers, expected) in [
        (
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
            TextInputAction::MoveLineStart,
        ),
        (
            KeyCode::Char('e'),
            KeyModifiers::CONTROL,
            TextInputAction::MoveLineEnd,
        ),
        (
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
            TextInputAction::DeleteToLineStart,
        ),
        (
            KeyCode::Char('\u{0001}'),
            KeyModifiers::NONE,
            TextInputAction::MoveLineStart,
        ),
        (
            KeyCode::Char('\u{0005}'),
            KeyModifiers::NONE,
            TextInputAction::MoveLineEnd,
        ),
        (
            KeyCode::Char('\u{0015}'),
            KeyModifiers::NONE,
            TextInputAction::DeleteToLineStart,
        ),
    ] {
        assert_eq!(action(code, modifiers), Some(expected));
    }
    assert_eq!(
        action(KeyCode::Char('b'), KeyModifiers::ALT),
        Some(TextInputAction::MoveWordLeft)
    );
    assert_eq!(
        action(KeyCode::Char('f'), KeyModifiers::ALT),
        Some(TextInputAction::MoveWordRight)
    );
    for modifiers in [KeyModifiers::ALT, KeyModifiers::CONTROL] {
        assert_eq!(
            action(KeyCode::Char('\u{007f}'), modifiers),
            Some(TextInputAction::DeleteWordLeft)
        );
    }
    assert_eq!(
        action(KeyCode::Char('\u{007f}'), KeyModifiers::SUPER),
        Some(TextInputAction::DeleteToLineStart)
    );
}

#[test]
fn shortcut_matching_is_exact_and_ignores_modifier_only_events() {
    assert_eq!(
        action(KeyCode::Left, KeyModifiers::SUPER | KeyModifiers::SHIFT),
        None
    );
    assert_eq!(action(KeyCode::Backspace, KeyModifiers::META), None);
    assert_eq!(
        action(
            KeyCode::Modifier(ModifierKeyCode::LeftSuper),
            KeyModifiers::SUPER
        ),
        None
    );

    let release = KeyEvent {
        code: KeyCode::Left,
        modifiers: KeyModifiers::SUPER,
        kind: KeyEventKind::Release,
        state: KeyEventState::NONE,
    };
    assert_eq!(text_input_action_from_key(release), None);
}

#[test]
fn plain_navigation_and_forward_delete_are_available() {
    let mut input = EditableText::new("alpha beta");
    input.apply(TextInputAction::MoveWordLeft);

    for (code, expected) in [
        (KeyCode::Home, TextInputAction::MoveLineStart),
        (KeyCode::End, TextInputAction::MoveLineEnd),
        (KeyCode::Delete, TextInputAction::DeleteForward),
    ] {
        assert_eq!(action(code, KeyModifiers::NONE), Some(expected));
    }

    input.apply(TextInputAction::DeleteForward);
    assert_eq!(input.text(), "alpha eta");
}

#[test]
fn line_shortcuts_use_logical_line_boundaries() {
    let mut input = EditableText::new("alpha\nbeta gamma\nomega");
    for _ in 0..6 {
        input.apply(TextInputAction::MoveLeft);
    }
    assert_eq!(input.cursor(), 16);

    input.apply(TextInputAction::MoveLineStart);
    assert_eq!(input.cursor(), 6);
    input.apply(TextInputAction::MoveLineEnd);
    assert_eq!(input.cursor(), 16);
    assert!(input.apply(TextInputAction::DeleteToLineStart));
    assert_eq!(input.text(), "alpha\n\nomega");
    assert_eq!(input.cursor(), 6);
    assert!(!input.apply(TextInputAction::DeleteToLineStart));
}

#[test]
fn word_shortcuts_share_unicode_and_separator_semantics() {
    let mut input = EditableText::new("naive beta_gamma, \u{4e16}\u{754c} tail");
    for _ in 0..5 {
        input.apply(TextInputAction::MoveLeft);
    }
    input.apply(TextInputAction::MoveWordLeft);
    assert_eq!(&input.text()[input.cursor()..], "\u{4e16}\u{754c} tail");
    assert!(input.apply(TextInputAction::DeleteWordLeft));
    assert_eq!(input.text(), "naive \u{4e16}\u{754c} tail");
    input.apply(TextInputAction::MoveWordRight);
    assert_eq!(&input.text()[input.cursor()..], " tail");
}

#[test]
fn software_cursor_tracks_utf8_byte_boundaries() {
    let mut input = EditableText::new("alpha \u{4e16}\u{754c}");
    input.apply(TextInputAction::MoveWordLeft);

    assert_eq!(input.text_with_cursor(), "alpha ▏\u{4e16}\u{754c}");
}

#[test]
fn editing_preserves_extended_grapheme_clusters() {
    let mut accent = EditableText::new("Ae\u{301}B");
    accent.apply(TextInputAction::MoveLeft);
    assert!(accent.apply(TextInputAction::DeleteBackward));
    assert_eq!(accent.text_with_cursor(), "A▏B");

    let mut family =
        EditableText::new("a\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}b");
    family.apply(TextInputAction::MoveLeft);
    assert!(family.apply(TextInputAction::DeleteBackward));
    assert_eq!(family.text_with_cursor(), "a▏b");

    let mut word = EditableText::new("e\u{301} word");
    word.apply(TextInputAction::MoveLineStart);
    word.apply(TextInputAction::MoveWordRight);
    assert_eq!(word.text_with_cursor(), "e\u{301}▏ word");

    let mut inserted = EditableText::new("\u{1f1e7}");
    inserted.apply(TextInputAction::MoveLineStart);
    inserted.insert_char('\u{1f1e6}');
    assert_eq!(inserted.text_with_cursor(), "\u{1f1e6}\u{1f1e7}▏");
    assert!(inserted.apply(TextInputAction::DeleteBackward));
    assert_eq!(inserted, EditableText::default());
}

#[test]
fn word_shortcuts_follow_unicode_word_boundaries() {
    let mut input = EditableText::new("can't 3.14");
    input.apply(TextInputAction::MoveWordLeft);
    assert_eq!(input.text_with_cursor(), "can't ▏3.14");
    assert!(input.apply(TextInputAction::DeleteWordLeft));
    assert_eq!(input.text_with_cursor(), "▏3.14");
    input.apply(TextInputAction::MoveWordRight);
    assert_eq!(input.text_with_cursor(), "3.14▏");
}

#[test]
fn vertical_movement_preserves_terminal_column() {
    let mut input = EditableText::new("ab\n\u{754c}c");
    input.apply(TextInputAction::MoveLineStart);
    assert!(input.move_up());
    input.apply(TextInputAction::MoveRight);
    input.apply(TextInputAction::MoveRight);
    assert!(input.move_down());
    assert_eq!(input.text_with_cursor(), "ab\n\u{754c}▏c");
    assert!(input.move_up());
    assert_eq!(input.text_with_cursor(), "ab▏\n\u{754c}c");
}

#[test]
fn cursor_window_keeps_long_unicode_input_visible() {
    let mut input = EditableText::new("ab\u{4e16}\u{754c}cd");
    assert_eq!(
        input.text_with_cursor_window(/*max_width*/ 6),
        "…\u{754c}cd▏"
    );

    input.apply(TextInputAction::MoveLeft);
    input.apply(TextInputAction::MoveLeft);
    assert_eq!(
        input.text_with_cursor_window(/*max_width*/ 6),
        "…\u{754c}▏cd"
    );
}

#[test]
fn cursor_window_expands_tabs_without_changing_input() {
    let mut input = EditableText::new("a\tb");

    assert_eq!(
        (
            input.text(),
            input.text_with_cursor_window(/*max_width*/ 12)
        ),
        ("a\tb", "a       b▏".to_string())
    );

    input.apply(TextInputAction::MoveLeft);
    assert_eq!(
        (
            input.text(),
            input.text_with_cursor_window(/*max_width*/ 12)
        ),
        ("a\tb", "a       ▏b".to_string())
    );

    let emoji = EditableText::new("👩‍💻\tb");
    assert_eq!(
        (
            emoji.text(),
            emoji.text_with_cursor_window(/*max_width*/ 12)
        ),
        ("👩‍💻\tb", "👩‍💻      b▏".to_string())
    );
}

#[test]
fn masked_cursor_window_hides_content_and_keeps_cursor_visible() {
    let mut input = EditableText::new("secret-token-value");
    input.apply(TextInputAction::MoveWordLeft);

    let masked = input.masked_text_with_cursor_window(/*max_width*/ 8);

    assert_eq!(masked, "…***▏**…");
    assert!(!masked.contains("secret"));
}
