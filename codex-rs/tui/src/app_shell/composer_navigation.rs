use super::ShellState;
use super::composer_layout::ComposerVerticalDirection;
use super::composer_layout::ComposerVerticalTarget;
use super::composer_layout::composer_body;
use super::render::ShellView;
use ratatui::layout::Rect;

#[derive(Clone, Copy)]
pub(super) enum ComposerNavigationLayout {
    LogicalLines,
    Area(Rect),
}

impl ShellState {
    pub(super) fn move_composer_up(&mut self, layout: ComposerNavigationLayout) {
        match layout {
            ComposerNavigationLayout::LogicalLines => {
                self.focused_editor_mut().move_up_or_recall_history();
            }
            ComposerNavigationLayout::Area(area) => {
                let target = self.composer_vertical_target(area, ComposerVerticalDirection::Up);
                self.focused_editor_mut()
                    .move_or_recall_history_visually(ComposerVerticalDirection::Up, target);
            }
        }
    }

    pub(super) fn move_composer_down(&mut self, layout: ComposerNavigationLayout) {
        match layout {
            ComposerNavigationLayout::LogicalLines => {
                self.focused_editor_mut().move_down_or_recall_history();
            }
            ComposerNavigationLayout::Area(area) => {
                let target = self.composer_vertical_target(area, ComposerVerticalDirection::Down);
                self.focused_editor_mut()
                    .move_or_recall_history_visually(ComposerVerticalDirection::Down, target);
            }
        }
    }

    pub(super) fn composer_vertical_target(
        &self,
        area: Rect,
        direction: ComposerVerticalDirection,
    ) -> ComposerVerticalTarget {
        let view = ShellView { shell: self };
        let body = view.question_popup(area).map_or_else(
            || composer_body(view.input_area(area)),
            |popup| popup.editor,
        );
        if body.width == 0 {
            return ComposerVerticalTarget::Boundary;
        }
        let display = self.focused_editor().display();
        if self.pending_user_input.is_some() {
            return super::composer_layout::ComposerLayout::new(display.text(), body.width.into())
                .vertical_target(display.cursor(), direction);
        }
        self.composer_layout
            .borrow_mut()
            .for_draft(display.text(), usize::from(body.width))
            .vertical_target(display.cursor(), direction)
    }
}

#[cfg(test)]
#[path = "composer_navigation_tests.rs"]
mod tests;
