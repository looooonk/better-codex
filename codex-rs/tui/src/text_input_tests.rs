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
fn selection_tracks_anchor_and_cursor_at_grapheme_boundaries() {
    let text = "a e\u{301} \u{754c} z";
    let accent_start = text.find('e').expect("accent should be present");
    let wide_end = text
        .find('\u{754c}')
        .expect("wide character should be present")
        + '\u{754c}'.len_utf8();
    let mut input = EditableText::new(text);

    input.set_selection(wide_end, accent_start);

    assert_eq!(input.selected_text(), Some("e\u{301} \u{754c}"));
    assert_eq!(
        input,
        EditableText {
            text: text.to_string(),
            cursor: accent_start,
            selection_anchor: Some(wide_end),
        }
    );

    input.set_cursor(accent_start + 1);
    let accent_end = accent_start + "e\u{301}".len();
    assert_eq!(
        input,
        EditableText {
            text: text.to_string(),
            cursor: accent_end,
            selection_anchor: None,
        }
    );
}

#[test]
fn insertion_replaces_the_entire_selection() {
    let mut character = EditableText::new("before e\u{301} after");
    let selected_start = character
        .text()
        .find("e\u{301}")
        .expect("accent should be present");
    let selected_end = selected_start + "e\u{301}".len();
    character.set_selection(selected_end, selected_start);
    character.insert_char('\u{754c}');

    let mut expected_character = EditableText::new("before \u{754c} after");
    expected_character.set_cursor(selected_start + '\u{754c}'.len_utf8());
    assert_eq!(character, expected_character);

    let mut string = EditableText::new("alpha middle omega");
    let selected_start = string
        .text()
        .find("middle")
        .expect("middle should be present");
    let selected_end = selected_start + "middle".len();
    string.set_selection(selected_start, selected_end);
    string.insert_str("first\nsecond");

    let mut expected_string = EditableText::new("alpha first\nsecond omega");
    expected_string.set_cursor(selected_start + "first\nsecond".len());
    assert_eq!(string, expected_string);
}

#[test]
fn empty_insertion_preserves_selection() {
    let mut input = EditableText::new("alpha beta");
    input.set_selection("alpha ".len(), "alpha beta".len());
    let before = input.clone();

    input.insert_str("");

    assert_eq!(input, before);
}

#[test]
fn every_deletion_action_deletes_the_selection_first() {
    for action in [
        TextInputAction::DeleteBackward,
        TextInputAction::DeleteForward,
        TextInputAction::DeleteWordLeft,
        TextInputAction::DeleteToLineStart,
    ] {
        let mut input = EditableText::new("alpha e\u{301}\n\u{754c} omega");
        let selected_start = input.text().find('e').expect("accent should be present");
        let selected_end = input
            .text()
            .find(" omega")
            .expect("suffix should be present");
        input.set_selection(selected_end, selected_start);

        assert!(input.apply(action));

        let mut expected = EditableText::new("alpha  omega");
        expected.set_cursor(selected_start);
        assert_eq!(input, expected, "unexpected result for {action:?}");
    }
}

#[test]
fn horizontal_movement_collapses_selection_to_the_corresponding_edge() {
    let mut selected = EditableText::new("alpha beta omega");
    let selection_start = selected
        .text()
        .find("beta")
        .expect("beta should be present");
    let selection_end = selection_start + "beta".len();
    selected.set_selection(selection_end, selection_start);

    let mut left = selected.clone();
    left.apply(TextInputAction::MoveLeft);
    let mut expected_left = EditableText::new("alpha beta omega");
    expected_left.set_cursor(selection_start);
    assert_eq!(left, expected_left);

    let mut right = selected;
    right.apply(TextInputAction::MoveRight);
    let mut expected_right = EditableText::new("alpha beta omega");
    expected_right.set_cursor(selection_end);
    assert_eq!(right, expected_right);
}

#[test]
fn vertical_movement_clears_selection_without_reporting_a_boundary_miss() {
    let mut up = EditableText::new("alpha beta");
    up.set_selection("alpha ".len(), "alpha beta".len());
    assert!(up.move_up());
    let mut expected_up = EditableText::new("alpha beta");
    expected_up.set_cursor("alpha beta".len());
    assert_eq!(up, expected_up);

    let mut down = EditableText::new("alpha beta");
    down.set_selection("alpha beta".len(), "alpha ".len());
    assert!(down.move_down());
    let mut expected_down = EditableText::new("alpha beta");
    expected_down.set_cursor("alpha ".len());
    assert_eq!(down, expected_down);
}

#[test]
fn set_and_clear_text_remove_selection() {
    let mut input = EditableText::new("selected text");
    input.set_selection(/*anchor*/ 0, "selected".len());

    input.set_text("replacement");
    assert_eq!(input, EditableText::new("replacement"));

    input.set_selection(/*anchor*/ 0, "replace".len());
    input.clear();
    assert_eq!(input, EditableText::default());
}

#[test]
fn display_maps_source_selection_and_expanded_tab_graphemes() {
    let mut input = EditableText::new("a\t\u{754c}\n\tz");
    let wide_end = "a\t\u{754c}".len();
    input.set_selection("a".len(), wide_end);

    let display = input.display();

    assert_eq!(display.text(), "a       \u{754c}\n        z");
    assert_eq!(display.selection_range(), Some(1..11));
    assert_eq!(
        display.source_range_for_display_range(/*display_range*/ 4..5),
        1..2
    );
    assert_eq!(
        display.source_range_for_display_range(/*display_range*/ 8..11),
        2..5
    );
    assert_eq!(
        display.source_range_for_display_range(/*display_range*/ 8..8),
        2..2
    );
}

#[test]
fn display_maps_each_tab_expansion_back_to_its_source_tab() {
    let input = EditableText::new("a\tb\tc");
    let display = input.display();

    assert_eq!(display.text(), "a       b       c");
    assert_eq!(
        display.source_range_for_display_range(/*display_range*/ 4..5),
        1..2
    );
    assert_eq!(
        display.source_range_for_display_range(/*display_range*/ 12..13),
        3..4
    );
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
