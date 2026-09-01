use super::TurnInput;
use super::session::Session;
use crate::state::ActiveTurn;
use crate::tasks::RegularTask;
use codex_protocol::protocol::QueuedTurnStartRejectionReason;
use codex_protocol::protocol::QueuedTurnStartReply;
use codex_protocol::protocol::QueuedTurnStartSubmission;
use codex_protocol::user_input::UserInput;
use std::sync::Arc;

impl Session {
    pub(super) async fn start_queued_turn(
        self: &Arc<Self>,
        turn_id: String,
        items: Vec<UserInput>,
        client_user_message_id: String,
        reply: QueuedTurnStartReply,
    ) {
        if self.input_queue.has_trigger_turn_mailbox_items().await {
            reply.send(QueuedTurnStartSubmission::NotSubmitted {
                reason: QueuedTurnStartRejectionReason::PendingTriggerTurn,
            });
            return;
        }

        let turn_state = {
            let mut active_turn = self.active_turn.lock().await;
            if active_turn.is_some() {
                reply.send(QueuedTurnStartSubmission::NotSubmitted {
                    reason: QueuedTurnStartRejectionReason::Busy,
                });
                return;
            }
            let active_turn = active_turn.get_or_insert_with(ActiveTurn::default);
            Arc::clone(&active_turn.turn_state)
        };

        if self.input_queue.has_trigger_turn_mailbox_items().await {
            self.clear_reserved_idle_turn(&turn_state).await;
            self.maybe_start_turn_for_pending_work().await;
            reply.send(QueuedTurnStartSubmission::NotSubmitted {
                reason: QueuedTurnStartRejectionReason::PendingTriggerTurn,
            });
            return;
        }

        let turn_context = self.new_default_turn_with_sub_id(turn_id).await;
        self.maybe_emit_model_warnings_for_turn(turn_context.as_ref())
            .await;
        self.refresh_mcp_servers_if_requested(
            &turn_context,
            Some(self.mcp_elicitation_reviewer()),
        )
        .await;
        self.clear_connector_selection().await;
        if self.input_queue.has_trigger_turn_mailbox_items().await {
            self.clear_reserved_idle_turn(&turn_state).await;
            self.maybe_start_turn_for_pending_work().await;
            reply.send(QueuedTurnStartSubmission::NotSubmitted {
                reason: QueuedTurnStartRejectionReason::PendingTriggerTurn,
            });
            return;
        }
        let still_reserved = {
            let active_turn = self.active_turn.lock().await;
            active_turn.as_ref().is_some_and(|active_turn| {
                active_turn.task.is_none() && Arc::ptr_eq(&active_turn.turn_state, &turn_state)
            })
        };
        if !still_reserved {
            self.clear_reserved_idle_turn(&turn_state).await;
            reply.send(QueuedTurnStartSubmission::NotSubmitted {
                reason: QueuedTurnStartRejectionReason::Busy,
            });
            return;
        }

        turn_context.session_telemetry.user_prompt(&items);
        self.start_task(
            turn_context,
            vec![TurnInput::UserInput {
                content: items,
                client_id: Some(client_user_message_id),
            }],
            RegularTask::with_admission_reply(reply),
        )
        .await;
    }
}
