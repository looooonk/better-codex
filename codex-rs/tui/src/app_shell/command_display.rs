use crate::text_formatting::truncate_text;

const COMMAND_PREFIX: &str = "exec ";
const MAX_COMMAND_SUMMARY_GRAPHEMES: usize = 240;

pub(super) fn summary(command: &str) -> String {
    summary_with_suffix(command, "")
}

pub(super) fn completed_summary(
    command: &str,
    exit_code: Option<i32>,
    duration_ms: Option<i64>,
) -> String {
    let mut suffix = String::new();
    if let Some(exit_code) = exit_code {
        suffix.push_str(&format!(" exit {exit_code}"));
    }
    if let Some(duration_ms) = duration_ms {
        suffix.push_str(&format!(" {duration_ms}ms"));
    }
    summary_with_suffix(command, &suffix)
}

fn summary_with_suffix(command: &str, suffix: &str) -> String {
    let command_budget = MAX_COMMAND_SUMMARY_GRAPHEMES
        .saturating_sub(COMMAND_PREFIX.len().saturating_add(suffix.len()));
    let command = truncate_text(command, command_budget);
    format!("{COMMAND_PREFIX}{command}{suffix}")
}
