use crate::text_input::EditableText;
use crate::text_input::EditableTextDisplay;
use crate::text_input::TextInputAction;
use codex_protocol::user_input::MAX_USER_INPUT_TEXT_CHARS;
use std::collections::VecDeque;
use std::ops::Range;
use unicode_width::UnicodeWidthStr;

const MAX_COMPOSER_HISTORY: usize = 50;
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ComposerState {
    input: EditableText,
    history: VecDeque<String>,
    history_index: Option<usize>,
    draft_before_history: String,
    queued: VecDeque<String>,
    queued_index: Option<usize>,
    draft_before_queue: Option<ComposerDraft>,
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

    pub(super) fn queue_current_message(&mut self) -> bool {
        if self.queued_index.is_some() {
            return self.finish_queued_message_edit();
        }
        let message = self.submission_text();
        if message.trim().is_empty() {
            return false;
        }

        self.queued.push_back(message);
        self.clear();
        true
    }

    pub(super) fn has_queued_messages(&self) -> bool {
        !self.queued.is_empty()
    }

    pub(super) fn queued_count(&self) -> usize {
        self.queued.len()
    }

    pub(super) fn queued_edit_position(&self) -> Option<(usize, usize)> {
        self.queued_index
            .map(|index| (index.saturating_add(1), self.queued.len()))
    }

    pub(super) fn edit_previous_queued_message(&mut self) -> bool {
        if self.queued.is_empty() {
            return false;
        }

        let index = if let Some(index) = self.queued_index {
            self.save_queued_message_edit();
            if self.queued.is_empty() {
                self.restore_queue_draft();
                return true;
            }
            index
                .saturating_sub(1)
                .min(self.queued.len().saturating_sub(1))
        } else {
            self.draft_before_queue = Some(ComposerDraft {
                input: self.input.clone(),
                history_index: self.history_index,
                draft_before_history: self.draft_before_history.clone(),
            });
            self.queued.len() - 1
        };
        self.select_queued_message(index);
        true
    }

    pub(super) fn edit_next_queued_message(&mut self) -> bool {
        let Some(index) = self.queued_index else {
            return false;
        };
        let removed = self.save_queued_message_edit();
        let next = if removed {
            index
        } else {
            index.saturating_add(1)
        };
        if next < self.queued.len() {
            self.select_queued_message(next);
        } else {
            self.restore_queue_draft();
        }
        true
    }

    pub(super) fn finish_queued_message_edit(&mut self) -> bool {
        if self.queued_index.is_none() {
            return false;
        }
        self.save_queued_message_edit();
        self.restore_queue_draft();
        true
    }

    fn restore_queue_draft(&mut self) {
        self.queued_index = None;
        let draft = self.draft_before_queue.take().unwrap_or_default();
        self.input = draft.input;
        self.history_index = draft.history_index;
        self.draft_before_history = draft.draft_before_history;
    }

    pub(super) fn prepare_next_queued_message(&mut self) -> Option<String> {
        self.finish_queued_message_edit();
        self.queued.front().cloned()
    }

    pub(super) fn confirm_next_queued_message(&mut self, message: &str) {
        if self.queued.front().map(String::as_str) == Some(message) {
            self.queued.pop_front();
        }
        self.remember_submission(message);
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

    pub(super) fn move_up_or_recall_history(&mut self) {
        if !self.move_up() {
            self.recall_previous_history();
        }
    }

    pub(super) fn move_down_or_recall_history(&mut self) {
        if self.input.selection_range().is_some() {
            self.move_down();
        } else if self.history_index.is_some() {
            self.recall_next_history();
        } else {
            self.move_down();
        }
    }

    pub(super) fn remember_submission(&mut self, text: &str) {
        if text.trim().is_empty() || self.history.back().is_some_and(|entry| entry == text) {
            return;
        }

        self.history.push_back(text.to_string());
        while self.history.len() > MAX_COMPOSER_HISTORY {
            self.history.pop_front();
        }
    }

    pub(super) fn move_up(&mut self) -> bool {
        self.input.move_up()
    }

    pub(super) fn move_down(&mut self) -> bool {
        self.input.move_down()
    }

    fn recall_previous_history(&mut self) {
        if self.history.is_empty() {
            return;
        }

        let index = match self.history_index {
            Some(index) => index.saturating_sub(1),
            None => {
                self.draft_before_history = self.input.text().to_string();
                self.history.len() - 1
            }
        };
        self.history_index = Some(index);
        if let Some(entry) = self.history.get(index).cloned() {
            self.set_recalled_text(entry);
        }
    }

    fn recall_next_history(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };

        if index + 1 < self.history.len() {
            let next = index + 1;
            self.history_index = Some(next);
            if let Some(entry) = self.history.get(next).cloned() {
                self.set_recalled_text(entry);
            }
        } else {
            self.history_index = None;
            let draft = std::mem::take(&mut self.draft_before_history);
            self.set_recalled_text(draft);
        }
    }

    fn set_recalled_text(&mut self, text: String) {
        self.input.set_text(text);
    }

    fn select_queued_message(&mut self, index: usize) {
        self.queued_index = Some(index);
        if let Some(message) = self.queued.get(index).cloned() {
            self.set_text(message);
        }
    }

    fn save_queued_message_edit(&mut self) -> bool {
        let Some(index) = self.queued_index else {
            return false;
        };
        let message = self.submission_text();
        if message.trim().is_empty() {
            return self.queued.remove(index).is_some();
        }
        if let Some(queued) = self.queued.get_mut(index) {
            *queued = message;
        }
        false
    }

    fn clear_history_recall(&mut self) {
        self.history_index = None;
        self.draft_before_history.clear();
    }
}

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
