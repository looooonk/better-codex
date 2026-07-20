use super::MAX_SESSION_COMPOSE_LINES;

pub(super) struct CompositionBudget {
    remaining_slots: usize,
}

impl CompositionBudget {
    pub(super) fn new() -> Self {
        Self {
            remaining_slots: MAX_SESSION_COMPOSE_LINES * 2,
        }
    }

    pub(super) fn reserve(&mut self, slots: usize) -> bool {
        let Some(remaining_slots) = self.remaining_slots.checked_sub(slots) else {
            return false;
        };
        self.remaining_slots = remaining_slots;
        true
    }
}
