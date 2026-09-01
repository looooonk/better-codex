use std::sync::Arc;

use crate::error_code::internal_error;
use crate::thread_state::ThreadTerminalEvent;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ThreadItem;
use codex_core::CodexThread;
use codex_core::config::Config;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadResumeInput;
use codex_protocol::ThreadId;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_state::BlockedSubmissionRetryPolicy;
use codex_state::QueueTerminalDisposition;
use codex_state::QueuedSubmissionRecord;
use codex_state::QueuedSubmissionState;
use codex_state::QueuedSubmissionTerminalStatus;
use codex_thread_store::ListItemsParams;
use codex_thread_store::ListTurnsParams;
use codex_thread_store::LoadThreadHistoryParams;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::SortDirection;
use codex_thread_store::StoredTurn;
use codex_thread_store::StoredTurnItemsView;

use super::thread_queue_service::ThreadQueueService;
use super::thread_queue_support::QueueRecoveryAction;
use super::thread_queue_support::QueueRecoveryOutcome;
use super::thread_queue_support::missing_turn_recovery_outcome;
use super::thread_queue_support::paginated_queue_recovery_outcome;
use super::thread_queue_support::queue_error;
use super::thread_queue_support::queue_recovery_action;
use super::thread_queue_support::queue_recovery_outcome;

#[derive(Clone, Copy)]
enum TerminalHistoryBarrier {
    FlushLoadedThread,
    ShutdownDurable,
}

