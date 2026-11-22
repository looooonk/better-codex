use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::turn_timer::TurnMetrics;
use crate::line_truncation::line_width;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
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
        let width = usize::from(content.width);
        let project = Line::from(workspace.to_owned().bold());
        let status_budget = width.saturating_sub(line_width(&project).min(10) + 1);
        let status = if line_width(&full_status) > status_budget && self.turn_metrics.is_some() {
            let mut compact = self.turn_metrics_line();
            compact.spans.insert(0, self.status_indicator());
            // Prefer the complete timer and throughput once the status is compact.
            truncate_line_with_ellipsis_if_overflow(compact, width.saturating_sub(2))
        } else {
            truncate_line_with_ellipsis_if_overflow(full_status, status_budget)
        };
        let status_width = line_width(&status) as u16;
        let project_width = content.width.saturating_sub(status_width + 1);
        Paragraph::new(truncate_line_with_ellipsis_if_overflow(project, usize::from(project_width)))
            .render(content, buf);
        let status_area = Rect::new(content.right() - status_width, content.y, status_width, 1);
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
