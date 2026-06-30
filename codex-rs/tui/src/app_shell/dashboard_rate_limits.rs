use super::dashboard::dashboard_value;
use super::dashboard::format_i64;
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::RateLimitWindow;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(super) fn rate_limit_lines(limit: &RateLimitSnapshot, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let label = rate_limit_label(limit, width);
    if let Some(primary) = &limit.primary {
        lines.push(rate_limit_window_line(&label, primary));
    } else {
        lines.push(Line::from(label));
    }
    if let Some(secondary) = &limit.secondary {
        lines.push(rate_limit_window_line("  secondary", secondary));
    }
    if let Some(reached) = limit.rate_limit_reached_type {
        lines.push(Line::from(format!("limited {reached:?}").red()));
    }
    if let Some(credits) = &limit.credits {
        if credits.unlimited {
            lines.push(Line::from("credits unlimited".green()));
        } else if let Some(balance) = &credits.balance {
            lines.push(Line::from(format!("credits {balance}").dim()));
        } else if !credits.has_credits {
            lines.push(Line::from("credits depleted".red()));
        }
    }
    if let Some(individual_limit) = &limit.individual_limit {
        lines.push(Line::from(format!(
            "spend {}% left",
            individual_limit.remaining_percent
        )));
    }
    lines
}

fn rate_limit_label(limit: &RateLimitSnapshot, width: usize) -> String {
    limit
        .limit_name
        .as_deref()
        .or(limit.limit_id.as_deref())
        .map(|label| dashboard_value(label, width, /*prefix_width*/ 10))
        .unwrap_or_else(|| "account".to_string())
}

fn rate_limit_window_line(label: &str, window: &RateLimitWindow) -> Line<'static> {
    let percent = format!("{}%", window.used_percent);
    let percent = if window.used_percent >= 90 {
        percent.red()
    } else if window.used_percent >= 75 {
        percent.magenta()
    } else if window.used_percent >= 50 {
        percent.cyan()
    } else {
        percent.green()
    };
    let mut spans = vec![label.to_string().into(), " ".dim(), percent];
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
    Line::from(spans)
}
