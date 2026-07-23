use super::*;
use pretty_assertions::assert_eq;

#[test]
fn queued_messages_are_edited_in_place_while_traversing() {
    let mut composer = ComposerState::default();
    for message in ["first", "second", "third"] {
        composer.set_text(message);
        assert!(composer.queue_current_message());
    }
    composer.set_text("ordinary draft");

    assert!(composer.edit_previous_queued_message());
    assert_eq!(composer.text(), "third");
    composer.insert_str(" updated");
    assert!(composer.edit_previous_queued_message());
    assert_eq!(composer.text(), "second");
    composer.insert_str(" updated");
    assert!(composer.edit_next_queued_message());
    assert_eq!(composer.text(), "third updated");
    assert!(composer.edit_next_queued_message());

    assert_eq!(
        composer.queued,
        VecDeque::from([
            "first".to_string(),
            "second updated".to_string(),
            "third updated".to_string(),
        ])
    );
    assert_eq!(composer.text(), "ordinary draft");
    assert_eq!(composer.queued_edit_position(), None);
}

#[test]
fn queueing_an_edited_message_preserves_its_slot_and_restores_the_draft() {
    let mut composer = ComposerState::default();
    for message in ["first", "second"] {
        composer.set_text(message);
        assert!(composer.queue_current_message());
    }
    composer.set_text("ordinary draft");
    assert!(composer.edit_previous_queued_message());
    composer.set_text("edited second");

    assert!(composer.queue_current_message());

    assert_eq!(
        composer.queued,
        VecDeque::from(["first".to_string(), "edited second".to_string()])
    );
    assert_eq!(composer.text(), "ordinary draft");
}

#[test]
fn queue_editing_restores_the_draft_cursor() {
    let mut composer = ComposerState::default();
    composer.set_text("queued");
    assert!(composer.queue_current_message());
    composer.set_text("draft tail");
    for _ in 0..5 {
        composer.move_left();
    }

    assert!(composer.edit_previous_queued_message());
    assert!(composer.finish_queued_message_edit());
    composer.insert_char('!');

    assert_eq!(composer.text(), "draft! tail");
}

#[test]
fn submission_queue_and_history_preserve_boundary_whitespace() {
    let mut composer = ComposerState::default();
    let message = "  indented content\n\n";
    composer.set_text(message);

    assert_eq!(composer.submission_text(), message);
    assert!(composer.queue_current_message());
    assert_eq!(
        composer.prepare_next_queued_message().as_deref(),
        Some(message)
    );
    composer.confirm_next_queued_message(message);
    composer.set_text("draft");
    composer.move_up_or_recall_history();

    assert_eq!(composer.text(), message);
    assert_eq!(composer.queued_count(), 0);
}

#[test]
fn clearing_a_queued_edit_removes_it_and_restores_the_draft() {
    let mut composer = ComposerState::default();
    for message in ["first", "second"] {
        composer.set_text(message);
        assert!(composer.queue_current_message());
    }
    composer.set_text("ordinary draft");
    assert!(composer.edit_previous_queued_message());
    composer.clear();

    assert!(composer.finish_queued_message_edit());

    assert_eq!(composer.queued, VecDeque::from(["first".to_string()]));
    assert_eq!(composer.text(), "ordinary draft");
    assert_eq!(composer.queued_edit_position(), None);
}

#[test]
fn deleting_queued_edits_while_traversing_keeps_adjacent_messages() {
    let mut composer = ComposerState::default();
    for message in ["first", "second", "third"] {
        composer.set_text(message);
        assert!(composer.queue_current_message());
    }
    composer.set_text("ordinary draft");
    assert!(composer.edit_previous_queued_message());
    assert!(composer.edit_previous_queued_message());
    composer.clear();

    assert!(composer.edit_next_queued_message());
    assert_eq!(composer.text(), "third");
    assert_eq!(
        composer.queued,
        VecDeque::from(["first".to_string(), "third".to_string()])
    );

    composer.clear();
    assert!(composer.edit_previous_queued_message());
    assert_eq!(composer.text(), "first");
    assert_eq!(composer.queued, VecDeque::from(["first".to_string()]));

    composer.clear();
    assert!(composer.edit_next_queued_message());
    assert_eq!(composer.text(), "ordinary draft");
    assert_eq!(composer.queued, VecDeque::new());
    assert_eq!(composer.queued_edit_position(), None);
}

#[test]
fn oversized_paste_is_rejected_before_normalization() {
    let mut composer = ComposerState::default();
    composer.set_text("draft");
    let pasted = "x".repeat(MAX_COMPOSER_BYTES);

    let result = composer.insert_str(&pasted);

    assert_eq!(
        result,
        ComposerInsertResult::TooLarge {
            attempted_bytes: MAX_COMPOSER_BYTES + "draft".len(),
        }
    );
    assert_eq!(composer.text(), "draft");
}

