use super::ShellState;
use super::composer_layout::ComposerVerticalDirection;
use super::composer_layout::ComposerVerticalTarget;
use super::composer_layout::composer_vertical_target;
use super::design::body_rect_after_title;
use super::design::pane_content_rect;
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
                self.composer.move_up_or_recall_history();
            }
            ComposerNavigationLayout::Area(area) => {
                let target = self.composer_vertical_target(area, ComposerVerticalDirection::Up);
                self.composer
                    .move_or_recall_history_visually(ComposerVerticalDirection::Up, target);
            }
        }
    }

    pub(super) fn move_composer_down(&mut self, layout: ComposerNavigationLayout) {
        match layout {
            ComposerNavigationLayout::LogicalLines => {
                self.composer.move_down_or_recall_history();
            }
            ComposerNavigationLayout::Area(area) => {
                let target = self.composer_vertical_target(area, ComposerVerticalDirection::Down);
                self.composer
                    .move_or_recall_history_visually(ComposerVerticalDirection::Down, target);
            }
        }
    }

    fn composer_vertical_target(
        &self,
        area: Rect,
        direction: ComposerVerticalDirection,
    ) -> ComposerVerticalTarget {
        let input = (ShellView { shell: self }).input_area(area);
        let body = body_rect_after_title(pane_content_rect(input));
        if body.width == 0 {
            return ComposerVerticalTarget::Boundary;
        }
        let display = self.composer.display();
        composer_vertical_target(
            display.text(),
            display.cursor(),
            usize::from(body.width),
            direction,
        )
    }
}

#[cfg(test)]
#[path = "composer_navigation_tests.rs"]
mod tests;
