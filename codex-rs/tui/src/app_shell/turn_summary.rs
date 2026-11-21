use super::ShellState;
use super::TranscriptKind;
use super::TranscriptLine;
use super::output_rate::estimate_tokens;
use codex_app_server_protocol::ThreadItemPayload;
use codex_utils_elapsed::format_duration;
use std::collections::HashMap;
use std::time::Duration;

/// Visible output for one unfinished turn. A finalized item replaces its streamed estimate.
#[derive(Default)]
pub(super) struct TurnOutput {
    tokens_by_item: HashMap<String, f64>,
}

impl TurnOutput {
    pub(super) fn add_delta(&mut self, item_id: &str, tokens: f64) {
        if let Some(count) = self.tokens_by_item.get_mut(item_id) {
            *count += tokens;
        } else {
            self.tokens_by_item.insert(item_id.to_owned(), tokens);
        }
    }

    pub(super) fn record_item(&mut self, item: &ThreadItemPayload) {
        if let ThreadItemPayload::AgentMessage { id, text, .. }
        | ThreadItemPayload::Plan { id, text } = item
        {
            let mut previous = '\0';
            self.tokens_by_item
                .insert(id.clone(), estimate_tokens(text, &mut previous));
        }
    }
}

impl<'a> FromIterator<&'a ThreadItemPayload> for TurnOutput {
    fn from_iter<T: IntoIterator<Item = &'a ThreadItemPayload>>(items: T) -> Self {
        let mut output = Self::default();
        for item in items {
            output.record_item(item);
        }
        output
    }
}

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
                    let tokens = output
                        .tokens_by_item
                        .values()
                        .fold(0.0, |sum, tokens| sum + tokens);
                    let rate = tokens / duration.as_secs_f64();
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
