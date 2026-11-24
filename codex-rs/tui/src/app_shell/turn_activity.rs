//! Animated status at the live end of the conversation.

use super::ShellState;
use super::transcript_view::ACTIVITY_INDENT;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::terminal_palette::rgb_color;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::time::Duration;

impl ShellState {
    pub(super) fn render_turn_activity(
        &self,
        body: Rect,
        activity_row: usize,
        visible_from: usize,
        buf: &mut Buffer,
    ) {
        let Some(rate) = self.active_turn_tps() else {
            return;
        };
        let Some(row) = activity_row
            .checked_sub(visible_from)
            .and_then(|offset| body.rows().nth(offset))
        else {
            return;
        };
        let area = Rect::new(
            row.x.saturating_add(ACTIVITY_INDENT),
            row.y,
            row.width.saturating_sub(ACTIVITY_INDENT),
            row.height,
        );
        let elapsed = self
            .turn_started_at
            .filter(|_| self.animations && self.shimmer_enabled)
            .map(|started| started.elapsed());
        let status = if self.compaction_item_id.is_some() {
            "compacting"
        } else {
            match self.status.as_str() {
                "" | "ready" => "working",
                status => status,
            }
        };
        render_activity(status, rate, elapsed, area, buf);
    }
}

fn render_activity(
    status: &str,
    rate: u64,
    elapsed: Option<Duration>,
    area: Rect,
    buf: &mut Buffer,
) {
    if area.is_empty() {
        return;
    }
    let mut chars = status.trim_end_matches(['.', '…']).chars();
    let first = chars
        .next()
        .map(|ch| ch.to_uppercase().to_string())
        .unwrap_or_default();
    let label = Line::from(format!("{first}{}...", chars.as_str()));
    let suffix = format!(" · {rate} tok/s");
    let label_budget = usize::from(area.width).saturating_sub(suffix.chars().count());
    let label_width = label.width();
    let dots = label_width - 3..label_width.min(label_budget);
    let mut line = truncate_line_with_ellipsis_if_overflow(label, label_budget);
    line.spans.push(suffix.into());
    let width = line.width().min(usize::from(area.width)) as u16;
    let gray = 152;
    Paragraph::new(line)
        .fg(rgb_color((gray, gray, gray)))
        .render(area, buf);

    if let Some(elapsed) = elapsed {
        // A soft white band travels left to right, then clears the final glyph
        // before the next pass begins.
        let radius = 4.0;
        let center = (elapsed.as_secs_f64() * 15.0) % (f64::from(width) + 3.0 * radius) - radius;
        // Terminals can shape "..." as a ligature. Give all three periods the
        // same foreground so a passing highlight never splits that glyph run.
        for column in 0..width {
            let sample = if dots.contains(&usize::from(column)) {
                (dots.start + 1) as f64
            } else {
                f64::from(column)
            };
            let intensity = (1.0 - (sample - center).abs() / radius).max(0.0);
            if intensity > 0.0 {
                let shade = (f64::from(gray) + 96.0 * intensity).round() as u8;
                buf[(area.x + column, area.y)].set_fg(rgb_color((shade, shade, shade)));
            }
        }
    }
}

#[cfg(test)]
#[path = "turn_activity_tests.rs"]
mod tests;
