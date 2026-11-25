use super::super::ShellState;
use super::super::composer_layout::ComposerVerticalDirection;
use super::super::composer_layout::ComposerVerticalTarget;
use super::super::composer_navigation::ComposerNavigationLayout;
use crate::text_input::text_input_action_from_key;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

impl ShellState {
    /// Extend the same source-byte selection used by mouse drags, copy, and editing.
    pub(in crate::app_shell) fn extend_composer_selection(
        &mut self,
        key: KeyEvent,
        layout: ComposerNavigationLayout,
    ) -> bool {
        let modifiers = key.modifiers - KeyModifiers::SHIFT;
        match (key.code, modifiers) {
            (KeyCode::Left | KeyCode::Up, KeyModifiers::SUPER)
            | (KeyCode::Home, KeyModifiers::CONTROL) => {
                self.composer
                    .input
                    .extend_selection_with(|input| input.set_cursor(0));
            }
            (KeyCode::Right | KeyCode::Down, KeyModifiers::SUPER)
            | (KeyCode::End, KeyModifiers::CONTROL) => {
                self.composer
                    .input
                    .extend_selection_with(|input| input.set_cursor(input.text().len()));
            }
            (KeyCode::Up | KeyCode::Down, KeyModifiers::NONE) => {
                let direction = if key.code == KeyCode::Up {
                    ComposerVerticalDirection::Up
                } else {
                    ComposerVerticalDirection::Down
                };
                let visual_target = match layout {
                    ComposerNavigationLayout::Area(area) => {
                        Some(self.composer_vertical_target(area, direction))
                    }
                    ComposerNavigationLayout::LogicalLines => None,
                };
                self.composer.input.extend_selection_with(|input| {
                    let boundary = match direction {
                        ComposerVerticalDirection::Up => 0,
                        ComposerVerticalDirection::Down => input.text().len(),
                    };
                    let cursor = match visual_target {
                        Some(ComposerVerticalTarget::Cursor(cursor)) => {
                            input
                                .display()
                                .source_range_for_display_range(cursor..cursor)
                                .start
                        }
                        Some(ComposerVerticalTarget::Boundary) => boundary,
                        None => {
                            let moved = match direction {
                                ComposerVerticalDirection::Up => input.move_up(),
                                ComposerVerticalDirection::Down => input.move_down(),
                            };
                            if moved {
                                return;
                            }
                            boundary
                        }
                    };
                    input.set_cursor(cursor);
                });
            }
            (KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End, _) => {
                let Some(action) = text_input_action_from_key(KeyEvent { modifiers, ..key }) else {
                    return false;
                };
                self.composer.input.extend_selection_with(|input| {
                    input.apply(action);
                });
            }
            _ => return false,
        }
        true
    }
}
