use super::ShellState;
use std::time::Instant;

impl ShellState {
    pub(super) fn record_active_turn_started(&mut self, turn_id: String) {
        if self.active_turn_id.as_deref() != Some(turn_id.as_str())
            || self.turn_started_at.is_none()
        {
            self.turn_started_at = Some(Instant::now());
        }
        self.active_turn_id = Some(turn_id);
    }

    pub(super) fn clear_active_turn(&mut self) {
        self.active_turn_id = None;
        self.turn_started_at = None;
    }

    pub(super) fn active_turn_elapsed_seconds(&self) -> Option<u64> {
        self.turn_started_at
            .map(|started_at| started_at.elapsed().as_secs())
    }
}
