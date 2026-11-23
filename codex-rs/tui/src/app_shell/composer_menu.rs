//! Compact menus attached to the composer toolbar.

use super::SelectorOption;
use super::SelectorState;
use crate::app_shell::composer_toolbar::ComposerControl;
use crate::app_shell::design::fill_rect;
use crate::app_shell::design::palette;
use crate::app_shell::design::pane_style;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

const MAX_VISIBLE_ROWS: u16 = 12;
// Top border, title, bottom border.
const MENU_CHROME: u16 = 3;

impl<T> SelectorState<T> {
    pub(in crate::app_shell) fn popup_size(
        &self,
        control: ComposerControl,
        available: u16,
    ) -> (u16, u16) {
        let max_label = self
            .options
            .iter()
            .map(|o| o.label.width())
            .max()
            .unwrap_or_default();
        let width = match control {
            ComposerControl::ReasoningEffort => return (40.min(available), 7),
            ComposerControl::Permissions => 46,
            ComposerControl::Model => (max_label + 7).clamp(26, 44) as u16,
            ComposerControl::ServiceTier => (max_label + 7).clamp(20, 36) as u16,
        }
        .min(available);
        let rows = self.options.iter().fold(0u16, |height, option| {
            height.saturating_add(option_height(option, control, width))
        });
        (width, MENU_CHROME + rows.clamp(1, MAX_VISIBLE_ROWS))
    }

    pub(super) fn menu_rows(&self, panel: Rect, control: ComposerControl) -> Vec<(usize, Rect)> {
        let capacity = panel.height.saturating_sub(MENU_CHROME);
        if capacity == 0 || self.options.is_empty() {
            return Vec::new();
        }
        let mut first = self.selected;
        let mut used = option_height(&self.options[first], control, panel.width);
        while first > 0 {
            let previous = option_height(&self.options[first - 1], control, panel.width);
            if used.saturating_add(previous) > capacity {
                break;
            }
            first -= 1;
            used += previous;
        }
        let mut y = panel.y + 2;
        let bottom = panel.bottom() - 1;
        let mut rows = Vec::new();
        for (index, option) in self.options.iter().enumerate().skip(first) {
            let remaining = bottom - y;
            let height = option_height(option, control, panel.width);
            if remaining == 0 || (height > remaining && !rows.is_empty()) {
                break;
            }
            let height = height.min(remaining);
            rows.push((
                index,
                Rect::new(panel.x + 1, y, panel.width.saturating_sub(2), height),
            ));
            y += height;
        }
        rows
    }

    pub(super) fn render_menu(
        &self,
        panel: Rect,
        control: ComposerControl,
        pointer: Option<Position>,
        buf: &mut Buffer,
    ) {
        if panel.height < MENU_CHROME || panel.width < 5 {
            return;
        }
        Clear.render(panel, buf);
        fill_rect(buf, panel, palette::elevated());
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(palette::border()))
            .style(pane_style(palette::elevated()))
            .render(panel, buf);
        let title = match control {
            ComposerControl::Permissions => "Permissions",
            ComposerControl::ServiceTier => "Speed",
            _ => &self.title,
        };
        let heading = Rect::new(panel.x + 2, panel.y + 1, panel.width - 4, 1);
        Paragraph::new(title)
            .fg(palette::muted())
            .render(heading, buf);

        if self.options.is_empty() {
            Paragraph::new("No options available")
                .fg(palette::muted())
                .render(
                    Rect {
                        y: heading.y + 1,
                        ..heading
                    },
                    buf,
                );
            return;
        }
        let rows = self.menu_rows(panel, control);
        for &(index, row) in &rows {
            let option = &self.options[index];
            if pointer.is_some_and(|p| row.contains(p)) || index == self.selected {
                fill_rect(buf, row, palette::border());
            }
            let label = Rect::new(row.x + 1, row.y, row.width.saturating_sub(4), 1);
            let text = truncate_line_with_ellipsis_if_overflow(
                Line::from(option.label.clone()),
                usize::from(label.width),
            );
            Paragraph::new(text).fg(option.label_color()).render(label, buf);
            if option.current {
                buf[(row.right() - 2, row.y)]
                    .set_symbol("✓")
                    .set_fg(palette::focus());
            }
            if row.height > 1 {
                let details = textwrap::wrap(
                    &option.detail,
                    usize::from(panel.width.saturating_sub(4)).max(1),
                )
                .into_iter()
                .map(Line::from)
                .collect::<Vec<_>>();
                Paragraph::new(details).fg(palette::muted()).render(
                    Rect::new(
                        row.x + 1,
                        row.y + 1,
                        row.width.saturating_sub(2),
                        row.height - 1,
                    ),
                    buf,
                );
            }
        }
        if let (Some(&(first, top)), Some(&(last, bottom))) = (rows.first(), rows.last()) {
            if first > 0 {
                buf[(panel.right() - 1, top.y)]
                    .set_symbol("▴")
                    .set_fg(palette::muted());
            }
            if last + 1 < self.options.len() {
                buf[(panel.right() - 1, bottom.bottom() - 1)]
                    .set_symbol("▾")
                    .set_fg(palette::muted());
            }
        }
    }
}

fn option_height<T>(option: &SelectorOption<T>, control: ComposerControl, width: u16) -> u16 {
    if control == ComposerControl::Permissions {
        let lines = textwrap::wrap(&option.detail, usize::from(width.saturating_sub(4)).max(1));
        1u16.saturating_add(u16::try_from(lines.len()).unwrap_or(u16::MAX))
    } else {
        1
    }
}
