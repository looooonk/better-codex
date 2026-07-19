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
