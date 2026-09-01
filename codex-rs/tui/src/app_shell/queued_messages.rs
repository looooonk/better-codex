use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use super::composer::QueueEdit;
use codex_app_server_protocol::QueuedSubmission;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::UserInput;
use codex_protocol::ThreadId;
use color_eyre::Result;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::time::Duration;

const MAX_ADD_ATTEMPTS: u8 = 2;
const MAX_HYDRATION_FAILURES: u8 = 2;

struct AddRecovery {
    client_user_message_id: String,
    error: String,
}

#[derive(Debug, Clone)]
pub(super) enum QueueTarget {
    Id(String),
    Client(String),
}

#[derive(Debug, Clone)]
pub(super) enum QueueMutation {
    Add {
        input: Vec<UserInput>,
        client_user_message_id: String,
        attempts: u8,
    },
    Update {
        target: QueueTarget,
        input: Vec<UserInput>,
    },
    Delete {
        target: QueueTarget,
    },
    Reorder {
        targets: Vec<QueueTarget>,
    },
    Start,
}

#[derive(Debug, Clone)]
pub(super) enum QueueRpc {
    Add {
        input: Vec<UserInput>,
        client_user_message_id: String,
    },
    Update {
        queued_submission_id: String,
        input: Vec<UserInput>,
    },
    Delete {
        queued_submission_id: String,
    },
    Reorder {
        queued_submission_ids: Vec<String>,
    },
    Start,
}

#[derive(Debug)]
pub(super) enum QueueRpcResponse {
    Added(QueuedSubmission),
    Updated(QueuedSubmission),
    Deleted(bool),
    Reordered,
    Started(Turn),
}

#[derive(Default)]
pub(super) struct DurableQueueState {
    pending: VecDeque<QueueMutation>,
    resolved_ids: HashMap<String, String>,
    hydration_due: bool,
    hydration_failures: u8,
    hydration_retry_delayed: bool,
    start_pending: bool,
    add_recovery: Option<AddRecovery>,
    reconciliation_required: bool,
}

impl DurableQueueState {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    fn resolve(&self, target: &QueueTarget) -> Option<String> {
        match target {
            QueueTarget::Id(id) => Some(id.clone()),
            QueueTarget::Client(client_id) => self.resolved_ids.get(client_id).cloned(),
        }
    }

    fn record(&mut self, submission: &QueuedSubmission) {
        self.resolved_ids.insert(
            submission.client_user_message_id.clone(),
            submission.id.clone(),
        );
    }

    fn replace_records(&mut self, submissions: &[QueuedSubmission]) {
        self.resolved_ids.clear();
        for submission in submissions {
            self.record(submission);
        }
    }

    fn recovery_required(&self) -> bool {
        self.add_recovery.is_some() || self.reconciliation_required
    }
}

impl ShellState {
    pub(super) fn has_pending_queue_mutation(&self) -> bool {
        !self.queue_state.pending.is_empty()
            || self.has_pending_backend_action(ActionGroup::QueueMutation)
            || self.queue_state.recovery_required()
    }

    pub(super) fn queue_current_message<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        if self.composer.queued_edit_position().is_some() {
            self.composer.finish_queued_message_edit();
            self.sync_composer_queue_edits(app_server);
            return;
        }

