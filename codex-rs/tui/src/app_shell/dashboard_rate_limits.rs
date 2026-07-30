use super::dashboard::dashboard_value;
use super::dashboard::format_i64;
use super::design::palette;
use codex_app_server_protocol::RateLimitSnapshot;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

pub(super) fn rate_limit_line(limit: &RateLimitSnapshot, width: usize) -> Line<'static> {
    let label = rate_limit_label(limit, width);
    let mut spans = vec![Span::from(label)];
    for (index, window) in [limit.primary.as_ref(), limit.secondary.as_ref()]
        .into_iter()
        .flatten()
        .enumerate()
    {
        spans.push(if index == 0 { " ".dim() } else { " | ".dim() });
        let percent = format!("{}%", window.used_percent);
        spans.push(if window.used_percent >= 90 {
            percent.fg(palette::error())
        } else if window.used_percent >= 75 {
            percent.fg(palette::purple())
        } else if window.used_percent >= 50 {
            percent.fg(palette::cyan())
        } else {
            percent.fg(palette::success())
        });
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
            spans.extend([" ".dim(), duration.dim()]);
        }
    }
    if let Some(reached) = limit.rate_limit_reached_type {
        spans.extend([
            " | ".dim(),
            format!("limited {reached:?}").fg(palette::error()),
        ]);
    }
    if let Some(individual_limit) = &limit.individual_limit {
        spans.extend([
            " | ".dim(),
            format!("spend {}% left", individual_limit.remaining_percent).into(),
        ]);
    }
    Line::from(spans)
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

fn rate_limit_label(limit: &RateLimitSnapshot, width: usize) -> String {
    limit
        .limit_name
        .as_deref()
        .or(limit.limit_id.as_deref())
        .map(|label| dashboard_value(label, width, /*prefix_width*/ 10))
        .unwrap_or_else(|| "account".to_string())
}
