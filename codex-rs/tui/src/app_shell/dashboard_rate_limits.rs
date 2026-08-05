use super::dashboard::dashboard_value;
use super::dashboard::format_i64;
use super::design::palette;
use codex_app_server_protocol::RateLimitSnapshot;
use ratatui::style::Color;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

const USAGE_BAR_SEGMENTS: usize = 10;
const PERCENT_WIDTH: usize = 4;
const MIN_USED_PERCENT: i32 = 0;
const MAX_USED_PERCENT: i32 = 100;

pub(super) fn rate_limit_lines(
    limit: &RateLimitSnapshot,
    width: usize,
    current_time_at: i64,
) -> Vec<Line<'static>> {
    let label = limit
        .limit_name
        .as_deref()
        .or(limit.limit_id.as_deref())
        .unwrap_or("account");
    let mut lines = [limit.primary.as_ref(), limit.secondary.as_ref()]
        .into_iter()
        .flatten()
        .map(|window| {
            let used_percent = window
                .used_percent
                .clamp(MIN_USED_PERCENT, MAX_USED_PERCENT);
            let usage_color = usage_color(used_percent);
            let time_left = format_time_left(window.resets_at, current_time_at);
            let label_reserve = PERCENT_WIDTH
                .saturating_add(USAGE_BAR_SEGMENTS)
                .saturating_add(time_left.len())
                .saturating_add(3);
            let label = dashboard_value(label, width, label_reserve);
            let percent = format!("{used_percent}%");
            let mut spans = vec![
                percent.clone().fg(usage_color),
                " ".repeat(PERCENT_WIDTH.saturating_sub(percent.len()) + 1)
                    .into(),
            ];
            spans.extend(usage_bar_spans(used_percent, usage_color));
            spans.extend([" ".into(), Span::from(label), " ".dim(), time_left.dim()]);
            Line::from(spans)
        })
        .collect::<Vec<_>>();

    let mut details = Vec::new();
    if let Some(reached) = limit.rate_limit_reached_type {
        details.push(format!("limited {reached:?}").fg(palette::error()));
    }
    if let Some(individual_limit) = &limit.individual_limit {
        if !details.is_empty() {
            details.push(" | ".dim());
        }
        details.push(format!("spend {}% left", individual_limit.remaining_percent).into());
    }
    if !details.is_empty() {
        lines.push(Line::from_iter(std::iter::once("  ".into()).chain(details)));
    }
    lines
}

pub(super) fn credits_and_resets_line(
    limits: &[RateLimitSnapshot],
    reset_credits: Option<i64>,
) -> Line<'static> {
    let credits = limits.iter().find_map(|limit| limit.credits.as_ref());
    let mut spans = vec!["credits ".dim()];
    match credits {
        Some(credits) => {
            if credits.unlimited {
                spans.push("unlimited".fg(palette::success()));
            } else if let Some(balance) = &credits.balance {
                spans.push(balance.clone().into());
            } else if !credits.has_credits {
                spans.push("depleted".fg(palette::error()));
            } else {
                spans.push("available".fg(palette::success()));
            }
        }
        None => spans.push("unavailable".dim()),
    }
    spans.extend([" | ".dim(), "resets ".dim()]);
    match reset_credits {
        Some(reset_credits) => spans.push(format_i64(reset_credits).into()),
        None => spans.push("unavailable".dim()),
    }
    Line::from(spans)
}

fn usage_color(used_percent: i32) -> Color {
    if used_percent >= 90 {
        palette::error()
    } else if used_percent >= 75 {
        palette::purple()
    } else if used_percent >= 50 {
        palette::cyan()
    } else {
        palette::success()
    }
}

fn format_time_left(resets_at: Option<i64>, current_time_at: i64) -> String {
    let Some(resets_at) = resets_at else {
        return "unknown".to_string();
    };
    let total_hours = resets_at.saturating_sub(current_time_at).max(0) / (60 * 60);
    let days = total_hours / 24;
    let hours = total_hours % 24;
    if days > 0 {
        format!("{days}d {hours}h")
    } else {
        format!("{hours}h")
    }
}

fn usage_bar_spans(used_percent: i32, usage_color: Color) -> [Span<'static>; 2] {
    let used_percent = used_percent.clamp(MIN_USED_PERCENT, MAX_USED_PERCENT);
    let filled_segments =
        usize::try_from(used_percent).unwrap_or_default() * USAGE_BAR_SEGMENTS / 100;
    let empty_segments = USAGE_BAR_SEGMENTS.saturating_sub(filled_segments);
    [
        "█".repeat(filled_segments).fg(usage_color),
        "░".repeat(empty_segments).fg(palette::border()),
    ]
}

#[cfg(test)]
#[path = "dashboard_rate_limits_tests.rs"]
mod tests;
