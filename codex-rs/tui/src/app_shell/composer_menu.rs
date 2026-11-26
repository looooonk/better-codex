//! Compact menus attached to the composer toolbar.

use super::SelectorState;
use super::effort_slider::EffortSlider;
use crate::app_shell::composer_toolbar::ComposerControl;
use crate::app_shell::design::fill_rect;
use crate::app_shell::design::palette;
use crate::app_shell::design::pane_style;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use ratatui::buffer::Buffer;
use ratatui::layout::Margin;
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

const MAX_VISIBLE_ROWS: usize = 12;
// Top border, title, bottom border.
const MENU_CHROME: u16 = 3;
const EMPTY_MENU_MESSAGE: &str = "No options available";

impl<T> SelectorState<T> {
    pub(super) fn menu_height(&self) -> u16 {
        MENU_CHROME + self.options.len().clamp(1, MAX_VISIBLE_ROWS) as u16
    }

    pub(in crate::app_shell) fn popup_size(
        &self,
        control: ComposerControl,
        available: u16,
    ) -> (u16, u16) {
        // Include both borders, side padding, and space for the active checkmark.
        let option_width = self
            .options
            .iter()
            .map(|option| option.label.width() + 7)
            .max();
        let width = match control {
            ComposerControl::ReasoningEffort => return (40.min(available), EffortSlider::HEIGHT),
            ComposerControl::Permissions => {
                let heading = self.save_error.as_deref().unwrap_or(&self.title);
                option_width
                    .unwrap_or(EMPTY_MENU_MESSAGE.width() + 4)
                    .max(heading.width() + 4)
            }
            ComposerControl::Model => option_width.unwrap_or_default().clamp(26, 44),
            ComposerControl::ServiceTier => option_width.unwrap_or_default().clamp(20, 36),
        }
        .min(usize::from(available)) as u16;
        (width, self.menu_height())
    }

    pub(super) fn menu_rows(&self, panel: Rect) -> Vec<(usize, Rect)> {
        let capacity = usize::from(panel.height.saturating_sub(MENU_CHROME));
        let first = (self.selected + 1).saturating_sub(capacity);
        let inner = panel.inner(Margin::new(/*horizontal*/ 1, /*vertical*/ 1));
        (first..self.options.len())
            .zip(inner.rows().skip(1))
            .collect()
    }

    pub(super) fn render_menu(
        &self,
        panel: Rect,
        control: Option<ComposerControl>,
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
            Some(ComposerControl::ServiceTier) => "Speed",
            _ => &self.title,
        };
        let heading = Rect::new(panel.x + 2, panel.y + 1, panel.width - 4, 1);
        Paragraph::new(self.save_error.as_deref().unwrap_or(title))
            .fg(if self.save_error.is_some() {
                palette::error()
            } else {
                palette::muted()
            })
            .render(heading, buf);

        if self.options.is_empty() {
            Paragraph::new(EMPTY_MENU_MESSAGE)
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
        let rows = self.menu_rows(panel);
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
            Paragraph::new(text)
                .fg(option.label_color())
                .render(label, buf);
            if option.current {
                buf[(row.right() - 2, row.y)]
                    .set_symbol("✓")
                    .set_fg(palette::focus());
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
