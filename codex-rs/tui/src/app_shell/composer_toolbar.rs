use super::ShellState;
use super::design::palette;
use super::design::pane_content_rect;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposerControl {
    Permissions,
    Model,
    ReasoningEffort,
    ServiceTier,
}

struct Button {
    control: ComposerControl,
    area: Rect,
    label: Line<'static>,
}

pub(super) struct ComposerToolbar {
    buttons: [Button; 4],
    row: Rect,
}

impl ComposerToolbar {
    pub(super) fn new(shell: &ShellState, input: Rect) -> Self {
        let content = pane_content_rect(input);
        let row = Rect::new(
            content.x,
            content.bottom().saturating_sub(1),
            content.width,
            content.height.min(1),
        );
        let compact = row.width < 58;
        let permissions = if row.width < 44 { "Perms" } else { "Permissions" };
        let effort = shell
            .reasoning_effort
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "Effort".to_string());
        let speed = shell
            .service_tier
            .as_deref()
            .filter(|tier| !tier.is_empty() && *tier != "default")
            .unwrap_or("Speed");
        let mut buttons = [
            (ComposerControl::Permissions, permissions, 11),
            (ComposerControl::Model, shell.model.as_str(), 20),
            (
                ComposerControl::ReasoningEffort,
                effort.as_str(),
                if row.width < 60 { 6 } else { 12 },
            ),
            (
                ComposerControl::ServiceTier,
                speed,
                if row.width < 60 { 5 } else { 12 },
            ),
        ]
        .map(|(control, value, limit)| Button {
            control,
            area: Rect::default(),
            label: truncate_line_with_ellipsis_if_overflow(value.to_owned().into(), limit),
        });
        // Keep every dropdown reachable. Give the model name the remaining space.
        let decoration = if compact { 2 } else { 4 };
        let reserved = buttons
            .iter()
            .filter(|b| b.control != ComposerControl::Model)
            .map(|b| b.label.width())
            .sum::<usize>()
            + 4 * decoration
            + 3;
        let model_width = usize::from(row.width).saturating_sub(reserved);
        buttons[1].label = truncate_line_with_ellipsis_if_overflow(
            std::mem::take(&mut buttons[1].label),
            model_width,
        );
        let mut right = row.right();
        for button in buttons.iter_mut().rev() {
            if !compact {
                button.label.spans.insert(0, " ".into());
            }
            button
                .label
                .spans
                .push(if compact { " ▾" } else { " ▾ " }.into());
            let width = button.label.width() as u16;
            let x = if button.control == ComposerControl::Permissions {
                row.x
            } else {
                right.saturating_sub(width)
            };
            button.area = Rect::new(x, row.y, width, row.height).intersection(row);
            right = x.saturating_sub(1);
        }
        Self { buttons, row }
    }

    pub(super) fn control_at(&self, position: Position) -> Option<ComposerControl> {
        self.buttons
            .iter()
            .find(|button| button.area.contains(position))
            .map(|button| button.control)
    }

    pub(super) fn render(self, shell: &ShellState, pointer: Option<Position>, buf: &mut Buffer) {
        let effort = self.buttons[2].area;
        let ripple_areas = [self.buttons[1].area, effort, self.buttons[3].area];
        for button in self.buttons {
            let hovered = pointer.is_some_and(|position| button.area.contains(position));
            let color = match button.control {
                ComposerControl::Model | ComposerControl::Permissions => palette::text(),
                ComposerControl::ReasoningEffort | ComposerControl::ServiceTier => {
                    palette::purple()
                }
            };
            Paragraph::new(button.label)
                .fg(color)
                .bg(if hovered {
                    palette::border()
                } else {
                    palette::elevated()
                })
                .render(button.area, buf);
        }
        if let Some(frame) = shell
            .reasoning_ripple
            .as_ref()
            .and_then(|ripple| ripple.frame(std::time::Instant::now()))
        {
            frame.render(self.row, effort, ripple_areas, buf);
        }
    }
}
