//! Small reusable multiline composer for non-chat TUI surfaces.

use std::time::Duration;

use crate::text_input::EditableText;
use crate::text_input::text_input_action_from_key;
use crate::text_input::text_input_shortcut_from_key;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

/// Action returned from feeding a key event into the composer.
pub enum ComposerAction {
    /// The user submitted the current text.
    Submitted(String),
    /// No submission occurred.
    None,
}

/// A minimal multiline input field with submit semantics.
#[derive(Clone, Debug)]
pub struct ComposerInput {
    input: EditableText,
    hint_items: Option<Vec<(String, String)>>,
}

impl ComposerInput {
    /// Create an empty composer input.
    pub fn new() -> Self {
        Self {
            input: EditableText::default(),
            hint_items: None,
        }
    }

    /// Returns true if the input is empty.
    pub fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    /// Return the current input text.
    pub fn text(&self) -> &str {
        self.input.text()
    }

    /// Return the input text with a software cursor marker.
    pub fn text_with_cursor(&self) -> String {
        self.input.text_with_cursor()
    }

    /// Return a width-bounded input window that always contains the cursor.
    pub fn text_with_cursor_window(&self, max_width: usize) -> String {
        self.input.text_with_cursor_window(max_width)
    }

    /// Clear the input text.
    pub fn clear(&mut self) {
        self.input.clear();
    }

    /// Apply a modified text-editing shortcut if the event matches one.
    pub fn apply_text_shortcut(&mut self, key: KeyEvent) -> bool {
        let Some(action) = text_input_shortcut_from_key(key) else {
            return false;
        };
        self.input.apply(action);
        true
    }

    /// Feed a key event into the composer and return a high-level action.
    pub fn input(&mut self, key: KeyEvent) -> ComposerAction {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return ComposerAction::None;
        }
        if let Some(action) = text_input_action_from_key(key) {
            self.input.apply(action);
            return ComposerAction::None;
        }

        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.input.insert_str("\n");
            }
            KeyCode::Enter => {
                let submitted = self.input.text().trim().to_string();
                if !submitted.is_empty() {
                    self.clear();
                    return ComposerAction::Submitted(submitted);
                }
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.input.insert_char(ch);
            }
            KeyCode::Tab | KeyCode::BackTab => self.input.insert_str("    "),
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Esc
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_)
            | KeyCode::Char(_) => {}
        }
        ComposerAction::None
    }

    /// Insert pasted text after normalizing line endings.
    pub fn handle_paste(&mut self, pasted: String) -> bool {
        if pasted.is_empty() {
            return false;
        }
        self.input
            .insert_str(&pasted.replace("\r\n", "\n").replace('\r', "\n"));
        true
    }

    /// Override the footer hint items displayed under the composer.
    pub fn set_hint_items(&mut self, items: Vec<(impl Into<String>, impl Into<String>)>) {
        self.hint_items = Some(
            items
                .into_iter()
                .map(|(key, label)| (key.into(), label.into()))
                .collect(),
        );
    }

    /// Clear custom footer hints.
    pub fn clear_hint_items(&mut self) {
        self.hint_items = None;
    }

    /// Desired height (in rows) for a given width.
    pub fn desired_height(&self, width: u16) -> u16 {
        u16::try_from(
            self.rendered_lines(usize::from(width))
                .len()
                .saturating_add(1),
        )
        .unwrap_or(u16::MAX)
    }

    /// Compute the on-screen cursor position for the given area.
    pub fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        if area.width == 0 || area.height <= 1 {
            return None;
        }
        let width = usize::from(area.width);
        let content_width = width.saturating_sub(2).max(1);
        let text = self.input.text();
        let cursor = self.input.cursor();
        let line_start = text[..cursor]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let logical_line_index = text[..line_start].chars().filter(|ch| *ch == '\n').count();
        let logical_line = text[line_start..].split('\n').next().unwrap_or_default();
        let cursor_in_line = cursor.saturating_sub(line_start);
        let options =
            textwrap::Options::new(content_width).wrap_algorithm(textwrap::WrapAlgorithm::FirstFit);
        let ranges = crate::wrapping::wrap_ranges(logical_line, options);
        let wrapped_line = ranges
            .partition_point(|range| range.start <= cursor_in_line)
            .saturating_sub(1);
        let wrapped_start = ranges
            .get(wrapped_line)
            .map(|range| range.start)
            .unwrap_or(0);
        let row = self
            .input
            .text()
            .split('\n')
            .take(logical_line_index)
            .map(|line| textwrap::wrap(line, content_width).len().max(1))
            .sum::<usize>()
            .saturating_add(wrapped_line);
        let column = 2 + UnicodeWidthStr::width(&logical_line[wrapped_start..cursor_in_line]);
        let visible_height = usize::from(area.height.saturating_sub(1));
        let total_height = self.rendered_lines(width).len();
        let visible_start = total_height.saturating_sub(visible_height);
        let row = row.saturating_sub(visible_start);
        (row < visible_height).then(|| {
            (
                area.x
                    .saturating_add(u16::try_from(column).unwrap_or(u16::MAX)),
                area.y
                    .saturating_add(u16::try_from(row).unwrap_or(u16::MAX)),
            )
        })
    }

    /// Render the input into the provided buffer at `area`.
    pub fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let input_height = area.height.saturating_sub(1);
        let input_area = Rect::new(area.x, area.y, area.width, input_height);
        let lines = self.rendered_lines(usize::from(area.width));
        let visible_start = lines.len().saturating_sub(usize::from(input_height));
        Paragraph::new(lines.into_iter().skip(visible_start).collect::<Vec<_>>())
            .render(input_area, buf);

        if area.height > 1 {
            let footer = self.footer_line();
            footer.render(
                Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
                buf,
            );
        }
    }

    /// The standalone composer inserts paste events atomically.
    pub fn is_in_paste_burst(&self) -> bool {
        false
    }

    /// No deferred paste state is retained.
    pub fn flush_paste_burst_if_due(&mut self) -> bool {
        false
    }

    /// Delay retained for callers that schedule composer refreshes.
    pub fn recommended_flush_delay() -> Duration {
        Duration::from_millis(10)
    }

    fn rendered_lines(&self, width: usize) -> Vec<Line<'static>> {
        if self.input.is_empty() {
            return vec![vec!["> ".cyan(), "Compose new task".dim()].into()];
        }

        let content_width = width.saturating_sub(2).max(1);
        self.input
            .text()
            .split('\n')
            .flat_map(|logical_line| {
                let wrapped = textwrap::wrap(logical_line, content_width);
                let wrapped = if wrapped.is_empty() {
                    vec![std::borrow::Cow::Borrowed("")]
                } else {
                    wrapped
                };
                wrapped.into_iter().enumerate().map(|(index, line)| {
                    vec![
                        if index == 0 { "> ".cyan() } else { "  ".dim() },
                        line.into_owned().into(),
                    ]
                    .into()
                })
            })
            .collect()
    }

    fn footer_line(&self) -> Line<'static> {
        let items = self.hint_items.clone().unwrap_or_else(|| {
            vec![
                ("Enter".to_string(), "send".to_string()),
                ("Shift+Enter".to_string(), "newline".to_string()),
            ]
        });
        let mut spans = Vec::new();
        for (index, (key, label)) in items.into_iter().enumerate() {
            if index > 0 {
                spans.push("   ".into());
            }
            spans.push(key.cyan());
            spans.push(" ".into());
            spans.push(label.dim());
        }
        spans.into()
    }
}

impl Default for ComposerInput {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "composer_input_tests.rs"]
mod tests;
