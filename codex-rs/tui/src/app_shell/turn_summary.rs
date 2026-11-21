use super::ShellState;
use super::TranscriptKind;
use super::TranscriptLine;
use codex_utils_elapsed::format_duration;
use std::time::Duration;

pub(super) fn turn_duration(
    duration_ms: Option<i64>,
    started_at: Option<i64>,
    completed_at: Option<i64>,
) -> Option<Duration> {
    duration_ms
        .and_then(|ms| u64::try_from(ms).ok())
        .map(Duration::from_millis)
        .or_else(|| {
            let seconds = completed_at?.checked_sub(started_at?)?;
            Some(Duration::from_secs(u64::try_from(seconds).ok()?))
        })
}

impl ShellState {
    pub(super) fn push_turn_separator(&mut self, turn_id: &str, duration: Option<Duration>) {
        let output = self.turn_outputs.remove(turn_id);
        let label = duration
            .map(|duration| {
                let time = format_duration(duration);
                if let Some(output) = output
                    && !duration.is_zero()
                {
                    let rate = output as f64 / duration.as_secs_f64();
                    format!("Took {time} · {rate:.1} tok/s")
                } else {
                    format!("Took {time}")
                }
            })
            .unwrap_or_default();
        self.upsert_line(TranscriptLine::new(TranscriptKind::Separator, label).item_id(turn_id));
    }
}

#[cfg(test)]
#[path = "turn_summary_tests.rs"]
mod tests;
