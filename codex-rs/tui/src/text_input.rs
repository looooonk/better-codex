use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextInputAction {
    MoveLeft,
    MoveRight,
    MoveLineStart,
    MoveLineEnd,
    MoveWordLeft,
    MoveWordRight,
    DeleteBackward,
    #[cfg(not(target_os = "macos"))]
    DeleteForward,
    DeleteWordLeft,
    DeleteToLineStart,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EditableText {
    text: String,
    cursor: usize,
}

impl EditableText {
    pub(crate) fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.len();
        Self { text, cursor }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn into_text(self) -> String {
        self.text
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn text_with_cursor(&self) -> String {
        let mut text = self.text.clone();
        text.insert(self.cursor, '▏');
        text
    }

    pub(crate) fn text_with_cursor_window(&self, max_width: usize) -> String {
        let text = self.text_with_cursor();
        if UnicodeWidthStr::width(text.as_str()) <= max_width {
            return text;
        }
        if max_width == 0 {
            return String::new();
        }
        if max_width == 1 {
            return "▏".to_string();
        }
        if max_width == 2 {
            return if self.cursor > 0 {
                "…▏".to_string()
            } else {
                "▏…".to_string()
            };
        }

        let left = self.text[..self.cursor].graphemes(true).collect::<Vec<_>>();
        let right = self.text[self.cursor..].graphemes(true).collect::<Vec<_>>();
        let mut left_start = left.len();
        let mut right_end = 0;
        let mut left_width = 0;
        let mut right_width = 0;

        loop {
            let prefer_left = left_width <= right_width;
            let mut added = false;
            for add_left in [prefer_left, !prefer_left] {
                if add_left && left_start > 0 {
                    let width = UnicodeWidthStr::width(left[left_start - 1]);
                    let next_start = left_start - 1;
                    let total = 1
                        + left_width
                        + width
                        + right_width
                        + usize::from(next_start > 0)
                        + usize::from(right_end < right.len());
                    if total <= max_width {
                        left_start = next_start;
                        left_width += width;
                        added = true;
                        break;
                    }
                } else if !add_left && right_end < right.len() {
                    let width = UnicodeWidthStr::width(right[right_end]);
                    let next_end = right_end + 1;
                    let total = 1
                        + left_width
                        + right_width
                        + width
                        + usize::from(left_start > 0)
                        + usize::from(next_end < right.len());
                    if total <= max_width {
                        right_end = next_end;
                        right_width += width;
                        added = true;
                        break;
                    }
                }
            }
            if !added {
                break;
            }
        }

        let mut visible = String::new();
        if left_start > 0 {
            visible.push('…');
        }
        for grapheme in &left[left_start..] {
            visible.push_str(grapheme);
        }
        visible.push('▏');
        for grapheme in &right[..right_end] {
            visible.push_str(grapheme);
        }
        if right_end < right.len() {
            visible.push('…');
        }
        visible
    }

    pub(crate) fn masked_text_with_cursor_window(&self, max_width: usize) -> String {
        let left = self.text[..self.cursor]
            .chars()
            .map(|ch| if ch == '\n' { '\n' } else { '*' })
            .collect::<String>();
        let right = self.text[self.cursor..]
            .chars()
            .map(|ch| if ch == '\n' { '\n' } else { '*' })
            .collect::<String>();
        Self {
            cursor: left.len(),
            text: left + &right,
        }
        .text_with_cursor_window(max_width)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(crate) fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
    }

    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub(crate) fn insert_char(&mut self, ch: char) {
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.normalize_cursor_forward();
    }

    pub(crate) fn insert_str(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.normalize_cursor_forward();
    }

    pub(crate) fn apply(&mut self, action: TextInputAction) -> bool {
        match action {
            TextInputAction::MoveLeft => self.move_left(),
            TextInputAction::MoveRight => self.move_right(),
            TextInputAction::MoveLineStart => self.move_to_line_start(),
            TextInputAction::MoveLineEnd => self.move_to_line_end(),
            TextInputAction::MoveWordLeft => self.move_word_left(),
            TextInputAction::MoveWordRight => self.move_word_right(),
            TextInputAction::DeleteBackward => return self.backspace(),
            #[cfg(not(target_os = "macos"))]
            TextInputAction::DeleteForward => return self.delete(),
            TextInputAction::DeleteWordLeft => return self.delete_word_left(),
            TextInputAction::DeleteToLineStart => return self.delete_to_line_start(),
        }
        false
    }

    pub(crate) fn move_up(&mut self) -> bool {
        let current_start = self.line_start();
        if current_start == 0 {
            return false;
        }

        let previous_end = current_start - 1;
        let previous_start = self.line_start_at(previous_end);
        let column = UnicodeWidthStr::width(&self.text[current_start..self.cursor]);
        self.cursor = self.byte_for_display_column(previous_start, previous_end, column);
        true
    }

    pub(crate) fn move_down(&mut self) -> bool {
        let current_start = self.line_start();
        let current_end = self.line_end();
        if current_end >= self.text.len() {
            return false;
        }

        let next_start = current_end + 1;
        let next_end = self.line_end_at(next_start);
        let column = UnicodeWidthStr::width(&self.text[current_start..self.cursor]);
        self.cursor = self.byte_for_display_column(next_start, next_end, column);
        true
    }

    fn backspace(&mut self) -> bool {
        let Some(previous) = self.previous_boundary() else {
            return false;
        };
        self.text.drain(previous..self.cursor);
        self.cursor = previous;
        true
    }

    #[cfg(not(target_os = "macos"))]
    fn delete(&mut self) -> bool {
        let Some(next) = self.next_boundary() else {
            return false;
        };
        self.text.drain(self.cursor..next);
        true
    }

    fn delete_word_left(&mut self) -> bool {
        let delete_from = self.word_left_boundary();
        if delete_from == self.cursor {
            return false;
        }
        self.text.drain(delete_from..self.cursor);
        self.cursor = delete_from;
        true
    }

    fn delete_to_line_start(&mut self) -> bool {
        let line_start = self.line_start();
        if line_start == self.cursor {
            return false;
        }
        self.text.drain(line_start..self.cursor);
        self.cursor = line_start;
        true
    }

    fn move_left(&mut self) {
        if let Some(previous) = self.previous_boundary() {
            self.cursor = previous;
        }
    }

    fn move_right(&mut self) {
        if let Some(next) = self.next_boundary() {
            self.cursor = next;
        }
    }

    fn move_word_left(&mut self) {
        self.cursor = self.word_left_boundary();
    }

    fn move_word_right(&mut self) {
        self.cursor = self
            .word_ranges()
            .into_iter()
            .find_map(|range| (range.end > self.cursor).then_some(range.end))
            .unwrap_or(self.text.len());
    }

    fn move_to_line_start(&mut self) {
        self.cursor = self.line_start();
    }

    fn move_to_line_end(&mut self) {
        self.cursor = self.line_end();
    }

    fn word_left_boundary(&self) -> usize {
        if self.cursor == 0 {
            return 0;
        }

        self.word_ranges()
            .into_iter()
            .take_while(|range| range.start < self.cursor)
            .last()
            .map(|range| range.start)
            .unwrap_or(0)
    }

    fn word_ranges(&self) -> Vec<Range<usize>> {
        let mut ranges = Vec::<Range<usize>>::new();
        for (start, word) in self.text.unicode_word_indices() {
            let end = start + word.len();
            if let Some(previous) = ranges.last_mut()
                && previous.end == start
            {
                previous.end = end;
            } else {
                ranges.push(start..end);
            }
        }
        ranges
    }

    fn line_start(&self) -> usize {
        self.line_start_at(self.cursor)
    }

    fn line_end(&self) -> usize {
        self.line_end_at(self.cursor)
    }

    fn line_start_at(&self, cursor: usize) -> usize {
        self.text[..cursor]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0)
    }

    fn line_end_at(&self, cursor: usize) -> usize {
        self.text[cursor..]
            .find('\n')
            .map(|offset| cursor + offset)
            .unwrap_or(self.text.len())
    }

    fn byte_for_display_column(&self, start: usize, end: usize, target_column: usize) -> usize {
        let mut boundary = start;
        let mut width = 0;
        for (offset, grapheme) in self.text[start..end].grapheme_indices(true) {
            let next_width = width + UnicodeWidthStr::width(grapheme);
            if next_width > target_column {
                break;
            }
            boundary = start + offset + grapheme.len();
            width = next_width;
        }
        boundary
    }

    fn normalize_cursor_forward(&mut self) {
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .find(|index| *index >= self.cursor)
            .unwrap_or(self.text.len());
    }

    fn previous_boundary(&self) -> Option<usize> {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map(|(index, _)| index)
    }

    fn next_boundary(&self) -> Option<usize> {
        self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map(|grapheme| self.cursor + grapheme.len())
    }
}

pub(crate) fn text_input_action_from_key(key: KeyEvent) -> Option<TextInputAction> {
    text_input_shortcut_from_key(key).or_else(|| plain_text_input_action_from_key(key))
}

pub(crate) fn text_input_shortcut_from_key(key: KeyEvent) -> Option<TextInputAction> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    match (key.code, key.modifiers) {
        (KeyCode::Left, KeyModifiers::SUPER) => Some(TextInputAction::MoveLineStart),
        (KeyCode::Right, KeyModifiers::SUPER) => Some(TextInputAction::MoveLineEnd),
        (KeyCode::Left, KeyModifiers::ALT | KeyModifiers::CONTROL)
        | (KeyCode::Char('b'), KeyModifiers::ALT) => Some(TextInputAction::MoveWordLeft),
        (KeyCode::Right, KeyModifiers::ALT | KeyModifiers::CONTROL)
        | (KeyCode::Char('f'), KeyModifiers::ALT) => Some(TextInputAction::MoveWordRight),
        (
            KeyCode::Backspace | KeyCode::Char('\u{007f}'),
            KeyModifiers::ALT | KeyModifiers::CONTROL,
        ) => Some(TextInputAction::DeleteWordLeft),
        (KeyCode::Backspace | KeyCode::Char('\u{007f}'), KeyModifiers::SUPER) => {
            Some(TextInputAction::DeleteToLineStart)
        }
        _ => None,
    }
}

fn plain_text_input_action_from_key(key: KeyEvent) -> Option<TextInputAction> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    match (key.code, key.modifiers) {
        (KeyCode::Left, KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(TextInputAction::MoveLeft)
        }
        (KeyCode::Right, KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(TextInputAction::MoveRight)
        }
        (
            KeyCode::Backspace | KeyCode::Char('\u{007f}'),
            KeyModifiers::NONE | KeyModifiers::SHIFT,
        ) => Some(TextInputAction::DeleteBackward),
        #[cfg(not(target_os = "macos"))]
        (KeyCode::Home, KeyModifiers::NONE) => Some(TextInputAction::MoveLineStart),
        #[cfg(not(target_os = "macos"))]
        (KeyCode::End, KeyModifiers::NONE) => Some(TextInputAction::MoveLineEnd),
        #[cfg(not(target_os = "macos"))]
        (KeyCode::Delete, KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(TextInputAction::DeleteForward)
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "text_input_tests.rs"]
mod tests;
