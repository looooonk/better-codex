use super::*;

impl ThreadHistoryBuilder {
    pub(super) fn handle_context_compacted(&mut self, _payload: &ContextCompactedEvent) {
        let id = self.next_item_id();
        self.push_item_in_current_turn(ThreadItem::ContextCompaction { id });
    }

    pub(super) fn handle_entered_review_mode(
        &mut self,
        payload: &codex_protocol::protocol::EnteredReviewModeEvent,
    ) {
        let review = payload
            .user_facing_hint
            .clone()
            .unwrap_or_else(|| "Review requested.".to_string());
        let id = payload
            .item_id
            .clone()
            .unwrap_or_else(|| self.next_item_id());
        self.upsert_review_mode_item(
            payload.turn_id.as_deref(),
            ThreadItem::EnteredReviewMode { id, review },
        );
    }

    pub(super) fn handle_exited_review_mode(
        &mut self,
        payload: &codex_protocol::protocol::ExitedReviewModeEvent,
    ) {
        let review = review_output_text(payload.review_output.as_ref());
        let id = payload
            .item_id
            .clone()
            .unwrap_or_else(|| self.next_item_id());
        self.upsert_review_mode_item(
            payload.turn_id.as_deref(),
            ThreadItem::ExitedReviewMode { id, review },
        );
    }

    pub(super) fn upsert_review_mode_item(&mut self, turn_id: Option<&str>, item: ThreadItem) {
        let Some(turn_id) = turn_id else {
            self.upsert_item_in_current_turn(item);
            return;
        };
        let current_turn_matches = self
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.id == turn_id);
        if !current_turn_matches && !self.turns.iter().any(|turn| turn.id == turn_id) {
            self.finish_current_turn();
            let turn = self.new_turn(Some(turn_id.to_string()));
            self.record_changed_pending_turn(&turn);
            self.current_turn = Some(turn);
        }
        self.upsert_item_in_turn_id(turn_id, item);
    }

    pub(super) fn handle_error(&mut self, payload: &ErrorEvent) {
        if !payload.affects_turn_status() {
            return;
        }
        let tracking_changes = self.is_tracking_changes();
        let changed_turn = if let Some(turn) = self.current_turn.as_mut() {
            turn.status = TurnStatus::Failed;
            turn.error = Some(V2TurnError {
                message: payload.message.clone(),
                codex_error_info: payload.codex_error_info.clone().map(Into::into),
                additional_details: None,
            });
            tracking_changes.then(|| ThreadHistoryTurnChange::from_pending_turn(turn))
        } else {
            None
        };
        if let Some(changed_turn) = changed_turn {
            self.record_changed_turn(changed_turn);
        }
    }

    pub(super) fn handle_turn_aborted(&mut self, payload: &TurnAbortedEvent) {
        let apply_abort = |turn: &mut PendingTurn| {
            turn.status = TurnStatus::Interrupted;
            turn.completed_at = payload.completed_at;
            turn.duration_ms = payload.duration_ms;
            ThreadHistoryTurnChange {
                abort_reason: Some(payload.reason.clone()),
                ..ThreadHistoryTurnChange::from_pending_turn(turn)
            }
        };
        if let Some(turn_id) = payload.turn_id.as_deref() {
            // Prefer an exact ID match so we interrupt the turn explicitly targeted by the event.
            if let Some(turn) = self.current_turn.as_mut().filter(|turn| turn.id == turn_id) {
                let changed_turn = apply_abort(turn);
                self.record_changed_turn(changed_turn);
                return;
            }

            if let Some(turn) = self.turns.iter_mut().find(|turn| turn.id == turn_id) {
                turn.status = TurnStatus::Interrupted;
                turn.completed_at = payload.completed_at;
                turn.duration_ms = payload.duration_ms;
                let changed_turn = ThreadHistoryTurnChange {
                    abort_reason: Some(payload.reason.clone()),
                    ..ThreadHistoryTurnChange::from_turn(turn)
                };
                self.record_changed_turn(changed_turn);
                return;
            }
        }

        // If the event has no ID (or refers to an unknown turn), fall back to the active turn.
        if let Some(turn) = self.current_turn.as_mut() {
            let changed_turn = apply_abort(turn);
            self.record_changed_turn(changed_turn);
        }
    }

    pub(super) fn handle_turn_started(&mut self, payload: &TurnStartedEvent) {
        self.finish_current_turn();
        let turn = self
            .new_turn(Some(payload.turn_id.clone()))
            .with_status(TurnStatus::InProgress)
            .with_started_at(payload.started_at)
            .opened_explicitly();
        self.record_changed_pending_turn(&turn);
        self.current_turn = Some(turn);
    }

    pub(super) fn handle_turn_complete(&mut self, payload: &TurnCompleteEvent) {
        let mark_completed = |turn: &mut PendingTurn| {
            if matches!(turn.status, TurnStatus::Completed | TurnStatus::InProgress) {
                turn.status = TurnStatus::Completed;
            }
            turn.completed_at = payload.completed_at;
            turn.duration_ms = payload.duration_ms;
            ThreadHistoryTurnChange::from_pending_turn(turn)
        };

        // Prefer an exact ID match from the active turn and then close it.
        if let Some(current_turn) = self
            .current_turn
            .as_mut()
            .filter(|turn| turn.id == payload.turn_id)
        {
            let changed_turn = mark_completed(current_turn);
            self.record_changed_turn(changed_turn);
            self.finish_current_turn();
            return;
        }

        if let Some(turn) = self
            .turns
            .iter_mut()
            .find(|turn| turn.id == payload.turn_id)
        {
            if matches!(turn.status, TurnStatus::Completed | TurnStatus::InProgress) {
                turn.status = TurnStatus::Completed;
            }
            turn.completed_at = payload.completed_at;
            turn.duration_ms = payload.duration_ms;
            let changed_turn = ThreadHistoryTurnChange::from_turn(turn);
            self.record_changed_turn(changed_turn);
            return;
        }

        // If the completion event cannot be matched, apply it to the active turn.
        if let Some(current_turn) = self.current_turn.as_mut() {
            let changed_turn = mark_completed(current_turn);
            self.record_changed_turn(changed_turn);
            self.finish_current_turn();
        }
    }

    /// Marks the current turn as containing a persisted compaction marker.
    ///
    /// This keeps compaction-only legacy turns from being dropped by
    /// `finish_current_turn` when they have no renderable items and were not
    /// explicitly opened.
    pub(super) fn handle_compacted(&mut self, _payload: &CompactedItem) {
        self.ensure_turn().saw_compaction = true;
    }

    pub(super) fn handle_thread_rollback(&mut self, payload: &ThreadRolledBackEvent) {
        self.finish_current_turn();

        let n = usize::try_from(payload.num_turns).unwrap_or(usize::MAX);
        let removed_turn_ids = if n >= self.turns.len() {
            self.turns.iter().map(|turn| turn.id.clone()).collect()
        } else if n == 0 {
            Vec::new()
        } else {
            self.turns[self.turns.len() - n..]
                .iter()
                .map(|turn| turn.id.clone())
                .collect()
        };
        self.record_removed_turn_ids(removed_turn_ids);

        if n >= self.turns.len() {
            self.turns.clear();
        } else {
            self.turns.truncate(self.turns.len().saturating_sub(n));
        }

        let item_count: usize = self.turns.iter().map(|t| t.items.len()).sum();
        self.next_item_index = i64::try_from(item_count.saturating_add(1)).unwrap_or(i64::MAX);
    }
}
