use super::ShellState;
use super::backend::AppShellBackend;
use super::backend::AppShellTurnStart;
use super::settings::SettingsUpdate;
use crate::app_server_session::AppServerStartedThread;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::TurnStartResponse;
use codex_protocol::ThreadId;
use color_eyre::Result;
use std::collections::HashSet;
use std::future::Future;
use tokio::task::JoinSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ActionGroup {
    Approval,
    SessionDelete,
    SessionRename,
    SessionSwitch,
    Settings,
    TurnStart,
    UserInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnSubmission {
    Initial,
    Interactive,
    Queued,
}

#[derive(Debug)]
pub(super) enum BackendActionResult {
    Approval {
        request_id: RequestId,
        title: String,
        decision: &'static str,
        edit_prompt: Option<String>,
        result: std::io::Result<()>,
    },
    CurrentTime {
        result: std::io::Result<()>,
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
    TurnStart {
        params: AppShellTurnStart,
        prompt: String,
        submission: TurnSubmission,
        result: Result<TurnStartResponse>,
    },
}

#[derive(Debug)]
struct CompletedAction {
    group: Option<ActionGroup>,
    result: BackendActionResult,
}

#[derive(Default)]
pub(super) struct BackendActions {
    groups: HashSet<ActionGroup>,
    tasks: JoinSet<CompletedAction>,
}

impl BackendActions {
    pub(super) fn start<F>(&mut self, group: Option<ActionGroup>, future: F) -> bool
    where
        F: Future<Output = BackendActionResult> + Send + 'static,
    {
        if group.is_some_and(|group| !self.groups.insert(group)) {
            return false;
        }
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
        match self.tasks.try_join_next()? {
            Ok(completed) => {
                if let Some(group) = completed.group {
                    self.groups.remove(&group);
                }
                Some(Ok(completed.result))
            }
            Err(err) => {
                self.groups.clear();
                Some(Err(err.into()))
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
                Err(err) => self.report_action_error("background action failed", err),
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
                title,
                decision,
                edit_prompt,
                result,
            } => match result {
                Ok(()) => {
                    if self
                        .pending_approval
                        .as_ref()
                        .is_some_and(|pending| pending.request_id() == request_id)
                    {
                        self.pending_approval = None;
                    }
                    if let Some(edit_prompt) = edit_prompt {
                        self.seed_composer_with_edit_prompt(edit_prompt);
                    }
                    self.push_decision_audit("approval", decision, &title);
                    self.status = "ready".to_string();
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
            BackendActionResult::UserInputAutoResolution {
                request_id,
                title,
                result,
            } => match result {
                Ok(()) => {
                    if self
                        .pending_user_input
                        .as_ref()
                        .is_some_and(|pending| pending.request_id() == &request_id)
                    {
                        self.pending_user_input = None;
                        self.composer.clear();
                        self.push_decision_audit("tool input", "auto-resolved", &title);
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
        }
    }

    pub(super) fn report_action_error(&mut self, context: &str, error: color_eyre::Report) {
        self.status = "action failed".to_string();
        self.push_error(format!("{context}: {error:#}"));
    }
}
