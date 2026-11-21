use super::ShellState;
use std::time::Instant;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TurnMetrics {
    pub(super) elapsed_seconds: u64,
    pub(super) tokens_per_second: u64,
}

impl ShellState {
    pub(super) fn record_active_turn_started(&mut self, turn_id: String) {
        if self.active_turn_id.as_deref() != Some(turn_id.as_str())
            || self.turn_started_at.is_none()
        {
            if let Some(previous) = self.active_turn_id.take() {
                self.turn_outputs.remove(&previous);
            }
            self.turn_started_at = Some(Instant::now());
            self.output_rate = Default::default();
        }
        self.active_turn_id = Some(turn_id);
    }

    pub(super) fn clear_active_turn(&mut self) {
        self.turn_outputs.clear();
        self.active_turn_id = None;
        self.turn_started_at = None;
        self.output_rate = Default::default();
    }

    pub(super) fn record_streamed_output(&mut self, turn_id: &str, delta: &str) {
        if self.active_turn_id.as_deref() == Some(turn_id) {
            self.output_rate.record(Instant::now(), delta);
        }
    }

    pub(super) fn active_turn_metrics(&self) -> Option<TurnMetrics> {
        let started = self.turn_started_at?;
        let now = Instant::now();
        Some(TurnMetrics {
            elapsed_seconds: now.duration_since(started).as_secs(),
            tokens_per_second: self.output_rate.tokens_per_second(now),
        })
    }
}

#[cfg(test)]
#[path = "turn_timer_tests.rs"]
mod tests;
