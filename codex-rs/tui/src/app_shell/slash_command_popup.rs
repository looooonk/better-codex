use super::ShellState;
use super::composer::ComposerState;
use super::slash_commands::SLASH_COMMANDS;
use super::slash_commands::SlashCommandDefinition;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use std::ops::Range;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SlashCommandPopupState {
    query: Option<String>,
    selected: usize,
    dismissed_query: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SlashCommandPopupKeyResult {
    Unhandled,
    Consumed,
    Submit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionMode {
    ContinueEditing,
    Submit,
}

pub(super) struct SlashCommandSuggestions {
    query: String,
    token_range: Range<usize>,
    entries: Vec<SlashCommandDefinition>,
    selected: usize,
}

impl SlashCommandPopupState {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    fn sync(&mut self, query: &str, entry_count: usize) {
        if self.query.as_deref() != Some(query) {
            self.query = Some(query.to_string());
            self.selected = 0;
            self.dismissed_query = None;
        }
        self.selected = self.selected.min(entry_count.saturating_sub(1));
    }
}

impl SlashCommandSuggestions {
    fn candidate(composer: &ComposerState) -> Option<Self> {
        let text = composer.text();
        let cursor = composer.cursor();
        let before_cursor = text.get(..cursor)?;
        if !text.starts_with('/') || before_cursor.contains('\n') {
            return None;
        }
        let token_end = text.find(char::is_whitespace).unwrap_or(text.len());
        if cursor > token_end {
            return None;
        }
        let query = before_cursor.to_string();
        let entries = SLASH_COMMANDS
            .into_iter()
            .filter(|definition| definition.name().starts_with(&query))
            .collect::<Vec<_>>();
        (!entries.is_empty()).then_some(Self {
            query,
            token_range: 0..token_end,
            entries,
            selected: 0,
        })
    }

    pub(super) fn entries(&self) -> &[SlashCommandDefinition] {
        &self.entries
    }

    pub(super) fn selected(&self) -> usize {
        self.selected
    }

    fn selected_definition(&self) -> SlashCommandDefinition {
        self.entries[self.selected]
    }
}

impl ShellState {
    pub(super) fn slash_command_suggestions(&self) -> Option<SlashCommandSuggestions> {
        if !self.composer_owns_focus() {
            return None;
        }
        let mut suggestions = SlashCommandSuggestions::candidate(&self.composer)?;
        if self.slash_command_popup.dismissed_query.as_deref() == Some(&suggestions.query) {
            return None;
        }
        if self.slash_command_popup.query.as_deref() == Some(&suggestions.query) {
            suggestions.selected = self
                .slash_command_popup
                .selected
                .min(suggestions.entries.len().saturating_sub(1));
        }
        Some(suggestions)
    }

    pub(super) fn handle_slash_command_popup_key(
        &mut self,
        key: KeyEvent,
    ) -> SlashCommandPopupKeyResult {
        if key.modifiers != KeyModifiers::NONE || !self.composer_owns_focus() {
            return SlashCommandPopupKeyResult::Unhandled;
        }
        let Some(mut suggestions) = SlashCommandSuggestions::candidate(&self.composer) else {
            return SlashCommandPopupKeyResult::Unhandled;
        };
        self.slash_command_popup
            .sync(&suggestions.query, suggestions.entries.len());
        if self.slash_command_popup.dismissed_query.as_deref() == Some(&suggestions.query) {
            return SlashCommandPopupKeyResult::Unhandled;
        }
        suggestions.selected = self.slash_command_popup.selected;

        match key.code {
            KeyCode::Up => {
                self.slash_command_popup.selected = self
                    .slash_command_popup
                    .selected
                    .checked_sub(1)
                    .unwrap_or(suggestions.entries.len() - 1);
                SlashCommandPopupKeyResult::Consumed
            }
            KeyCode::Down => {
                self.slash_command_popup.selected =
                    (self.slash_command_popup.selected + 1) % suggestions.entries.len();
                SlashCommandPopupKeyResult::Consumed
            }
            KeyCode::Tab => {
                self.complete_slash_command(&suggestions, CompletionMode::ContinueEditing);
                SlashCommandPopupKeyResult::Consumed
            }
            KeyCode::Enter => {
                self.complete_slash_command(&suggestions, CompletionMode::Submit);
                SlashCommandPopupKeyResult::Submit
            }
            KeyCode::Esc => {
                self.slash_command_popup.dismissed_query = Some(suggestions.query);
                SlashCommandPopupKeyResult::Consumed
            }
            KeyCode::BackTab
            | KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Char(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => SlashCommandPopupKeyResult::Unhandled,
        }
    }

    fn complete_slash_command(
        &mut self,
        suggestions: &SlashCommandSuggestions,
        mode: CompletionMode,
    ) {
        let command = suggestions.selected_definition().name();
        let suffix = &self.composer.text()[suggestions.token_range.end..];
        let existing_horizontal_space = suffix
            .chars()
            .next()
            .filter(|ch| matches!(ch, ' ' | '\t'))
            .map_or(0, char::len_utf8);
        let add_space = existing_horizontal_space == 0 && mode == CompletionMode::ContinueEditing;
        let mut text = String::with_capacity(command.len() + usize::from(add_space) + suffix.len());
        text.push_str(command);
        if add_space {
            text.push(' ');
        }
        text.push_str(suffix);
        let cursor = command.len() + usize::from(add_space) + existing_horizontal_space;
        self.composer.set_text(text);
        self.composer.set_cursor(cursor);
        self.slash_command_popup.reset();
    }
}

#[cfg(test)]
#[path = "slash_command_popup_tests.rs"]
mod tests;