impl ThreadQueueService {
    pub(crate) async fn observe_event(
        &self,
        thread: Arc<CodexThread>,
        thread_id: ThreadId,
        event: &EventMsg,
    ) {
        let Some(terminal_event) = ThreadTerminalEvent::from_event(event) else {
            return;
        };
        let turn_id = terminal_event.turn_id().to_string();
        let service = self.clone();
        self.enqueue_background(thread_id, async move {
            match service
                .process_observed_event(thread, thread_id, &turn_id)
                .await
            {
                Ok(Some(QueueTerminalDisposition::Continue)) => {
                    service.schedule_dispatch(thread_id).await;
                }
                Ok(Some(QueueTerminalDisposition::Pause(_))) | Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        %thread_id,
                        message = %error.message,
                        "failed to process queued terminal event"
                    );
                }
            }
        })
        .await;
    }

    async fn process_observed_event(
        &self,
        thread: Arc<CodexThread>,
        thread_id: ThreadId,
        turn_id: &str,
    ) -> Result<Option<QueueTerminalDisposition>, JSONRPCErrorError> {
        let Some(record) = self
            .active_record_for_observed_turn(thread_id, turn_id)
            .await?
        else {
            return Ok(None);
        };
        flush_terminal_rollout(&thread, thread_id).await?;
        let outcome = self
            .durable_recovery_outcome(thread_id, turn_id, &record)
            .await?;
        let outcome = live_terminal_outcome(outcome);
        self.apply_recovery_outcome(thread_id, record, outcome)
            .await
    }

    async fn active_record_for_observed_turn(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
    ) -> Result<Option<QueuedSubmissionRecord>, JSONRPCErrorError> {
        let Some(state_db) = self.state_db.as_ref() else {
            return Ok(None);
        };
        Ok(state_db
            .queued_submission_for_turn(thread_id, turn_id)
            .await
            .map_err(queue_error)?
            .filter(|record| {
                matches!(
                    record.state,
                    QueuedSubmissionState::Starting | QueuedSubmissionState::Inflight
                )
            }))
    }

    pub(super) async fn recover(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<QueueTerminalDisposition>, JSONRPCErrorError> {
        let Some(active) = self
            .state_db()?
            .active_queued_submission(thread_id)
            .await
            .map_err(queue_error)?
        else {
            return Ok(None);
        };
        self.recover_record(thread_id, active, TerminalHistoryBarrier::FlushLoadedThread)
            .await
    }

    pub(super) async fn recover_after_shutdown(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<QueueTerminalDisposition>, JSONRPCErrorError> {
        let Some(active) = self
            .state_db()?
            .active_queued_submission(thread_id)
            .await
            .map_err(queue_error)?
        else {
            return Ok(None);
        };
        self.recover_record(thread_id, active, TerminalHistoryBarrier::ShutdownDurable)
            .await
    }

    #[cfg(test)]
    pub(super) async fn recover_terminal_only(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<QueueTerminalDisposition>, JSONRPCErrorError> {
        let Some(record) = self
            .state_db()?
            .active_queued_submission(thread_id)
            .await
            .map_err(queue_error)?
        else {
            return Ok(None);
        };
        self.recover_terminal_only_record(thread_id, record).await
    }

    pub(super) async fn recover_errored(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<QueueTerminalDisposition>, JSONRPCErrorError> {
        let Some(record) = self
            .state_db()?
            .active_queued_submission(thread_id)
            .await
            .map_err(queue_error)?
        else {
            return Ok(None);
        };
        let turn_id = record
            .turn_id
            .as_deref()
            .ok_or_else(|| internal_error("active queued submission is missing its turn id"))?
            .to_string();
        if self
            .thread_state_manager
            .thread_state(thread_id)
            .await
            .lock()
            .await
            .translated_terminal_event_matches(&turn_id)
        {
            return self
                .recover_record(thread_id, record, TerminalHistoryBarrier::FlushLoadedThread)
                .await;
        }
        self.recover_terminal_only_record(thread_id, record).await
    }

    async fn recover_terminal_only_record(
        &self,
        thread_id: ThreadId,
        record: QueuedSubmissionRecord,
    ) -> Result<Option<QueueTerminalDisposition>, JSONRPCErrorError> {
        let turn_id = record
            .turn_id
            .as_deref()
            .ok_or_else(|| internal_error("active queued submission is missing its turn id"))?
            .to_string();
        let outcome = self
            .durable_recovery_outcome(thread_id, &turn_id, &record)
            .await?;
        match outcome {
            QueueRecoveryOutcome::NotStarted | QueueRecoveryOutcome::Incomplete { .. } => {
                return Ok(None);
            }
            QueueRecoveryOutcome::Completed(_)
            | QueueRecoveryOutcome::Aborted(_)
            | QueueRecoveryOutcome::TerminalWithoutInput => {}
        }
        let disposition = self
            .apply_recovery_outcome(thread_id, record, outcome)
            .await?;
        self.thread_state_manager
            .thread_state(thread_id)
            .await
            .lock()
            .await
            .clear_queued_turn_recovery_markers(&turn_id);
        Ok(disposition)
    }

    async fn recover_record(
        &self,
        thread_id: ThreadId,
        record: QueuedSubmissionRecord,
        terminal_history_barrier: TerminalHistoryBarrier,
    ) -> Result<Option<QueueTerminalDisposition>, JSONRPCErrorError> {
        let turn_id = record
            .turn_id
            .as_deref()
            .ok_or_else(|| internal_error("active queued submission is missing its turn id"))?;
        if self
            .translated_terminal_event(thread_id, turn_id)
            .await
            .is_some()
        {
            let turn_id = turn_id.to_string();
            match terminal_history_barrier {
                TerminalHistoryBarrier::FlushLoadedThread => {
                    let thread_manager = self.thread_manager.upgrade().ok_or_else(|| {
                        internal_error(
                            "thread manager closed before queued terminal history was flushed",
                        )
                    })?;
                    let thread = thread_manager.get_thread(thread_id).await.map_err(|error| {
                        internal_error(format!(
                            "failed to load queued thread before terminal history flush: {error}"
                        ))
                    })?;
                    thread.flush_rollout().await.map_err(|error| {
                        internal_error(format!(
                            "failed to flush queued terminal history before recovery: {error}"
                        ))
                    })?;
                }
                TerminalHistoryBarrier::ShutdownDurable => {}
            }
            let outcome = self
                .durable_recovery_outcome(thread_id, &turn_id, &record)
                .await?;
            let outcome = live_terminal_outcome(outcome);
            let result = self
                .apply_recovery_action(
                    thread_id,
                    record,
                    recovery_action_with_barrier(outcome, terminal_history_barrier),
                )
                .await;
            if result.is_ok() {
                self.thread_state_manager
                    .thread_state(thread_id)
                    .await
                    .lock()
                    .await
                    .clear_queued_turn_recovery_markers(&turn_id);
            }
            return result;
        }
        if self
            .thread_state_manager
            .thread_state(thread_id)
            .await
            .lock()
            .await
            .queued_turn_ambiguous_recovery_failed(turn_id)
        {
            let turn_id = turn_id.to_string();
            let result = self
                .apply_recovery_outcome(
                    thread_id,
                    record,
                    QueueRecoveryOutcome::Incomplete {
                        input_persisted: true,
                    },
                )
                .await;
            if result.is_ok() {
                self.thread_state_manager
                    .thread_state(thread_id)
                    .await
                    .lock()
                    .await
                    .clear_queued_turn_recovery_markers(&turn_id);
            }
            return result;
        }
        let outcome = self
            .durable_recovery_outcome(thread_id, turn_id, &record)
            .await?;
        self.apply_recovery_action(
            thread_id,
            record,
            recovery_action_with_barrier(outcome, terminal_history_barrier),
        )
        .await
    }

    pub(super) async fn durable_recovery_outcome(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        record: &QueuedSubmissionRecord,
    ) -> Result<QueueRecoveryOutcome, JSONRPCErrorError> {
        let stored_thread = self
            .thread_store
            .read_thread(ReadThreadParams {
                thread_id,
                include_archived: false,
                include_history: false,
            })
            .await
            .map_err(|error| internal_error(format!("failed to read queued thread: {error}")))?;
        let outcome = match stored_thread.history_mode {
            ThreadHistoryMode::Legacy => {
                let history = self
                    .thread_store
                    .load_history(LoadThreadHistoryParams {
                        thread_id,
                        include_archived: false,
                    })
                    .await
                    .map_err(|error| {
                        internal_error(format!("failed to read queue recovery history: {error}"))
                    })?;
                queue_recovery_outcome(
                    &history.items,
                    turn_id,
                    &record.client_user_message_id,
                    record.admission_rejection,
                )
            }
            ThreadHistoryMode::Paginated => {
                self.paginated_recovery_outcome(thread_id, turn_id, &record)
                    .await?
            }
        };
        Ok(outcome)
    }

    pub(super) async fn recover_ambiguous_start(
        &self,
        thread_id: ThreadId,
        thread: &CodexThread,
        record: QueuedSubmissionRecord,
    ) -> Result<(), JSONRPCErrorError> {
        let turn_id = record
            .turn_id
            .as_deref()
            .ok_or_else(|| internal_error("active queued submission is missing its turn id"))?
            .to_string();
        let reconciliation = async {
            thread.flush_rollout().await.map_err(|error| {
                internal_error(format!(
                    "failed to flush queue history after ambiguous admission: {error}"
                ))
            })?;
            let outcome = self
                .durable_recovery_outcome(thread_id, &turn_id, &record)
                .await?;
            self.apply_recovery_outcome(thread_id, record.clone(), outcome)
                .await
        }
        .await;
        self.finalize_ambiguous_start_recovery(thread_id, record, turn_id, reconciliation)
            .await
    }

    async fn finalize_ambiguous_start_recovery(
        &self,
        thread_id: ThreadId,
        record: QueuedSubmissionRecord,
        turn_id: String,
        reconciliation: Result<Option<QueueTerminalDisposition>, JSONRPCErrorError>,
    ) -> Result<(), JSONRPCErrorError> {
        match reconciliation {
            Ok(disposition) => {
                self.thread_state_manager
                    .thread_state(thread_id)
                    .await
                    .lock()
                    .await
                    .clear_queued_turn_recovery_markers(&turn_id);
                if matches!(disposition, Some(QueueTerminalDisposition::Continue)) {
                    self.schedule_dispatch(thread_id).await;
                }
            }
            Err(recovery_error) => {
                let blocked = match self.state_db() {
                    Ok(state_db) => state_db
                        .block_indeterminate_queued_submission(
                            thread_id,
                            &record.id,
                            &turn_id,
                            BlockedSubmissionRetryPolicy::Forbidden,
                        )
                        .await
                        .map_err(queue_error),
                    Err(error) => Err(error),
                };
                match blocked {
                    Ok(true) => {
                        self.thread_state_manager
                            .thread_state(thread_id)
                            .await
                            .lock()
                            .await
                            .clear_queued_turn_recovery_markers(&turn_id);
                        tracing::error!(
                            %thread_id,
                            queued_submission_id = %record.id,
                            %turn_id,
                            recovery_message = %recovery_error.message,
                            "ambiguous queued turn recovery failed; blocked the item from retry"
                        );
                        self.notify_changed(thread_id).await;
                    }
                    Ok(false) => {
                        self.thread_state_manager
                            .thread_state(thread_id)
                            .await
                            .lock()
                            .await
                            .mark_queued_turn_ambiguous_recovery_failed(turn_id.clone());
                        return Err(internal_error(format!(
                            "ambiguous queued turn recovery failed ({}) and submission {} could not be blocked",
                            recovery_error.message, record.id
                        )));
                    }
                    Err(block_error) => {
                        self.thread_state_manager
                            .thread_state(thread_id)
                            .await
                            .lock()
                            .await
                            .mark_queued_turn_ambiguous_recovery_failed(turn_id.clone());
                        return Err(internal_error(format!(
                            "ambiguous queued turn recovery failed ({}) and submission {} could not be blocked: {}",
                            recovery_error.message, record.id, block_error.message
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    async fn translated_terminal_event(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
    ) -> Option<ThreadTerminalEvent> {
        let thread_state = self.thread_state_manager.thread_state(thread_id).await;
        let (listener_generation, mut terminal_event_rx, terminal_event_pending) = {
            let state = thread_state.lock().await;
            (
                state.listener_generation,
                state.translated_terminal_event_receiver(),
                state.terminal_event_pending(turn_id),
            )
        };
        let terminal_event = terminal_event_rx.borrow_and_update().clone();
        if terminal_event
            .as_ref()
            .is_some_and(|event| event.turn_id() == turn_id)
        {
            return terminal_event;
        }
        if !terminal_event_pending
            || self
                .thread_state_manager
                .current_listener_command_tx(thread_id)
                .is_none()
        {
            return None;
        }
        loop {
            let terminal_event = terminal_event_rx.borrow_and_update().clone();
            if terminal_event
                .as_ref()
                .is_some_and(|event| event.turn_id() == turn_id)
            {
                return terminal_event;
            }
            let listener_is_current = {
                let state = thread_state.lock().await;
                state.listener_generation == listener_generation
                    && state.listener_command_tx().is_some()
                    && state.terminal_event_pending(turn_id)
            };
            if !listener_is_current || terminal_event_rx.changed().await.is_err() {
                return None;
            }
        }
    }

    pub(super) async fn apply_recovery_outcome(
        &self,
        thread_id: ThreadId,
        record: QueuedSubmissionRecord,
        outcome: QueueRecoveryOutcome,
    ) -> Result<Option<QueueTerminalDisposition>, JSONRPCErrorError> {
        self.apply_recovery_action(thread_id, record, queue_recovery_action(outcome))
            .await
    }

    async fn apply_recovery_action(
        &self,
        thread_id: ThreadId,
        record: QueuedSubmissionRecord,
        action: QueueRecoveryAction,
    ) -> Result<Option<QueueTerminalDisposition>, JSONRPCErrorError> {
        let turn_id = record
            .turn_id
            .as_deref()
            .ok_or_else(|| internal_error("active queued submission is missing its turn id"))?;
        match action {
            QueueRecoveryAction::Indeterminate { input_persisted } => {
                let retry_policy = if input_persisted {
                    BlockedSubmissionRetryPolicy::Forbidden
                } else {
                    BlockedSubmissionRetryPolicy::Allowed
                };
                let blocked = self
                    .state_db()?
                    .block_indeterminate_queued_submission(
                        thread_id,
                        &record.id,
                        turn_id,
                        retry_policy,
                    )
                    .await
                    .map_err(queue_error)?;
                if !blocked {
                    return Err(internal_error(format!(
                        "queued submission {} for indeterminate turn {turn_id} could not be blocked",
                        record.id
                    )));
                }
                self.notify_changed(thread_id).await;
                Ok(Some(QueueTerminalDisposition::Pause(
                    codex_state::ThreadQueuePauseReason::Interrupted,
                )))
            }
            QueueRecoveryAction::Finish {
                status,
                disposition,
            } => {
                let changed = self
                    .state_db()?
                    .finish_queued_submission(thread_id, turn_id, status, disposition)
                    .await
                    .map_err(queue_error)?;
                if !changed {
                    return Err(internal_error(format!(
                        "queued submission {} for terminal turn {turn_id} could not be finalized",
                        record.id
                    )));
                }
                self.notify_changed(thread_id).await;
                Ok(Some(disposition))
            }
        }
    }

    async fn paginated_recovery_outcome(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        record: &QueuedSubmissionRecord,
    ) -> Result<QueueRecoveryOutcome, JSONRPCErrorError> {
        let Some(turn) = self.paginated_turn(thread_id, turn_id).await? else {
            return Ok(missing_turn_recovery_outcome(record.admission_rejection));
        };
        let input_persisted = self
            .paginated_input_persisted(thread_id, turn_id, &record.client_user_message_id)
            .await?;
        Ok(paginated_queue_recovery_outcome(
            turn.status,
            turn.abort_reason,
            input_persisted,
            record.admission_rejection,
        ))
    }

    async fn paginated_turn(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
    ) -> Result<Option<StoredTurn>, JSONRPCErrorError> {
        let page = self
            .thread_store
            .list_turns(ListTurnsParams {
                thread_id,
                turn_id: Some(turn_id.to_string()),
                include_archived: false,
                cursor: None,
                page_size: 1,
                sort_direction: SortDirection::Desc,
                items_view: StoredTurnItemsView::NotLoaded,
            })
            .await
            .map_err(|error| {
                internal_error(format!("failed to list queued turn history: {error}"))
            })?;
        Ok(page.turns.into_iter().next())
    }

    async fn paginated_input_persisted(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        client_user_message_id: &str,
    ) -> Result<bool, JSONRPCErrorError> {
        // Queued user input precedes model and tool items in its reserved turn.
        let page = self
            .thread_store
            .list_items(ListItemsParams {
                thread_id,
                turn_id: Some(turn_id.to_string()),
                include_archived: false,
                cursor: None,
                page_size: 100,
                sort_direction: SortDirection::Asc,
            })
            .await
            .map_err(|error| {
                internal_error(format!("failed to list queued turn items: {error}"))
            })?;
        for item in page.items {
            let item = serde_json::from_slice::<ThreadItem>(&item.item_json).map_err(|error| {
                internal_error(format!(
                    "failed to deserialize queued turn item {}: {error}",
                    item.item_id
                ))
            })?;
            if matches!(
                item,
                ThreadItem::UserMessage { client_id: Some(client_id), .. }
                    if client_id == client_user_message_id
            ) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn recovery_action_with_barrier(
    outcome: QueueRecoveryOutcome,
    terminal_history_barrier: TerminalHistoryBarrier,
) -> QueueRecoveryAction {
    match (terminal_history_barrier, outcome) {
        (TerminalHistoryBarrier::ShutdownDurable, QueueRecoveryOutcome::Aborted(_)) => {
            QueueRecoveryAction::Finish {
                status: QueuedSubmissionTerminalStatus::Interrupted,
                disposition: QueueTerminalDisposition::Continue,
            }
        }
        (_, outcome) => queue_recovery_action(outcome),
    }
}

fn live_terminal_outcome(outcome: QueueRecoveryOutcome) -> QueueRecoveryOutcome {
    if outcome == QueueRecoveryOutcome::NotStarted {
        QueueRecoveryOutcome::TerminalWithoutInput
    } else {
        outcome
    }
}

impl ThreadLifecycleContributor<Config> for ThreadQueueService {
    fn on_thread_resume<'a>(&'a self, input: ThreadResumeInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Ok(thread_id) = ThreadId::from_string(input.thread_store.level_id()) else {
                return;
            };
            if let Err(error) = self.recover_serialized(thread_id).await {
                tracing::warn!(
                    %thread_id,
                    message = %error.message,
                    "failed to recover thread queue while resuming"
                );
            }
        })
    }

    fn on_thread_idle<'a>(&'a self, input: ThreadIdleInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Ok(thread_id) = ThreadId::from_string(input.thread_store.level_id()) else {
                return;
            };
            if let Err(error) = self.recover_and_dispatch_serialized(thread_id).await {
                tracing::warn!(
                    %thread_id,
                    message = %error.message,
                    "failed to dispatch thread queue while idle"
                );
            }
        })
    }
}

#[cfg(test)]
#[path = "thread_queue_recovery_tests.rs"]
mod tests;

async fn flush_terminal_rollout(
    thread: &CodexThread,
    thread_id: ThreadId,
) -> Result<(), JSONRPCErrorError> {
    thread.flush_rollout().await.map_err(|error| {
        internal_error(format!(
            "failed to flush queued turn terminal event for {thread_id}: {error}"
        ))
    })
}
