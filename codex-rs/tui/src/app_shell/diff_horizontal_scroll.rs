use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use std::cell::Cell;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

const HORIZONTAL_SCROLL_STEP: usize = 8;

#[derive(Debug, Default)]
pub(super) struct HorizontalScroll {
    offset: Cell<usize>,
    max: Cell<usize>,
}

impl HorizontalScroll {
    pub(super) fn offset(&self) -> usize {
        self.offset.get().min(self.max.get())
    }

    pub(super) fn max(&self) -> usize {
        self.max.get()
    }

    pub(super) fn set_max(&self, max: usize) {
        self.max.set(max);
        self.offset.set(self.offset.get().min(max));
    }

    pub(super) fn handle_key(&self, key: KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Left, KeyModifiers::SHIFT) => {
                self.offset
                    .set(self.offset().saturating_sub(HORIZONTAL_SCROLL_STEP));
                true
            }
            (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Right, KeyModifiers::SHIFT) => {
                self.offset.set(
                    self.offset()
                        .saturating_add(HORIZONTAL_SCROLL_STEP)
                        .min(self.max.get()),
                );
                true
            }
            (KeyCode::Home, KeyModifiers::SHIFT) => {
                self.offset.set(0);
                true
            }
            (KeyCode::End, KeyModifiers::SHIFT) => {
                self.offset.set(self.max.get());
                true
            }
            _ => false,
        }
    }

    pub(super) fn visible_text(&self, text: &str, width: usize) -> String {
        if width == 0 {
            return String::new();
        }
        let offset = self.offset();
        let hidden_right = UnicodeWidthStr::width(text) > offset.saturating_add(width);
        let width = width.saturating_sub(usize::from(hidden_right));
        let mut column = 0usize;
        let mut used = 0usize;
        let mut visible = String::new();
        for ch in text.chars() {
            let char_width = UnicodeWidthChar::width(ch).unwrap_or_default();
            let start_column = column;
            let next_column = column.saturating_add(char_width);
            if next_column <= offset {
                column = next_column;
                continue;
            }
            column = next_column;
            if start_column < offset {
                let partial_width = next_column.saturating_sub(offset);
                if used.saturating_add(partial_width) > width {
                    break;
                }
                used = used.saturating_add(partial_width);
                visible.extend(std::iter::repeat_n(' ', partial_width));
                continue;
            }
            if used.saturating_add(char_width) > width {
                break;
            }
            used = used.saturating_add(char_width);
            visible.push(ch);
        }
        if hidden_right {
            visible.push('…');
        }
        visible
    }

    pub(super) fn reset(&self) {
        self.offset.set(0);
        self.max.set(0);
    }
}
