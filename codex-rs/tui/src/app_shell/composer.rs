use crate::text_input::EditableText;
use crate::text_input::EditableTextDisplay;
use crate::text_input::TextInputAction;
use codex_protocol::user_input::MAX_USER_INPUT_TEXT_CHARS;
use std::collections::VecDeque;
use std::ops::Range;
use unicode_width::UnicodeWidthStr;

pub(super) const MAX_COMPOSER_BYTES: usize = MAX_USER_INPUT_TEXT_CHARS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposerInsertResult {
    Inserted,
    TooLarge { attempted_bytes: usize },
}

impl ComposerInsertResult {
    fn for_size(attempted_bytes: usize) -> Self {
        if attempted_bytes <= MAX_COMPOSER_BYTES {
            Self::Inserted
        } else {
            Self::TooLarge { attempted_bytes }
        }
    }
}

pub(super) fn input_too_large_message(actual_bytes: usize) -> String {
    format!(
        "Message exceeds the maximum size of {MAX_COMPOSER_BYTES} bytes ({actual_bytes} provided)."
    )
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ComposerDraft {
    input: EditableText,
    history_index: Option<usize>,
    draft_before_history: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedMessage {
    id: Option<String>,
    client_user_message_id: String,
    text: String,
    editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct QueuedMessageIdentity {
    pub(super) id: Option<String>,
    pub(super) client_user_message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum QueueEdit {
    Update {
        id: Option<String>,
        client_user_message_id: String,
        text: String,
    },
    Delete {
        id: Option<String>,
        client_user_message_id: String,
    },
    Reorder {
        order: Vec<QueuedMessageIdentity>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ComposerState {
    input: EditableText,
    history: VecDeque<String>,
    history_index: Option<usize>,
    draft_before_history: String,
    queued: VecDeque<QueuedMessage>,
    queued_index: Option<usize>,
    draft_before_queue: Option<ComposerDraft>,
    queue_edits: VecDeque<QueueEdit>,
}

impl ComposerState {
    pub(super) fn text(&self) -> &str {
        self.input.text()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    pub(super) fn cursor(&self) -> usize {
        self.input.cursor()
    }

    pub(super) fn selection_range(&self) -> Option<Range<usize>> {
        self.input.selection_range()
    }

    pub(super) fn selected_text(&self) -> Option<&str> {
        self.input.selected_text()
    }

    pub(super) fn set_cursor(&mut self, cursor: usize) {
        self.input.set_cursor(cursor);
    }

    pub(super) fn set_selection(&mut self, anchor: usize, cursor: usize) {
        self.input.set_selection(anchor, cursor);
    }

    pub(super) fn clear_selection(&mut self) {
        self.input.clear_selection();
    }

    pub(super) fn set_cursor_from_display_range(&mut self, display_range: Range<usize>) {
        let source_range = self
            .input
            .display()
            .source_range_for_display_range(display_range);
        self.input.set_cursor(source_range.start);
    }

    pub(super) fn set_selection_from_display_ranges(
        &mut self,
        anchor: Range<usize>,
        cursor: Range<usize>,
    ) {
        let cursor_precedes_anchor = cursor.start < anchor.start;
        let display = self.input.display();
        let anchor = display.source_range_for_display_range(anchor);
        let cursor = display.source_range_for_display_range(cursor);
        if cursor_precedes_anchor {
            self.input.set_selection(anchor.end, cursor.start);
        } else {
            self.input.set_selection(anchor.start, cursor.end);
        }
    }

    pub(super) fn display(&self) -> EditableTextDisplay<'_> {
        self.input.display()
    }

    pub(super) fn text_with_cursor_window(&self, max_width: usize) -> String {
        self.input.text_with_cursor_window(max_width)
    }

    pub(super) fn masked_text_with_cursor_window(&self, max_width: usize) -> String {
        self.input.masked_text_with_cursor_window(max_width)
    }

    pub(super) fn cursor_position(&self) -> (usize, usize) {
        let display = self.input.display();
        let cursor = display.cursor();
        let text = display.text();
        let line_start = text[..cursor]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let line = text[..line_start].chars().filter(|ch| *ch == '\n').count();
        let column = UnicodeWidthStr::width(&text[line_start..cursor]);
        (line, column)
    }

    pub(super) fn submission_text(&self) -> String {
        self.input.text().to_string()
    }

    pub(super) fn clear(&mut self) {
        self.input.clear();
        self.clear_history_recall();
    }

    pub(super) fn restore_failed_submission(&mut self, submission: &str) {
        let draft = self.input.text().to_string();
        self.set_text(if draft.is_empty() {
            submission.to_string()
        } else {
            format!("{submission}\n\n{draft}")
        });
    }

    pub(super) fn reset_for_session(&mut self) {
        *self = Self::default();
    }

    pub(super) fn set_text(&mut self, text: impl Into<String>) {
        self.input.set_text(text);
        self.clear_history_recall();
    }

    pub(super) fn insert_char(&mut self, ch: char) -> ComposerInsertResult {
        let result =
            ComposerInsertResult::for_size(self.size_after_replacing_selection(ch.len_utf8()));
        if let ComposerInsertResult::TooLarge { .. } = result {
            return result;
        }
        self.input.insert_char(ch);
        self.clear_history_recall();
        result
    }

    pub(super) fn insert_newline(&mut self) -> ComposerInsertResult {
        self.insert_char('\n')
    }

    pub(super) fn insert_str(&mut self, text: &str) -> ComposerInsertResult {
        let result =
            ComposerInsertResult::for_size(self.size_after_replacing_selection(text.len()));
        if let ComposerInsertResult::TooLarge { .. } = result {
            return result;
        }
        if text.contains('\r') {
            self.input
                .insert_str(&text.replace("\r\n", "\n").replace('\r', "\n"));
        } else {
            self.input.insert_str(text);
        }
        self.clear_history_recall();
        result
    }

    fn size_after_replacing_selection(&self, inserted_bytes: usize) -> usize {
        self.input
            .text()
            .len()
            .saturating_sub(self.input.selection_range().map_or(0, |range| range.len()))
            .saturating_add(inserted_bytes)
    }

    pub(super) fn move_left(&mut self) {
        self.input.apply(TextInputAction::MoveLeft);
    }

    pub(super) fn move_right(&mut self) {
        self.input.apply(TextInputAction::MoveRight);
    }

    pub(super) fn move_word_left(&mut self) {
        self.input.apply(TextInputAction::MoveWordLeft);
    }

    pub(super) fn move_word_right(&mut self) {
        self.input.apply(TextInputAction::MoveWordRight);
    }

    pub(super) fn apply_text_input_action(&mut self, action: TextInputAction) {
        if self.input.apply(action) {
            self.clear_history_recall();
        }
    }

    fn clear_history_recall(&mut self) {
        self.history_index = None;
        self.draft_before_history.clear();
    }
}

#[path = "composer_queue.rs"]
mod queue;

#[path = "composer_history.rs"]
mod history;

impl super::ShellState {
    pub(super) fn report_composer_insert(&mut self, result: ComposerInsertResult) {
        if let ComposerInsertResult::TooLarge { attempted_bytes } = result {
            self.push_error(input_too_large_message(attempted_bytes));
        }
    }

    pub(super) fn reject_oversized_composer(&mut self) -> bool {
        let actual_bytes = self.composer.text().len();
        self.reject_oversized_input(actual_bytes)
    }

    pub(super) fn reject_oversized_input(&mut self, actual_bytes: usize) -> bool {
        if actual_bytes <= MAX_COMPOSER_BYTES {
            return false;
        }
        self.push_error(input_too_large_message(actual_bytes));
        true
    }
}

#[cfg(test)]
#[path = "composer_tests.rs"]
mod tests;
