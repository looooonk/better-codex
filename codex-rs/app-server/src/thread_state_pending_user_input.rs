use std::collections::HashSet;

use codex_protocol::protocol::EventMsg;

#[derive(Default)]
pub(crate) struct PendingUserInputSubmissions {
    turn_ids: HashSet<String>,
}

impl PendingUserInputSubmissions {
    pub(crate) fn mark(&mut self, turn_id: String) {
        self.turn_ids.insert(turn_id);
    }

    pub(crate) fn is_pending(&self) -> bool {
        !self.turn_ids.is_empty()
    }

    pub(crate) fn observe(&mut self, event_turn_id: &str, event: &EventMsg) {
        if matches!(
            event,
            EventMsg::TurnStarted(_) | EventMsg::UserMessage(_) | EventMsg::Error(_)
        ) {
            self.turn_ids.remove(event_turn_id);
        }
        if matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_)) {
            self.clear();
        }
    }

    pub(crate) fn clear(&mut self) {
        self.turn_ids.clear();
    }
}

#[cfg(test)]
#[path = "thread_state_pending_user_input_tests.rs"]
mod tests;
