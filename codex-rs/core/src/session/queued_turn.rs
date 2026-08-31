use super::TurnInput;
use super::session::Session;
use crate::codex_thread::StartQueuedTurnRejectionReason;
use crate::state::ActiveTurn;
use crate::tasks::RegularTask;
use codex_protocol::user_input::UserInput;
use std::sync::Arc;

impl Session {
    pub(super) async fn start_queued_turn(
        self: &Arc<Self>,
        turn_id: String,
        items: Vec<UserInput>,
        client_user_message_id: String,
    ) -> Result<(), StartQueuedTurnRejectionReason> {
        if self.input_queue.has_trigger_turn_mailbox_items().await {
            return Err(StartQueuedTurnRejectionReason::PendingTriggerTurn);
        }

        let turn_state = {
            let mut active_turn = self.active_turn.lock().await;
            if active_turn.is_some() {
                return Err(StartQueuedTurnRejectionReason::Busy);
            }
            let active_turn = active_turn.get_or_insert_with(ActiveTurn::default);
            Arc::clone(&active_turn.turn_state)
        };

        if self.input_queue.has_trigger_turn_mailbox_items().await {
            self.clear_reserved_idle_turn(&turn_state).await;
            self.maybe_start_turn_for_pending_work().await;
            return Err(StartQueuedTurnRejectionReason::PendingTriggerTurn);
        }

        let turn_context = self.new_default_turn_with_sub_id(turn_id).await;
        self.maybe_emit_model_warnings_for_turn(turn_context.as_ref())
            .await;
        let still_reserved = {
            let active_turn = self.active_turn.lock().await;
            active_turn.as_ref().is_some_and(|active_turn| {
                active_turn.task.is_none() && Arc::ptr_eq(&active_turn.turn_state, &turn_state)
            })
        };
        if !still_reserved {
            self.clear_reserved_idle_turn(&turn_state).await;
            return Err(StartQueuedTurnRejectionReason::Busy);
        }

        self.start_task(
            turn_context,
            vec![TurnInput::UserInput {
                content: items,
                client_id: Some(client_user_message_id),
            }],
            RegularTask::new(),
        )
        .await;
        Ok(())
    }
}
