use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use crate::line_truncation::line_width;
use crate::text_formatting::truncate_text;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

const CONTROL_GAP: u16 = 1;
const BRAND_CONTROL_GAP: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HeaderControl {
    Dashboard,
    Model,
    ReasoningEffort,
}

pub(super) struct HeaderView<'a> {
    pub(super) cwd: &'a str,
    pub(super) model: &'a str,
    pub(super) reasoning_effort: &'a str,
    pub(super) status: &'a str,
    pub(super) dashboard_visible: bool,
}

impl HeaderView<'_> {
    pub(super) fn render(&self, area: Rect, hovered: Option<HeaderControl>, buf: &mut Buffer) {
        fill_rect(buf, area, palette::DARK);
        let content = pane_content_rect(area);
        let layout = self.control_layout(area);
        Paragraph::new(self.brand_line(layout.is_some_and(|layout| layout.compact_brand)))
            .style(pane_style(palette::DARK))
            .render(Rect::new(content.x, content.y, content.width, 1), buf);

        if let Some(layout) = layout {
            if let Some(dashboard) = layout.dashboard {
                Paragraph::new(self.dashboard_label())
                    .style(
                        Style::new()
                            .fg(palette::CYAN)
                            .bg(control_background(hovered, HeaderControl::Dashboard)),
                    )
                    .render(dashboard, buf);
            }
            if let Some(model) = layout.model {
                Paragraph::new(self.model_label())
                    .style(
                        Style::new()
                            .fg(palette::TEXT)
                            .bg(control_background(hovered, HeaderControl::Model)),
                    )
                    .render(model, buf);
            }
            if let Some(effort) = layout.effort {
                Paragraph::new(self.effort_label())
                    .style(
                        Style::new()
                            .fg(palette::PURPLE)
                            .bg(control_background(hovered, HeaderControl::ReasoningEffort)),
                    )
                    .render(effort, buf);
            }
            if let Some(status) = layout.status {
                Paragraph::new(self.status_line())
                    .style(pane_style(palette::DARK))
                    .render(status, buf);
            }
        }

        Paragraph::new(
            Line::from("─".repeat(usize::from(area.width)))
                .style(Style::new().fg(palette::BORDER).bg(palette::DARK)),
        )
        .render(
            Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
            buf,
        );
    }

    pub(super) fn control_at(&self, area: Rect, position: Position) -> Option<HeaderControl> {
        let layout = self.control_layout(area)?;
        if layout.dashboard.is_some_and(|area| area.contains(position)) {
            Some(HeaderControl::Dashboard)
        } else if layout.model.is_some_and(|area| area.contains(position)) {
            Some(HeaderControl::Model)
        } else if layout.effort.is_some_and(|area| area.contains(position)) {
            Some(HeaderControl::ReasoningEffort)
        } else {
            None
        }
    }

    fn control_layout(&self, area: Rect) -> Option<HeaderControlLayout> {
        let content = pane_content_rect(area);
        let model_width = text_width(&self.model_label());
        let effort_width = text_width(&self.effort_label());
        let dashboard_width = if self.dashboard_visible {
            None
        } else {
            Some(u16::try_from(text_width(&self.dashboard_label())).ok()?)
        };
        let status_width = line_width(&self.status_line());
        let model_width = u16::try_from(model_width).ok()?;
        let effort_width = u16::try_from(effort_width).ok()?;
        let status_width = u16::try_from(status_width).ok()?;
        for (compact_brand, include_status) in [(false, true), (true, true), (true, false)] {
            let status_and_gap = if include_status {
                status_width.saturating_add(CONTROL_GAP)
            } else {
                0
            };
            let dashboard_and_gap = dashboard_width
                .map(|width| width.saturating_add(CONTROL_GAP))
                .unwrap_or_default();
            let controls_width = dashboard_and_gap
                .saturating_add(model_width)
                .saturating_add(CONTROL_GAP)
                .saturating_add(effort_width)
                .saturating_add(status_and_gap);
            let required_width = line_width(&self.brand_line(compact_brand))
                .saturating_add(usize::from(BRAND_CONTROL_GAP))
                .saturating_add(usize::from(controls_width));
            if required_width > usize::from(content.width) {
                continue;
            }
            let controls_x = content.right().saturating_sub(controls_width);
            let dashboard = dashboard_width.map(|width| Rect::new(controls_x, content.y, width, 1));
            let model_x = controls_x.saturating_add(dashboard_and_gap);
            let effort_x = model_x
                .saturating_add(model_width)
                .saturating_add(CONTROL_GAP);
            let status = include_status.then(|| {
                Rect::new(
                    effort_x
                        .saturating_add(effort_width)
                        .saturating_add(CONTROL_GAP),
                    content.y,
                    status_width,
                    1,
                )
            });
            return Some(HeaderControlLayout {
                dashboard,
                model: Some(Rect::new(model_x, content.y, model_width, 1)),
                effort: Some(Rect::new(effort_x, content.y, effort_width, 1)),
                status,
                compact_brand,
            });
        }
        if let Some(width) = dashboard_width {
            let compact_brand = true;
            let required_width = line_width(&self.brand_line(compact_brand))
                .saturating_add(usize::from(BRAND_CONTROL_GAP))
                .saturating_add(usize::from(width));
            if required_width <= usize::from(content.width) {
                return Some(HeaderControlLayout {
                    dashboard: Some(Rect::new(
                        content.right().saturating_sub(width),
                        content.y,
                        width,
                        1,
                    )),
                    model: None,
                    effort: None,
                    status: None,
                    compact_brand,
                });
            }
        }
        None
    }

    fn brand_line(&self, compact: bool) -> Line<'static> {
        if compact {
            return Line::from(vec![
                "◆".fg(palette::PURPLE).bold(),
                " BC".fg(palette::TEXT).bold(),
            ]);
        }
        let workspace = std::path::Path::new(self.cwd)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(self.cwd);
        Line::from(vec![
            "◆".fg(palette::PURPLE).bold(),
            " BETTER CODEX".fg(palette::TEXT).bold(),
            "  ".into(),
            truncate_text(workspace, /*max_graphemes*/ 18).fg(palette::MUTED),
        ])
    }

    fn model_label(&self) -> String {
        format!(" {} ▾ ", truncate_text(self.model, /*max_graphemes*/ 18))
    }

    fn dashboard_label(&self) -> String {
        " Panels ".to_string()
    }

    fn effort_label(&self) -> String {
        format!(
            " {} ▾ ",
            truncate_text(self.reasoning_effort, /*max_graphemes*/ 12)
        )
    }

    fn status_line(&self) -> Line<'static> {
        let color = match self.status {
            "ready" => palette::SUCCESS,
            "failed" | "error" | "disconnected" => palette::ERROR,
            "interrupted" => palette::WARNING,
            _ => palette::CYAN,
        };
        Line::from(vec![
            "● ".fg(color),
            self.status.to_string().fg(palette::TEXT).bold(),
        ])
    }
}

fn control_background(hovered: Option<HeaderControl>, control: HeaderControl) -> Color {
    if hovered == Some(control) {
        palette::BORDER
    } else {
        palette::ELEVATED
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HeaderControlLayout {
    dashboard: Option<Rect>,
    model: Option<Rect>,
    effort: Option<Rect>,
    status: Option<Rect>,
    compact_brand: bool,
}

fn text_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

#[cfg(test)]
#[path = "header_tests.rs"]
mod tests;
