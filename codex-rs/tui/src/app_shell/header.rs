use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::turn_timer::TurnMetrics;
use crate::line_truncation::line_width;
use crate::text_formatting::truncate_text;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

const STATUS_SPINNER_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];

pub(super) struct HeaderView<'a> {
    pub(super) cwd: &'a str,
    pub(super) status: &'a str,
    pub(super) status_spinner_frame: Option<usize>,
    pub(super) turn_metrics: Option<TurnMetrics>,
}

impl HeaderView<'_> {
    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, palette::dark());
        let content = pane_content_rect(area);
        let full_status = self.status_line();
        let workspace = std::path::Path::new(self.cwd)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(self.cwd);
        let full_brand = Line::from(vec![
            "BETTER CODEX".bold(),
            "  ".into(),
            truncate_text(workspace, /*max_graphemes*/ 18).fg(palette::muted()),
        ]);
        let width = usize::from(content.width);
        let status_width = line_width(&full_status);
        let brand = if line_width(&full_brand) + 2 + status_width <= width {
            full_brand
        } else if 13 + status_width <= width {
            "BETTER CODEX".bold().into()
        } else {
            "BC".bold().into()
        };
        let brand_width = line_width(&brand) as u16;
        Paragraph::new(brand).render(content, buf);
        let status_area = Rect::new(
            content.x + brand_width + 1,
            content.y,
            content.width.saturating_sub(brand_width + 1),
            1,
        );
        let status = if status_width > usize::from(status_area.width) && self.turn_metrics.is_some()
        {
            let mut compact = self.turn_metrics_line();
            compact.spans.insert(0, self.status_indicator());
            compact
        } else {
            full_status
        };
        Paragraph::new(status)
            .right_aligned()
            .render(status_area, buf);

        Paragraph::new("─".repeat(usize::from(area.width)))
            .fg(palette::border())
            .render(
                Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
                buf,
            );
    }

    fn status_indicator(&self) -> Span<'static> {
        let color = match self.status {
            "ready" => palette::success(),
            "failed" | "error" | "disconnected" => palette::error(),
            "interrupted" => palette::warning(),
            _ => palette::cyan(),
        };
        let indicator = self
            .status_spinner_frame
            .map(|frame| STATUS_SPINNER_FRAMES[frame % STATUS_SPINNER_FRAMES.len()])
            .unwrap_or("●");
        format!("{indicator} ").fg(color)
    }

    fn status_line(&self) -> Line<'static> {
        let mut spans = vec![
            self.status_indicator(),
            self.status.to_string().fg(palette::text()).bold(),
        ];
        if self.turn_metrics.is_some() {
            spans.push(" · ".fg(palette::muted()));
            spans.extend(self.turn_metrics_line().spans);
        }
        Line::from(spans)
    }

    fn turn_timer_line(&self) -> Line<'static> {
        let Some(metrics) = &self.turn_metrics else {
            return Line::default();
        };
        Line::from(vec![
            "turn ".fg(palette::muted()),
            format_turn_elapsed(metrics.elapsed_seconds)
                .fg(palette::text())
                .bold(),
        ])
    }

    fn turn_metrics_line(&self) -> Line<'static> {
        let mut line = self.turn_timer_line();
        if let Some(metrics) = &self.turn_metrics {
            line.spans.extend([
                " · ".fg(palette::muted()),
                metrics
                    .tokens_per_second
                    .to_string()
                    .fg(palette::text())
                    .bold(),
                " tok/s".fg(palette::muted()),
            ]);
        }
        line
    }
}

fn format_turn_elapsed(seconds: u64) -> String {
    let hours = seconds / 3_600;
    let minutes = seconds % 3_600 / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
#[path = "header_tests.rs"]
mod tests;
