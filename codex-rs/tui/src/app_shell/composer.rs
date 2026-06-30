use std::collections::VecDeque;

const MAX_COMPOSER_HISTORY: usize = 50;

#[derive(Debug, Clone, Default)]
pub(super) struct ComposerState {
    text: String,
    cursor: usize,
    history: VecDeque<String>,
    history_index: Option<usize>,
    draft_before_history: String,
}

impl ComposerState {
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(super) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(super) fn cursor_position(&self) -> (usize, usize) {
        let line_start = self.line_start(self.cursor);
        let line = self.text[..line_start]
            .chars()
            .filter(|ch| *ch == '\n')
            .count();
        let column = self.text[line_start..self.cursor].chars().count();
        (line, column)
    }

    pub(super) fn submission_text(&self) -> String {
        self.text.trim().to_string()
    }

    pub(super) fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.clear_history_recall();
    }

    pub(super) fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
        self.clear_history_recall();
    }

    pub(super) fn insert_char(&mut self, ch: char) {
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.clear_history_recall();
    }

    pub(super) fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    pub(super) fn insert_str(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.text.insert_str(self.cursor, &normalized);
        self.cursor += normalized.len();
        self.clear_history_recall();
    }

    pub(super) fn backspace(&mut self) {
        let Some(previous) = self.previous_boundary(self.cursor) else {
            return;
        };
        self.text.drain(previous..self.cursor);
        self.cursor = previous;
        self.clear_history_recall();
    }

    pub(super) fn delete(&mut self) {
        let Some(next) = self.next_boundary(self.cursor) else {
            return;
        };
        self.text.drain(self.cursor..next);
        self.clear_history_recall();
    }

    pub(super) fn move_left(&mut self) {
        if let Some(previous) = self.previous_boundary(self.cursor) {
            self.cursor = previous;
        }
    }

    pub(super) fn move_right(&mut self) {
        if let Some(next) = self.next_boundary(self.cursor) {
            self.cursor = next;
        }
    }

    pub(super) fn move_word_left(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let mut boundary = 0;
        let mut found_word = false;
        for (index, ch) in self.text[..self.cursor].char_indices().rev() {
            if word_motion_char(ch) {
                found_word = true;
                boundary = index;
            } else if found_word {
                break;
            }
        }
        self.cursor = boundary;
    }

    pub(super) fn move_word_right(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }

        let mut found_word = false;
        for (offset, ch) in self.text[self.cursor..].char_indices() {
            let index = self.cursor + offset;
            if word_motion_char(ch) {
                found_word = true;
            } else if found_word {
                self.cursor = index;
                return;
            }
        }
        self.cursor = self.text.len();
    }

    pub(super) fn move_to_line_start(&mut self) {
        self.cursor = self.line_start(self.cursor);
    }

    pub(super) fn move_to_line_end(&mut self) {
        self.cursor = self.line_end(self.cursor);
    }

    pub(super) fn move_up_or_recall_history(&mut self) {
        if !self.move_up() {
            self.recall_previous_history();
        }
    }

    pub(super) fn move_down_or_recall_history(&mut self) {
        if self.history_index.is_some() {
            self.recall_next_history();
        } else {
            self.move_down();
        }
    }

    pub(super) fn remember_submission(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() || self.history.back().is_some_and(|entry| entry == text) {
            return;
        }

        self.history.push_back(text.to_string());
        while self.history.len() > MAX_COMPOSER_HISTORY {
            self.history.pop_front();
        }
    }

    fn move_up(&mut self) -> bool {
        let current_start = self.line_start(self.cursor);
        if current_start == 0 {
            return false;
        }

        let previous_end = current_start - 1;
        let previous_start = self.line_start(previous_end);
        let column = self.cursor_column();
        self.cursor = self.byte_for_column(previous_start, previous_end, column);
        true
    }

    fn move_down(&mut self) -> bool {
        let current_end = self.line_end(self.cursor);
        if current_end >= self.text.len() {
            return false;
        }

        let next_start = current_end + 1;
        let next_end = self.line_end(next_start);
        let column = self.cursor_column();
        self.cursor = self.byte_for_column(next_start, next_end, column);
        true
    }

    fn recall_previous_history(&mut self) {
        if self.history.is_empty() {
            return;
        }

        let index = match self.history_index {
            Some(index) => index.saturating_sub(1),
            None => {
                self.draft_before_history = self.text.clone();
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
        self.text = text;
        self.cursor = self.text.len();
    }

    fn clear_history_recall(&mut self) {
        self.history_index = None;
        self.draft_before_history.clear();
    }

    fn cursor_column(&self) -> usize {
        let start = self.line_start(self.cursor);
        self.text[start..self.cursor].chars().count()
    }

    fn line_start(&self, cursor: usize) -> usize {
        self.text[..cursor]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0)
    }

    fn line_end(&self, cursor: usize) -> usize {
        self.text[cursor..]
            .find('\n')
            .map(|offset| cursor + offset)
            .unwrap_or(self.text.len())
    }

    fn byte_for_column(&self, start: usize, end: usize, target_column: usize) -> usize {
        self.text[start..end]
            .char_indices()
            .nth(target_column)
            .map(|(offset, _)| start + offset)
            .unwrap_or(end)
    }

    fn previous_boundary(&self, cursor: usize) -> Option<usize> {
        self.text[..cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
    }

    fn next_boundary(&self, cursor: usize) -> Option<usize> {
        self.text[cursor..]
            .chars()
            .next()
            .map(|ch| cursor + ch.len_utf8())
    }
}

fn word_motion_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
