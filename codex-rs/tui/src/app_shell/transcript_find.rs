//! Bounded search over the conversation already retained by this terminal.

use super::ShellState;
use super::TranscriptKind;
use super::selector::SelectorOption;
use super::selector::SelectorState;
use super::selector::SelectorValue;
use crate::text_input::EditableText;
use crate::text_input::text_input_action_from_key;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

const QUERY_MAX_BYTES: usize = 256;
const SEARCH_ITEM_MAX_BYTES: usize = 8_192;
const MAX_RESULTS: usize = 50;

impl ShellState {
    pub(super) fn open_transcript_find(&mut self, query: &str) {
        self.show_transcript_find(EditableText::new(
            &query[..query.floor_char_boundary(QUERY_MAX_BYTES.min(query.len()))],
        ));
    }

    fn show_transcript_find(&mut self, query: EditableText) {
        let needle = query.text().to_lowercase();
        let options = self
            .transcript
            .iter()
            .rev()
            .take(super::MAX_TRANSCRIPT_LINES)
            .filter_map(|line| {
                let text = &line.text[..line
                    .text
                    .floor_char_boundary(SEARCH_ITEM_MAX_BYTES.min(line.text.len()))];
                if !text.to_lowercase().contains(&needle) {
                    return None;
                }
                let role = match line.kind {
                    TranscriptKind::User => "You",
                    TranscriptKind::Assistant | TranscriptKind::Plan => "Assistant",
                    TranscriptKind::Tool | TranscriptKind::Output | TranscriptKind::Diff => {
                        "Activity"
                    }
                    TranscriptKind::System | TranscriptKind::Status | TranscriptKind::Audit => {
                        "System"
                    }
                    TranscriptKind::Separator => return None,
                    TranscriptKind::Error => "Error",
                };
                let preview = text.split_whitespace().collect::<Vec<_>>().join(" ");
                Some(SelectorOption::new(
                    SelectorValue::TranscriptMatch(line.render_revision),
                    format!("{role}: {}", preview.chars().take(64).collect::<String>()),
                    preview.chars().skip(64).take(110).collect::<String>(),
                ))
            })
            .take(MAX_RESULTS)
            .collect();
        self.open_selector(SelectorState::new(
            format!("Find retained text: {}", query.text_with_cursor_window(35)),
            options,
        ));
        if let Some(selector) = self.selector.as_mut() {
            selector.set_key_hints(format!(
                "Type to search  wheel move  {} select  {} cancel",
                self.keybindings.hint("list", "accept", "Enter"),
                self.keybindings.hint("list", "cancel", "Esc")
            ));
        }
        self.transcript_find = Some(query);
    }

    pub(super) fn handle_transcript_find_key(&mut self, key: KeyEvent) -> bool {
        if self.selector.is_none() {
            self.transcript_find = None;
        }
        let Some(mut query) = self.transcript_find.take() else {
            return false;
        };
        let handled = if let Some(action) = text_input_action_from_key(key) {
            query.apply(action);
            true
        } else if let KeyCode::Char(ch) = key.code
            && !ch.is_control()
            && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            if query.text().len().saturating_add(ch.len_utf8()) <= QUERY_MAX_BYTES {
                query.insert_char(ch);
            }
            true
        } else {
            false
        };
        if handled {
            self.show_transcript_find(query);
        } else {
            self.transcript_find = Some(query);
        }
        handled
    }

    pub(super) fn paste_transcript_find(&mut self, text: &str) -> bool {
        if self.selector.is_none() {
            self.transcript_find = None;
        }
        let Some(mut query) = self.transcript_find.take() else {
            return false;
        };
        for ch in text.chars() {
            let ch = if ch.is_whitespace() { ' ' } else { ch };
            if ch.is_control() {
                continue;
            }
            if query.text().len().saturating_add(ch.len_utf8()) > QUERY_MAX_BYTES {
                break;
            }
            query.insert_char(ch);
        }
        self.show_transcript_find(query);
        true
    }

    pub(super) fn select_transcript_match(&mut self, revision: u64) {
        self.selector = None;
        self.transcript_find = None;
        let Some(index) = self
            .transcript
            .iter()
            .position(|line| line.render_revision == revision)
        else {
            self.push_status(
                "That result changed or left the retained conversation. Search again with /find.",
            );
            return;
        };
        self.transcript_selection = Some(index);
        self.transcript_selection_needs_reveal.set(true);
        self.dashboard_visible = false;
        self.push_status("Search result selected; Esc returns to your draft");
    }
}
