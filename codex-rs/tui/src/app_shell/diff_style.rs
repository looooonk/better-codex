use super::design::palette;
use ratatui::style::Stylize;
use ratatui::text::Span;

pub(super) fn diff_stat_spans(text: String) -> Vec<Span<'static>> {
    let spans = text
        .split_inclusive(char::is_whitespace)
        .map(|segment| {
            let token = segment.trim_end_matches(char::is_whitespace);
            let color = if let Some(count) = token.strip_prefix('+') {
                is_diff_count(count).then_some(palette::SUCCESS)
            } else if let Some(count) = token.strip_prefix('-') {
                is_diff_count(count).then_some(palette::ERROR)
            } else {
                None
            };
            match color {
                Some(color) => Span::from(segment.to_string()).fg(color),
                None => segment.to_string().into(),
            }
        })
        .collect::<Vec<_>>();

    if spans.is_empty() {
        vec!["".into()]
    } else {
        spans
    }
}

fn is_diff_count(count: &str) -> bool {
    !count.is_empty()
        && count
            .chars()
            .all(|character| character.is_ascii_digit() || character == ',')
}
