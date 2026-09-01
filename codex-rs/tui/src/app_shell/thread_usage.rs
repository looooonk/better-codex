use codex_app_server_protocol::ThreadUsage;
use ratatui::text::Line;

const MICROS_PER_UNIT: i64 = 1_000_000;
const MICROS_PER_HUNDREDTH: i64 = 10_000;
const HUNDREDTHS_PER_UNIT: i64 = MICROS_PER_UNIT / MICROS_PER_HUNDREDTH;

pub(super) fn thread_usage_line(usage: &ThreadUsage) -> Option<Line<'static>> {
    let mut values = Vec::new();
    if usage.estimated_usage_credits_micros > 0 {
        values.push(format!(
            "{} credits",
            format_positive_micros(usage.estimated_usage_credits_micros)
        ));
    }
    if let Some(cost) = usage.estimated_usage_usd_micros.filter(|cost| *cost > 0) {
        values.push(format!("~${}", format_positive_micros(cost)));
    }
    (!values.is_empty()).then(|| Line::from(values.join(" | ")))
}

fn format_positive_micros(value: i64) -> String {
    let rounded_hundredths = value.saturating_add(MICROS_PER_HUNDREDTH / 2) / MICROS_PER_HUNDREDTH;
    if rounded_hundredths == 0 {
        return "<0.01".to_string();
    }
    let whole = rounded_hundredths / HUNDREDTHS_PER_UNIT;
    let hundredths = rounded_hundredths % HUNDREDTHS_PER_UNIT;
    match hundredths {
        0 => whole.to_string(),
        value if value % 10 == 0 => format!("{whole}.{}", value / 10),
        value => format!("{whole}.{value:02}"),
    }
}

#[cfg(test)]
#[path = "thread_usage_tests.rs"]
mod tests;
