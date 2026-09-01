use super::ShellState;
use super::backend::AppShellBackend;
use super::backend::AppShellTurnStart;
use super::queued_messages::QueueMutation;
use super::queued_messages::QueueRpcResponse;
use super::settings::SettingsUpdate;
use crate::app_server_session::AppServerStartedThread;
use codex_app_server_protocol::QueuedSubmission;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::TurnStartResponse;
use codex_protocol::ThreadId;
use color_eyre::Result;
use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use tokio::task::JoinSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ActionGroup {
    Approval,
    Compaction,
    ConversationBranch,
    SessionDelete,
    SessionRename,
    SessionSwitch,
    Settings,
    TurnStart,
    UserInput,
    QueueHydration,
    QueueMutation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnSubmission {
    Initial,
    Interactive,
}

#[derive(Debug)]
pub(super) enum BackendActionResult {
    Approval {
        request_id: RequestId,
        edit_prompt: Option<String>,
        result: std::io::Result<()>,
    },
    CurrentTime {
        result: std::io::Result<()>,
    },
    Compaction {
        result: Result<()>,
    },
    ConversationFork {
        point: super::rewind::RewindPoint,
        prompt: String,
        result: Result<AppServerStartedThread>,
    },
    UserInputAutoResolution {
        request_id: RequestId,
        title: String,
        result: std::io::Result<()>,
    },
    DescendantCount {
        thread_id: ThreadId,
        title: String,
        result: Result<usize>,
    },
    SessionDelete {
        thread_id: ThreadId,
        result: Result<()>,
    },
    SessionRename {
        thread_id: ThreadId,
        name: String,
        result: Result<()>,
    },
    SessionResume {
        result: Result<AppServerStartedThread>,
    },
    Settings {
        update: SettingsUpdate,
        result: Result<()>,
    },
    QueueHydration {
        thread_id: ThreadId,
        result: Result<Vec<QueuedSubmission>>,
    },
    QueueMutation {
        thread_id: ThreadId,
        mutation: QueueMutation,
        result: Result<QueueRpcResponse>,
    },
    TurnStart {
        params: AppShellTurnStart,
        prompt: String,
        submission: TurnSubmission,
        result: Result<TurnStartResponse>,
    },
}

#[derive(Debug)]
struct CompletedAction {
    group: Option<(ActionGroup, u64)>,
    result: BackendActionResult,
}

#[derive(Default)]
pub(super) struct BackendActions {
    groups: HashSet<ActionGroup>,
    group_revisions: HashMap<ActionGroup, u64>,
    tasks: JoinSet<CompletedAction>,
}

impl BackendActions {
    pub(super) fn invalidate(&mut self, groups: impl IntoIterator<Item = ActionGroup>) {
        for group in groups {
            self.groups.remove(&group);
            let revision = self.group_revisions.entry(group).or_default();
            *revision = revision.wrapping_add(1);
        }
    }

    pub(super) fn start<F>(&mut self, group: Option<ActionGroup>, future: F) -> bool
    where
        F: Future<Output = BackendActionResult> + Send + 'static,
    {
        let group = match group {
            Some(group) if !self.groups.insert(group) => return false,
            Some(group) => Some((
                group,
                self.group_revisions
                    .get(&group)
                    .copied()
                    .unwrap_or_default(),
            )),
            None => None,
        };
        self.tasks.spawn(async move {
            CompletedAction {
                group,
                result: future.await,
            }
        });
        true
    }

    pub(super) fn is_pending(&self) -> bool {
        !self.tasks.is_empty()
    }

    pub(super) fn contains(&self, group: ActionGroup) -> bool {
        self.groups.contains(&group)
    }

    fn try_next(&mut self) -> Option<Result<BackendActionResult>> {
        loop {
            match self.tasks.try_join_next()? {
                Ok(completed) => {
                    if let Some((group, revision)) = completed.group {
                        if self
                            .group_revisions
                            .get(&group)
                            .copied()
                            .unwrap_or_default()
                            != revision
                        {
                            continue;
                        }
                        self.groups.remove(&group);
                    }
                    return Some(Ok(completed.result));
                }
                Err(err) => {
                    self.groups.clear();
                    return Some(Err(err.into()));
                }
            }
        }
    }
}

impl ShellState {
    pub(super) fn start_backend_action<F>(
        &mut self,
        group: ActionGroup,
        description: &'static str,
        future: F,
    ) -> bool
    where
        F: Future<Output = BackendActionResult> + Send + 'static,
    {
        if self.backend_actions.start(Some(group), future) {
            self.status = description.to_string();
            true
        } else {
            self.push_status(format!("{description} is already in progress"));
            false
        }
    }

    pub(super) fn has_pending_backend_actions(&self) -> bool {
        self.backend_actions.is_pending()
    }

    pub(super) fn has_pending_backend_action(&self, group: ActionGroup) -> bool {
        self.backend_actions.contains(group)
    }

    pub(super) async fn poll_backend_actions<S>(&mut self, app_server: &S) -> bool
    where
        S: AppShellBackend,
    {
        let mut changed = false;
        while let Some(completion) = self.backend_actions.try_next() {
            changed = true;
            match completion {
                Ok(result) => self.complete_backend_action(app_server, result).await,
                Err(err) => {
                    self.recover_rewind_after_background_failure();
                    self.report_action_error("background action failed", err);
                }
            }
        }
        changed
    }

    async fn complete_backend_action<S>(&mut self, app_server: &S, action: BackendActionResult)
    where
        S: AppShellBackend,
    {
        match action {
            BackendActionResult::TurnStart {
                params,
                prompt,
                submission,
                result,
            } => self.complete_turn_start(app_server, params, prompt, submission, result),
            BackendActionResult::SessionResume { result } => match result {
                Ok(started) => {
                    self.complete_session_switch(started, app_server).await;
                    self.status = "ready".to_string();
                }
                Err(err) => self.report_action_error("failed to resume session", err),
            },
            BackendActionResult::DescendantCount {
                thread_id,
                title,
                result,
            } => self.complete_session_delete_inspection(thread_id, title, result),
            BackendActionResult::SessionDelete { thread_id, result } => {
                self.complete_session_delete(thread_id, result)
            }
            BackendActionResult::SessionRename {
                thread_id,
                name,
                result,
            } => self.complete_session_rename(thread_id, name, result),
            BackendActionResult::Approval {
                request_id,
                edit_prompt,
                result,
            } => match result {
                Ok(()) => {
                    let removal = self.remove_interactive_request(&request_id);
                    if let Some(edit_prompt) = edit_prompt {
                        self.seed_composer_with_edit_prompt(edit_prompt);
                    }
                    self.status = "ready".to_string();
                    if removal == super::InteractiveRequestRemoval::Active {
                        self.activate_next_interactive_request();
                    }
                }
                Err(err) => self.report_action_error(
                    "failed to resolve app-server approval request",
                    err.into(),
                ),
            },
            BackendActionResult::CurrentTime { result } => {
                if let Err(err) = result {
                    self.report_action_error("failed to report current time", err.into());
                }
            }
            BackendActionResult::Compaction { result } => match result {
                Ok(()) => {
                    self.status = "context compaction started".to_string();
                    self.push_status("context compaction started");
                }
                Err(err) => self.report_action_error("failed to start context compaction", err),
            },
            BackendActionResult::ConversationFork {
                point,
                prompt,
                result,
            } => {
                self.complete_rewind_fork(app_server, point, prompt, result)
                    .await
            }
            BackendActionResult::UserInputAutoResolution {
                request_id,
                title,
                result,
            } => match result {
                Ok(()) => {
                    let removal = self.remove_interactive_request(&request_id);
                    if removal == super::InteractiveRequestRemoval::Active {
                        self.push_decision_audit("tool input", "auto-resolved", &title);
                        self.activate_next_interactive_request();
                    }
                }
                Err(err) => self.report_action_error(
                    "failed to auto-resolve app-server tool input request",
                    err.into(),
                ),
            },
            BackendActionResult::Settings { update, result } => {
                self.complete_settings_update(update, result)
            }
            BackendActionResult::QueueHydration { thread_id, result } => {
                self.complete_queue_hydration(app_server, thread_id, result)
            }
            BackendActionResult::QueueMutation {
                thread_id,
                mutation,
                result,
            } => self.complete_queue_mutation(app_server, thread_id, mutation, result),
        }
    }

    pub(super) fn report_action_error(&mut self, context: &str, error: color_eyre::Report) {
        self.status = "action failed".to_string();
        self.push_error(format!("{context}: {error:#}"));
    }
}