        let prompt = self.composer.submission_text();
        if prompt.trim().is_empty()
            || self.reject_oversized_input(prompt.len())
            || self.reject_unavailable_session_action()
        {
            return;
        }
        let client_user_message_id = format!("better-codex-queue-{}", uuid::Uuid::new_v4());
        if !self
            .composer
            .queue_current_message_with_client_id(client_user_message_id.clone())
        {
            return;
        }
        if self.has_pending_backend_action(ActionGroup::QueueHydration) {
            self.queue_state.hydration_due = true;
        }
        self.composer.remember_submission(&prompt);
        self.queue_state.pending.push_back(QueueMutation::Add {
            input: text_input(prompt),
            client_user_message_id,
            attempts: 0,
        });
        self.status = "queueing message".to_string();
        self.start_next_queue_mutation(app_server);
    }

    pub(super) fn sync_composer_queue_edits<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        let edits = self.composer.drain_queue_edits().collect::<Vec<_>>();
        if !edits.is_empty() && self.has_pending_backend_action(ActionGroup::QueueHydration) {
            self.queue_state.hydration_due = true;
        }
        for edit in edits {
            match edit {
                QueueEdit::Update {
                    id,
                    client_user_message_id,
                    text,
                } => self.queue_state.pending.push_back(QueueMutation::Update {
                    target: queue_target(id, client_user_message_id),
                    input: text_input(text),
                }),
                QueueEdit::Delete {
                    id,
                    client_user_message_id,
                } => self.queue_state.pending.push_back(QueueMutation::Delete {
                    target: queue_target(id, client_user_message_id),
                }),
                QueueEdit::Reorder { order } => {
                    self.queue_state.pending.push_back(QueueMutation::Reorder {
                        targets: order
                            .into_iter()
                            .map(|identity| {
                                queue_target(identity.id, identity.client_user_message_id)
                            })
                            .collect(),
                    });
                }
            }
        }
        self.start_next_queue_mutation(app_server);
        self.maybe_start_queue_hydration(app_server);
    }

    pub(super) fn start_queued_message<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        let hydration_pending = self.has_pending_backend_action(ActionGroup::QueueHydration);
        if self.queue_state.recovery_required()
            || self.queue_state.hydration_due
            || hydration_pending
        {
            self.status = "refreshing queued messages".to_string();
            if !hydration_pending {
                self.queue_state.hydration_due = true;
                self.maybe_start_queue_hydration(app_server);
            }
            return;
        }
        if self.active_turn_id.is_some()
            || !self.composer.has_queued_messages()
            || self.reject_unavailable_session_action()
        {
            return;
        }
        if !self.queue_state.start_pending {
            if self.has_pending_backend_action(ActionGroup::QueueHydration) {
                self.queue_state.hydration_due = true;
            }
            self.queue_state.pending.push_back(QueueMutation::Start);
            self.queue_state.start_pending = true;
        }
        self.status = "resuming queued messages".to_string();
        self.start_next_queue_mutation(app_server);
    }

    pub(super) fn request_queue_hydration<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        self.queue_state.hydration_due = true;
        self.maybe_start_queue_hydration(app_server);
    }

    pub(super) fn maybe_start_queue_hydration<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        let recovery_required = self.queue_state.recovery_required();
        if !self.queue_state.hydration_due
            || (!recovery_required && !self.queue_state.pending.is_empty())
            || self.has_pending_backend_action(ActionGroup::QueueMutation)
            || self.has_pending_backend_action(ActionGroup::QueueHydration)
            || self.composer.queued_edit_position().is_some()
        {
            return;
        }
        self.queue_state.hydration_due = false;
        let retry_delayed = std::mem::take(&mut self.queue_state.hydration_retry_delayed);
        let thread_id = self.thread_id;
        let request = app_server.thread_queue_list_in_background(thread_id);
        self.backend_actions
            .start(Some(ActionGroup::QueueHydration), async move {
                if retry_delayed {
                    tokio::time::sleep(Duration::from_secs(/*secs*/ 1)).await;
                }
                BackendActionResult::QueueHydration {
                    thread_id,
                    result: request.await,
                }
            });
    }

    pub(super) fn complete_queue_hydration<S>(
        &mut self,
        app_server: &S,
        thread_id: ThreadId,
        result: Result<Vec<QueuedSubmission>>,
    ) where
        S: AppShellBackend,
    {
        if thread_id != self.thread_id {
            self.queue_state.hydration_due = true;
            self.maybe_start_queue_hydration(app_server);
            return;
        }
        let recovery_required = self.queue_state.recovery_required();
        if self.queue_state.hydration_due
            || (!recovery_required && !self.queue_state.pending.is_empty())
            || self.has_pending_backend_action(ActionGroup::QueueMutation)
            || self.composer.queued_edit_position().is_some()
        {
            self.queue_state.hydration_due = true;
            self.maybe_start_queue_hydration(app_server);
            return;
        }
        match result {
            Ok(submissions) => {
                self.queue_state.hydration_failures = 0;
                self.queue_state.replace_records(&submissions);
                if let Some(recovery) = self.queue_state.add_recovery.take() {
                    if let Some(submission) = submissions.iter().find(|submission| {
                        submission.client_user_message_id == recovery.client_user_message_id
                    }) {
                        self.composer.confirm_queued_submission(submission.clone());
                        self.status = "message queued".to_string();
                    } else {
                        self.cancel_pending_queue_mutations();
                        if let Some(submission) = self
                            .composer
                            .remove_queued_submission_for_client(
                                &recovery.client_user_message_id,
                            )
                        {
                            self.composer.restore_failed_queued_submission(&submission);
                        }
                        self.status = "action failed".to_string();
                        self.push_error(format!(
                            "failed to update queued messages: {}",
                            recovery.error
                        ));
                    }
                }
                self.queue_state.reconciliation_required = false;
                if self.queue_state.pending.is_empty() {
                    self.composer.replace_queued_submissions(submissions);
                } else {
                    self.queue_state.hydration_due = true;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "failed to hydrate queued messages");
                self.queue_state.hydration_failures =
                    self.queue_state.hydration_failures.saturating_add(1);
                if self.queue_state.hydration_failures < MAX_HYDRATION_FAILURES {
                    self.queue_state.hydration_due = true;
                    self.push_status("retrying queued message refresh");
                } else if self.queue_state.recovery_required() {
                    self.queue_state.hydration_failures = 0;
                    self.queue_state.hydration_due = true;
                    self.queue_state.hydration_retry_delayed = true;
                    self.push_status("waiting to retry queued message refresh");
                } else {
                    self.queue_state.hydration_failures = 0;
                    self.push_status("failed to refresh queued messages");
                }
            }
        }
        if !self.queue_state.recovery_required() {
            self.start_next_queue_mutation(app_server);
        }
        self.maybe_start_queue_hydration(app_server);
    }

    pub(super) fn complete_queue_mutation<S>(
        &mut self,
        app_server: &S,
        thread_id: ThreadId,
        mutation: QueueMutation,
        result: Result<QueueRpcResponse>,
    ) where
        S: AppShellBackend,
    {
        if thread_id != self.thread_id {
            self.queue_state.hydration_due = true;
            self.maybe_start_queue_hydration(app_server);
            return;
        }
        if matches!(&mutation, QueueMutation::Start) {
            self.queue_state.start_pending = false;
        }
        match result {
            Ok(response) => {
                match response {
                    QueueRpcResponse::Added(submission)
                    | QueueRpcResponse::Updated(submission) => {
                        self.queue_state.record(&submission);
                        self.composer.confirm_queued_submission(submission);
                    }
                    QueueRpcResponse::Deleted(true) => {}
                    QueueRpcResponse::Deleted(false) => {
                        self.cancel_pending_queue_mutations();
                        self.queue_state.reconciliation_required = true;
                        self.queue_state.hydration_due = true;
                        self.push_status("queued message changed; refreshing");
                    }
                    QueueRpcResponse::Reordered => {}
                    QueueRpcResponse::Started(turn) => {
                        self.record_active_turn_started(turn.id);
                        self.status = "thinking".to_string();
                        self.queue_state.hydration_due = true;
                    }
                }
                if matches!(mutation, QueueMutation::Add { .. }) {
                    self.status = "message queued".to_string();
                }
            }
            Err(error) => match mutation {
                QueueMutation::Add {
                    input,
                    client_user_message_id,
                    attempts,
                } if attempts.saturating_add(1) < MAX_ADD_ATTEMPTS => {
                    self.queue_state.pending.push_front(QueueMutation::Add {
                        input,
                        client_user_message_id,
                        attempts: attempts.saturating_add(1),
                    });
                    self.status = "retrying queued message".to_string();
                }
                QueueMutation::Add {
                    input: _,
                    client_user_message_id,
                    ..
                } => {
                    self.queue_state.add_recovery = Some(AddRecovery {
                        client_user_message_id,
                        error: format!("{error:#}"),
                    });
                    self.status = "confirming queued message".to_string();
                    self.queue_state.hydration_due = true;
                }
                QueueMutation::Update { .. }
                | QueueMutation::Delete { .. }
                | QueueMutation::Reorder { .. }
                | QueueMutation::Start => {
                    self.cancel_pending_queue_mutations();
                    self.queue_state.reconciliation_required = true;
                    self.report_action_error("failed to update queued messages", error);
                    self.queue_state.hydration_due = true;
                }
            },
        }
        if !self.queue_state.recovery_required() {
            self.start_next_queue_mutation(app_server);
        }
        self.maybe_start_queue_hydration(app_server);
    }

    fn start_next_queue_mutation<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        if self.queue_state.recovery_required() {
            self.queue_state.hydration_due = true;
            self.maybe_start_queue_hydration(app_server);
            return;
        }
        if self.has_pending_backend_action(ActionGroup::QueueMutation) {
            return;
        }
        while let Some(mutation) = self.queue_state.pending.pop_front() {
            if matches!(mutation, QueueMutation::Start) && !self.composer.has_queued_messages() {
                self.queue_state.start_pending = false;
                continue;
            }
            let Some(rpc) = self.resolve_queue_mutation(&mutation) else {
                self.cancel_pending_queue_mutations();
                self.queue_state.reconciliation_required = true;
                self.queue_state.hydration_due = true;
                self.push_status("queued message changed; refreshing");
                self.maybe_start_queue_hydration(app_server);
                return;
            };
            let thread_id = self.thread_id;
            let request = app_server.thread_queue_mutate_in_background(thread_id, rpc);
            self.backend_actions
                .start(Some(ActionGroup::QueueMutation), async move {
                    BackendActionResult::QueueMutation {
                        thread_id,
                        mutation,
                        result: request.await,
                    }
                });
            return;
        }
    }

    fn resolve_queue_mutation(&self, mutation: &QueueMutation) -> Option<QueueRpc> {
        match mutation {
            QueueMutation::Add {
                input,
                client_user_message_id,
                ..
            } => Some(QueueRpc::Add {
                input: input.clone(),
                client_user_message_id: client_user_message_id.clone(),
            }),
            QueueMutation::Update { target, input } => Some(QueueRpc::Update {
                queued_submission_id: self.queue_state.resolve(target)?,
                input: input.clone(),
            }),
            QueueMutation::Delete { target } => Some(QueueRpc::Delete {
                queued_submission_id: self.queue_state.resolve(target)?,
            }),
            QueueMutation::Reorder { targets } => Some(QueueRpc::Reorder {
                queued_submission_ids: targets
                    .iter()
                    .filter_map(|target| self.queue_state.resolve(target))
                    .collect(),
            }),
            QueueMutation::Start => Some(QueueRpc::Start),
        }
    }

    fn cancel_pending_queue_mutations(&mut self) {
        let pending = std::mem::take(&mut self.queue_state.pending);
        let failed_adds = pending
            .into_iter()
            .filter_map(|mutation| match mutation {
                QueueMutation::Add {
                    client_user_message_id,
                    ..
                } => Some(client_user_message_id),
                QueueMutation::Update { .. }
                | QueueMutation::Delete { .. }
                | QueueMutation::Reorder { .. }
                | QueueMutation::Start => None,
            })
            .collect::<Vec<_>>();
        for client_user_message_id in failed_adds.into_iter().rev() {
            if let Some(submission) = self
                .composer
                .remove_queued_submission_for_client(&client_user_message_id)
            {
                self.composer.restore_failed_queued_submission(&submission);
            }
        }
        self.queue_state.start_pending = false;
    }
}

fn queue_target(id: Option<String>, client_user_message_id: String) -> QueueTarget {
    id.map_or(
        QueueTarget::Client(client_user_message_id),
        QueueTarget::Id,
    )
}

fn text_input(text: String) -> Vec<UserInput> {
    vec![UserInput::Text {
        text,
        text_elements: Vec::new(),
    }]
}
