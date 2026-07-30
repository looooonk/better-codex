use super::dashboard::dashboard_value;
use super::dashboard::format_i64;
use super::design::palette;
use codex_app_server_protocol::RateLimitSnapshot;
use ratatui::style::Color;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

const USAGE_BAR_SEGMENTS: usize = 10;
const BLOCK_STEPS_PER_SEGMENT: usize = 8;
const TOTAL_BAR_BLOCK_STEPS: i32 = 80;
const FRACTIONAL_BLOCKS: [&str; BLOCK_STEPS_PER_SEGMENT] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
const MIN_USED_PERCENT: i32 = 0;
const MAX_USED_PERCENT: i32 = 100;

pub(super) fn rate_limit_lines(limit: &RateLimitSnapshot, width: usize) -> Vec<Line<'static>> {
    let windows = [limit.primary.as_ref(), limit.secondary.as_ref()]
        .into_iter()
        .flatten()
        .map(|window| {
            let usage_color = if window.used_percent >= 90 {
                palette::error()
            } else if window.used_percent >= 75 {
                palette::purple()
            } else if window.used_percent >= 50 {
                palette::cyan()
            } else {
                palette::success()
            };
            let mut suffix = vec![format!("{}%", window.used_percent).fg(usage_color)];
            if let Some(duration) = window.window_duration_mins {
                let duration = duration.max(0);
                let duration = if duration == 0 {
                    "0m".to_string()
                } else if duration % (24 * 60) == 0 {
                    format!("{}d", format_i64(duration / (24 * 60)))
                } else if duration % 60 == 0 {
                    format!("{}h", format_i64(duration / 60))
                } else {
                    format!("{}m", format_i64(duration))
                };
                suffix.extend([" ".dim(), duration.dim()]);
            }
            (window.used_percent, usage_color, suffix)
        })
        .collect::<Vec<_>>();
    let compact_windows_width = windows
        .iter()
        .enumerate()
        .map(|(index, (_, _, suffix))| {
            let separator = if index == 0 { " " } else { " | " };
            separator
                .len()
                .saturating_add(Line::from(suffix.clone()).width())
        })
        .sum::<usize>();
    let expanded_windows_width = compact_windows_width.saturating_add(
        windows
            .len()
            .saturating_mul(USAGE_BAR_SEGMENTS.saturating_add(1)),
    );
    let label = limit
        .limit_name
        .as_deref()
        .or(limit.limit_id.as_deref())
        .unwrap_or("account");
    let show_usage_bars = Line::from(label)
        .width()
        .saturating_add(expanded_windows_width)
        <= width;
    let selected_windows_width = if show_usage_bars {
        expanded_windows_width
    } else {
        compact_windows_width
    };
    let first_window_width = windows.first().map_or(0, |(_, _, suffix)| {
        " ".len()
            .saturating_add(Line::from(suffix.clone()).width())
            .saturating_add(if show_usage_bars {
                USAGE_BAR_SEGMENTS.saturating_add(1)
            } else {
                0
            })
    });
    let label_reserve = if selected_windows_width.saturating_add(1) <= width {
        selected_windows_width
    } else {
        first_window_width
    };
    let label = dashboard_value(label, width, label_reserve);
    let mut lines = Vec::new();
    let mut spans = vec![Span::from(label)];
    for (index, (used_percent, usage_color, suffix)) in windows.into_iter().enumerate() {
        let mut window_spans = Vec::new();
        if show_usage_bars {
            window_spans.extend(usage_bar_spans(used_percent, usage_color));
            window_spans.push(" ".dim());
        }
        window_spans.extend(suffix);

        let separator = if index == 0 { " " } else { " | " };
        let combined_width = Line::from(spans.clone())
            .width()
            .saturating_add(separator.len())
            .saturating_add(Line::from(window_spans.clone()).width());
        if index > 0 && combined_width > width {
            lines.push(Line::from(spans));
            spans = vec!["  ".into()];
            spans.extend(window_spans);
        } else {
            spans.push(separator.dim());
            spans.extend(window_spans);
        }
    }
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
    if details.is_empty() {
        lines.push(Line::from(spans));
        return lines;
    }

    let inline_width = Line::from(spans.clone())
        .width()
        .saturating_add(" | ".len())
        .saturating_add(Line::from(details.clone()).width());
    if inline_width <= width {
        spans.push(" | ".dim());
        spans.extend(details);
        lines.push(Line::from(spans));
    } else {
        lines.push(Line::from(spans));
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

fn usage_bar_spans(used_percent: i32, usage_color: Color) -> [Span<'static>; 3] {
    let used_percent = used_percent.clamp(MIN_USED_PERCENT, MAX_USED_PERCENT);
    let filled_block_steps = usize::try_from(
        (used_percent * TOTAL_BAR_BLOCK_STEPS + MAX_USED_PERCENT / 2) / MAX_USED_PERCENT,
    )
    .unwrap_or_default();
    let filled_segments = filled_block_steps / BLOCK_STEPS_PER_SEGMENT;
    let fractional_block = filled_block_steps % BLOCK_STEPS_PER_SEGMENT;
    let empty_segments = USAGE_BAR_SEGMENTS
        .saturating_sub(filled_segments.saturating_add(usize::from(fractional_block > 0)));
    [
        "█".repeat(filled_segments).fg(usage_color),
        FRACTIONAL_BLOCKS[fractional_block].fg(usage_color),
        "░".repeat(empty_segments).fg(palette::border()),
    ]
}

#[cfg(test)]
#[path = "dashboard_rate_limits_tests.rs"]
mod tests;
