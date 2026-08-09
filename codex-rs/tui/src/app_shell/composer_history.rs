use super::super::composer_layout::ComposerVerticalDirection;
use super::super::composer_layout::ComposerVerticalTarget;
use super::ComposerState;

const MAX_COMPOSER_HISTORY: usize = 50;

impl ComposerState {
    pub(in crate::app_shell) fn move_up_or_recall_history(&mut self) {
        if !self.move_up() {
            self.recall_previous_history();
        }
    }

    pub(in crate::app_shell) fn move_or_recall_history_visually(
        &mut self,
        direction: ComposerVerticalDirection,
        target: ComposerVerticalTarget,
    ) {
        if self.input.selection_range().is_some() {
            self.input.clear_selection();
            return;
        }
        match direction {
            ComposerVerticalDirection::Up => {
                if !self.move_to_vertical_target(target) {
                    self.recall_previous_history();
                }
            }
            ComposerVerticalDirection::Down => {
                if self.history_index.is_some() {
                    self.recall_next_history();
                } else {
                    self.move_to_vertical_target(target);
                }
            }
        }
    }

    pub(in crate::app_shell) fn move_down_or_recall_history(&mut self) {
        if self.input.selection_range().is_some() {
            self.move_down();
        } else if self.history_index.is_some() {
            self.recall_next_history();
        } else {
            self.move_down();
        }
    }

    pub(in crate::app_shell) fn remember_submission(&mut self, text: &str) {
        if text.trim().is_empty() || self.history.back().is_some_and(|entry| entry == text) {
            return;
        }

        self.history.push_back(text.to_string());
        while self.history.len() > MAX_COMPOSER_HISTORY {
            self.history.pop_front();
        }
    }

    pub(in crate::app_shell) fn move_up(&mut self) -> bool {
        self.input.move_up()
    }

    pub(in crate::app_shell) fn move_down(&mut self) -> bool {
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

    fn move_to_vertical_target(&mut self, target: ComposerVerticalTarget) -> bool {
        let ComposerVerticalTarget::Cursor(cursor) = target else {
            return false;
        };
        self.set_cursor_from_display_range(cursor..cursor);
        true
    }
}
