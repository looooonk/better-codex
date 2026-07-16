pub(super) const MAX_TASK_SUMMARY_CHARS: usize = 240;
pub(super) const MAX_LATEST_MESSAGE_CHARS: usize = 512;
pub(super) const MAX_MODEL_CHARS: usize = 128;

pub(super) fn concise_summary(text: &str) -> Option<String> {
    let summary = text.split_whitespace().collect::<Vec<_>>().join(" ");
    bounded_text(&summary, MAX_TASK_SUMMARY_CHARS)
}

pub(super) fn bounded_text(text: &str, max_chars: usize) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut chars = text.chars();
    let bounded = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_none() {
        Some(bounded)
    } else {
        Some(format!(
            "{}...",
            bounded
                .chars()
                .take(max_chars.saturating_sub(3))
                .collect::<String>()
        ))
    }
}

pub(super) fn append_bounded(
    current: Option<&str>,
    delta: &str,
    max_chars: usize,
) -> Option<String> {
    let combined = format!("{}{delta}", current.unwrap_or_default());
    if combined.trim().is_empty() {
        return None;
    }
    let char_count = combined.chars().count();
    if char_count <= max_chars {
        return Some(combined);
    }
    Some(format!(
        "...{}",
        combined
            .chars()
            .skip(char_count.saturating_sub(max_chars.saturating_sub(3)))
            .collect::<String>()
    ))
}
