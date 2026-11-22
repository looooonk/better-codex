use super::ShellState;
use super::design::palette;
use super::design::pane_content_rect;
use super::settings::approval_policy_label;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use codex_app_server_protocol::AskForApproval;
use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
use codex_protocol::config_types::ServiceTier;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;
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
    color: Color,
}

pub(super) struct ComposerToolbar {
    buttons: [Button; 4],
}

pub(super) fn toolbar_rows(content: Rect) -> u16 {
    if content.width < 48 && content.height >= 3 {
        2
    } else {
        content.height.min(1)
    }
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
        let stacked = toolbar_rows(content) == 2;
        let preset = shell
            .available_models
            .iter()
            .find(|preset| preset.model == shell.model);
        let effort = shell
            .reasoning_effort
            .as_ref()
            .or_else(|| preset.map(|preset| &preset.default_reasoning_effort))
            .map(ToString::to_string)
            .unwrap_or_else(|| "Default".to_owned());
        let speed = match shell.service_tier.as_deref() {
            None | Some("" | SERVICE_TIER_DEFAULT_REQUEST_VALUE) => "Default",
            Some(id) if ServiceTier::from_request_value(id) == Some(ServiceTier::Fast) => "Fast",
            Some(id) => preset
                .and_then(|preset| preset.service_tiers.iter().find(|tier| tier.id == id))
                .map(|tier| tier.name.as_str())
                .filter(|name| !name.is_empty())
                .unwrap_or(id),
        };
        let permissions_color = if shell.approval_policy == AskForApproval::Never {
            palette::error()
        } else {
            palette::purple()
        };
        let mut buttons = [
            (
                ComposerControl::Permissions,
                approval_policy_label(shell.approval_policy),
                16,
                permissions_color,
            ),
            (
                ComposerControl::Model,
                shell.model.as_str(),
                20,
                palette::text(),
            ),
            (
                ComposerControl::ReasoningEffort,
                effort.as_str(),
                12,
                palette::purple(),
            ),
            (ComposerControl::ServiceTier, speed, 12, palette::purple()),
        ]
        .map(|(control, value, limit, color)| Button {
            control,
            area: Rect::default(),
            label: truncate_line_with_ellipsis_if_overflow(value.to_owned().into(), limit),
            color,
        });
        // Keep every dropdown reachable. Give the model name the remaining space.
        let suffix = match (compact, stacked) {
            (true, false) => "▾",
            (true, true) => " ▾",
            (false, _) => " ▾ ",
        };
        let decoration = suffix.chars().count() + usize::from(!compact);
        let group = if stacked { &buttons[1..] } else { &buttons[..] };
        let reserved = group
            .iter()
            .filter(|b| b.control != ComposerControl::Model)
            .map(|b| b.label.width())
            .sum::<usize>()
            + group.len() * decoration
            + group.len()
            - 1;
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
            button.label.spans.push(suffix.into());
            let width = button.label.width() as u16;
            let x = if button.control == ComposerControl::Permissions {
                row.x
            } else {
                right.saturating_sub(width)
            };
            let mut button_row = row;
            if stacked && button.control == ComposerControl::Permissions {
                button_row.y -= 1;
            }
            button.area =
                Rect::new(x, button_row.y, width, button_row.height).intersection(button_row);
            right = x.saturating_sub(1);
        }
        Self { buttons }
    }

    pub(super) fn button_area(&self, control: ComposerControl) -> Rect {
        self.buttons
            .iter()
            .find(|button| button.control == control)
            .map_or(Rect::default(), |button| button.area)
    }

    pub(super) fn control_at(&self, position: Position) -> Option<ComposerControl> {
        self.buttons
            .iter()
            .find(|button| button.area.contains(position))
            .map(|button| button.control)
    }

    pub(super) fn render(self, pointer: Option<Position>, buf: &mut Buffer) {
        for button in self.buttons {
            let hovered = pointer.is_some_and(|position| button.area.contains(position));
            Paragraph::new(button.label)
                .fg(button.color)
                .bg(if hovered {
                    palette::border()
                } else {
                    palette::elevated()
                })
                .render(button.area, buf);
        }
    }
}
