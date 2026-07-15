use super::ShellState;
use super::backend::AppShellBackend;
use super::workspace;
use super::workspace::WorkspaceGitStatus;
use codex_app_server_protocol::ThreadGoal;
use codex_protocol::ThreadId;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const SESSION_HYDRATION_LOOKUP_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 5);

#[derive(Default)]
pub(super) struct SessionHydrationState {
    generation: u64,
    goal_revision: u64,
    workspace_revision: u64,
    task: Option<JoinHandle<SessionHydration>>,
}

struct SessionHydration {
    generation: u64,
    thread_id: ThreadId,
    goal_revision: u64,
    workspace_revision: u64,
    goal: Option<ThreadGoal>,
    workspace_git_status: Option<WorkspaceGitStatus>,
}

impl ShellState {
    pub(super) fn invalidate_session_hydration(&mut self) {
        self.session_hydration.generation = self.session_hydration.generation.wrapping_add(1);
        self.cancel_session_hydration();
    }

    pub(super) fn start_replaced_session_hydration<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        debug_assert!(self.session_hydration.task.is_none());
        let generation = self.session_hydration.generation;
        let thread_id = self.thread_id;
        let goal_revision = self.session_hydration.goal_revision;
        let workspace_revision = self.session_hydration.workspace_revision;
        let goal_lookup = app_server.thread_goal_get_in_background(thread_id);
        let workspace_command_runner = self.workspace_command_runner.clone();
        let cwd = std::path::PathBuf::from(&self.cwd);
        self.session_hydration.task = Some(tokio::spawn(async move {
            let workspace_lookup = async move {
                match workspace_command_runner {
                    Some(runner) => match timeout(
                        SESSION_HYDRATION_LOOKUP_TIMEOUT,
                        workspace::load_git_status(runner.as_ref(), &cwd),
                    )
                    .await
                    {
                        Ok(status) => status,
                        Err(_) => {
                            tracing::warn!(%thread_id, "replaced session workspace lookup timed out");
                            None
                        }
                    },
                    None => None,
                }
            };
            let (goal_result, workspace_git_status) = tokio::join!(
                timeout(SESSION_HYDRATION_LOOKUP_TIMEOUT, goal_lookup),
                workspace_lookup
            );
            let goal = match goal_result {
                Ok(Ok(response)) => response.goal,
                Ok(Err(err)) => {
                    tracing::warn!(%err, %thread_id, "failed to hydrate replaced session goal");
                    None
                }
                Err(_) => {
                    tracing::warn!(%thread_id, "replaced session goal lookup timed out");
                    None
                }
            };
            SessionHydration {
                generation,
                thread_id,
                goal_revision,
                workspace_revision,
                goal,
                workspace_git_status,
            }
        }));
    }

    pub(super) fn has_pending_session_hydration(&self) -> bool {
        self.session_hydration.task.is_some()
    }

    pub(super) async fn poll_session_hydration(&mut self) -> bool {
        let Some(task) = self.session_hydration.task.as_ref() else {
            return false;
        };
        if !task.is_finished() {
            return false;
        }
        let Some(task) = self.session_hydration.task.take() else {
            return false;
        };
        let hydration = match task.await {
            Ok(hydration) => hydration,
            Err(err) => {
                if !err.is_cancelled() {
                    tracing::warn!(%err, "replaced session hydration task failed");
                }
                return false;
            }
        };
        if hydration.generation != self.session_hydration.generation
            || hydration.thread_id != self.thread_id
        {
            return false;
        }
        let mut changed = false;
        if hydration.goal_revision == self.session_hydration.goal_revision {
            self.active_goal = hydration.goal;
            changed = true;
        }
        if hydration.workspace_revision == self.session_hydration.workspace_revision {
            self.workspace_git_status = hydration.workspace_git_status;
            self.workspace_status_refresh_due = false;
            changed = true;
        }
        changed
    }

    pub(super) fn cancel_session_hydration(&mut self) {
        if let Some(task) = self.session_hydration.task.take() {
            task.abort();
        }
    }

    pub(super) fn record_active_goal(&mut self, goal: Option<ThreadGoal>) {
        self.session_hydration.goal_revision = self.session_hydration.goal_revision.wrapping_add(1);
        self.active_goal = goal;
    }

    pub(super) fn record_workspace_git_status(&mut self, status: Option<WorkspaceGitStatus>) {
        self.session_hydration.workspace_revision =
            self.session_hydration.workspace_revision.wrapping_add(1);
        self.workspace_git_status = status;
        self.workspace_status_refresh_due = false;
    }

    pub(super) fn mark_workspace_status_refresh_due(&mut self) {
        self.session_hydration.workspace_revision =
            self.session_hydration.workspace_revision.wrapping_add(1);
        self.workspace_status_refresh_due = true;
    }
}