#[test]
fn paste_at_size_limit_is_inserted() {
    let mut composer = ComposerState::default();
    let pasted = "x".repeat(MAX_COMPOSER_BYTES);

    let result = composer.insert_str(&pasted);

    assert_eq!(result, ComposerInsertResult::Inserted);
    assert_eq!(composer.text(), pasted);
}

#[test]
fn composer_exposes_exact_source_selection_and_display_range_mapping() {
    let mut composer = ComposerState::default();
    composer.set_text("a\tb");

    composer.set_selection_from_display_ranges(/*anchor*/ 4..5, /*cursor*/ 8..9);

    assert_eq!(composer.selected_text(), Some("\tb"));
    assert_eq!(composer.selection_range(), Some(1..3));
    assert_eq!(composer.cursor(), 3);
    assert_eq!(composer.display().selection_range(), Some(1..9));

    composer.set_selection_from_display_ranges(/*anchor*/ 8..9, /*cursor*/ 4..5);
    assert_eq!(composer.selected_text(), Some("\tb"));
    assert_eq!(composer.selection_range(), Some(1..3));
    assert_eq!(composer.cursor(), 1);

    composer.set_cursor_from_display_range(/*display_range*/ 4..5);
    assert_eq!(composer.cursor(), 1);
    assert_eq!(composer.selection_range(), None);
}

#[test]
fn composer_insertions_replace_selection_and_preserve_exact_cursor() {
    let mut character = ComposerState::default();
    character.set_text("alpha middle omega");
    let selected_start = character
        .text()
        .find("middle")
        .expect("middle should be present");
    let selected_end = selected_start + "middle".len();
    character.set_selection(selected_end, selected_start);

    assert_eq!(
        character.insert_char('\u{754c}'),
        ComposerInsertResult::Inserted
    );

    let mut expected_character = ComposerState::default();
    expected_character.set_text("alpha \u{754c} omega");
    expected_character.set_cursor(selected_start + '\u{754c}'.len_utf8());
    assert_eq!(character, expected_character);

    let mut newline = ComposerState::default();
    newline.set_text("first selected last");
    let selected_start = newline
        .text()
        .find("selected")
        .expect("selected should be present");
    newline.set_selection(selected_start, selected_start + "selected".len());

    assert_eq!(newline.insert_newline(), ComposerInsertResult::Inserted);

    let mut expected_newline = ComposerState::default();
    expected_newline.set_text("first \n last");
    expected_newline.set_cursor(selected_start + 1);
    assert_eq!(newline, expected_newline);
}

#[test]
fn composer_size_limit_accounts_for_replaced_selection() {
    let mut composer = ComposerState::default();
    let prefix = "x".repeat(MAX_COMPOSER_BYTES - "tail".len());
    composer.set_text(format!("{prefix}tail"));
    composer.set_selection(prefix.len(), MAX_COMPOSER_BYTES);

    let result = composer.insert_str("wide");

    assert_eq!(result, ComposerInsertResult::Inserted);
    assert_eq!(composer.text(), format!("{prefix}wide"));
    assert_eq!(composer.text().len(), MAX_COMPOSER_BYTES);
    assert_eq!(composer.selection_range(), None);
}

#[test]
fn rejected_replacement_preserves_the_entire_composer_state() {
    let mut composer = ComposerState::default();
    let prefix = "x".repeat(MAX_COMPOSER_BYTES - "tail".len());
    composer.set_text(format!("{prefix}tail"));
    composer.set_selection(prefix.len(), MAX_COMPOSER_BYTES);
    let before = composer.clone();

    let result = composer.insert_str("wider");

    assert_eq!(
        result,
        ComposerInsertResult::TooLarge {
            attempted_bytes: MAX_COMPOSER_BYTES + 1,
        }
    );
    assert_eq!(composer, before);
}

#[test]
fn vertical_selection_collapse_does_not_recall_history() {
    let mut up = ComposerState::default();
    up.remember_submission("history");
    up.set_text("draft");
    up.set_selection(/*anchor*/ 0, "draft".len());
    let mut expected_up = up.clone();
    expected_up.clear_selection();

    up.move_up_or_recall_history();

    assert_eq!(up, expected_up);

    let mut down = ComposerState::default();
    down.remember_submission("history");
    down.set_text("draft");
    down.move_up_or_recall_history();
    assert_eq!(down.text(), "history");
    down.set_selection(/*anchor*/ 0, "history".len());
    let mut expected_down = down.clone();
    expected_down.clear_selection();

    down.move_down_or_recall_history();

    assert_eq!(down, expected_down);
}

#[test]
fn queue_editing_restores_the_draft_selection() {
    let mut composer = ComposerState::default();
    composer.set_text("queued");
    assert!(composer.queue_current_message());
    composer.set_text("draft selection");
    let selection_start = composer
        .text()
        .find("selection")
        .expect("selection should be present");
    composer.set_selection(selection_start, composer.text().len());
    let before = composer.clone();

    assert!(composer.edit_previous_queued_message());
    assert!(composer.finish_queued_message_edit());

    assert_eq!(composer, before);
}
