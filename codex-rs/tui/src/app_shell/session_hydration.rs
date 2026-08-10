use super::DashboardRoute;
use super::ShellState;
use super::backend::AppShellBackend;
use super::workspace;
use super::workspace::WorkspaceGitStatusProbe;
use crate::app_server_session::app_server_rate_limit_snapshots;
use codex_app_server_protocol::GetAccountRateLimitsResponse;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_protocol::ThreadId;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const SESSION_HYDRATION_LOOKUP_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 5);
type LookupResult<T> = std::result::Result<T, String>;
type LookupTask<T> = Option<JoinHandle<SessionLookup<LookupResult<T>>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionListLoad {
    Replace,
    Append,
}

struct SessionListPage {
    load: SessionListLoad,
    result: LookupResult<ThreadListResponse>,
}

#[derive(Default)]
pub(super) struct SessionHydrationState {
    generation: u64,
    goal_revision: u64,
    workspace_revision: u64,
    session_list_revision: u64,
    rate_limits_revision: u64,
    rate_limits_loaded: bool,
    rate_limits_refresh_due: bool,
    goal_task: LookupTask<Option<ThreadGoal>>,
    workspace_task: LookupTask<WorkspaceGitStatusProbe>,
    session_list_params: Option<ThreadListParams>,
    session_list_task: Option<JoinHandle<SessionLookup<SessionListPage>>>,
    rate_limits_task: LookupTask<GetAccountRateLimitsResponse>,
}

struct SessionLookup<T> {
    generation: u64,
    thread_id: ThreadId,
    revision: u64,
    value: T,
}

impl ShellState {
    pub(super) fn invalidate_session_hydration(&mut self) {
        let rate_limits_refresh_due = self.session_hydration.rate_limits_refresh_due
            || self.session_hydration.rate_limits_task.is_some();
        self.session_hydration.generation = self.session_hydration.generation.wrapping_add(1);
        self.cancel_session_hydration();
        self.session_hydration.rate_limits_refresh_due = rate_limits_refresh_due;
    }

    pub(super) fn start_replaced_session_hydration<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        self.start_goal_hydration(app_server);
        self.start_workspace_hydration();
        self.start_rate_limits_hydration(app_server);
    }

    pub(super) fn start_initial_dashboard_hydration<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        self.start_workspace_hydration();
        self.start_session_list_refresh(app_server);
        self.start_rate_limits_hydration(app_server);
    }

    pub(super) fn start_session_list_refresh<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        let params = self.session_list.first_page_params();
        self.start_session_list_lookup(app_server, params, SessionListLoad::Replace);
    }

    pub(super) fn start_session_list_next_page<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        let Some(params) = self.session_list.next_page_params() else {
            return;
        };
        self.start_session_list_lookup(app_server, params, SessionListLoad::Append);
    }

    fn start_session_list_lookup<S>(
        &mut self,
        app_server: &S,
        params: ThreadListParams,
        load: SessionListLoad,
    ) where
        S: AppShellBackend,
    {
        if self
            .session_hydration
            .session_list_task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
            && self.session_hydration.session_list_params.as_ref() == Some(&params)
        {
            return;
        }
        if let Some(task) = self.session_hydration.session_list_task.take() {
            task.abort();
        }

        let generation = self.session_hydration.generation;
        let thread_id = self.thread_id;
        let revision = self.begin_session_list_refresh();
        let lookup = app_server.thread_list_in_background(params.clone());
        self.session_hydration.session_list_params = Some(params);
        self.session_hydration.session_list_task = Some(tokio::spawn(async move {
            let value = match timeout(SESSION_HYDRATION_LOOKUP_TIMEOUT, lookup).await {
                Ok(Ok(response)) => Ok(response),
                Ok(Err(err)) => Err(err.to_string()),
                Err(_) => Err("session list refresh timed out".to_string()),
            };
            SessionLookup {
                generation,
                thread_id,
                revision,
                value: SessionListPage {
                    load,
                    result: value,
                },
            }
        }));
    }

    fn start_rate_limits_hydration<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        if self.session_hydration.rate_limits_task.is_some()
            || (self.session_hydration.rate_limits_loaded
                && !self.session_hydration.rate_limits_refresh_due)
        {
            return;
        }
        self.session_hydration.rate_limits_refresh_due = false;
        let generation = self.session_hydration.generation;
        let thread_id = self.thread_id;
        let revision = self.session_hydration.rate_limits_revision;
        let lookup = app_server.account_rate_limits_in_background();
        self.session_hydration.rate_limits_task = Some(tokio::spawn(async move {
            let value = match timeout(SESSION_HYDRATION_LOOKUP_TIMEOUT, lookup).await {
                Ok(Ok(response)) => Ok(response),
                Ok(Err(err)) => Err(err.to_string()),
                Err(_) => Err("rate-limit refresh timed out".to_string()),
            };
            SessionLookup {
                generation,
                thread_id,
                revision,
                value,
            }
        }));
    }

    pub(super) fn start_initial_goal_hydration<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        self.start_goal_hydration(app_server);
    }

    fn start_goal_hydration<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        if self.session_hydration.goal_task.is_some() {
            return;
        }
        let generation = self.session_hydration.generation;
        let thread_id = self.thread_id;
        let revision = self.session_hydration.goal_revision;
        let lookup = app_server.thread_goal_get_in_background(thread_id);
        self.session_hydration.goal_task = Some(tokio::spawn(async move {
            let value = match timeout(SESSION_HYDRATION_LOOKUP_TIMEOUT, lookup).await {
                Ok(Ok(response)) => Ok(response.goal),
                Ok(Err(err)) => Err(err.to_string()),
                Err(_) => Err("goal lookup timed out".to_string()),
            };
            SessionLookup {
                generation,
                thread_id,
                revision,
                value,
            }
        }));
    }

    fn start_workspace_hydration(&mut self) {
        if self.session_hydration.workspace_task.is_some() {
            return;
        }
        let Some(runner) = self.workspace_command_runner.clone() else {
            self.record_workspace_git_probe(WorkspaceGitStatusProbe::Unavailable);
            return;
        };
        let generation = self.session_hydration.generation;
        let thread_id = self.thread_id;
        let revision = self.session_hydration.workspace_revision;
        let cwd = std::path::PathBuf::from(&self.cwd);
        self.session_hydration.workspace_task = Some(tokio::spawn(async move {
            let value = match timeout(
                SESSION_HYDRATION_LOOKUP_TIMEOUT,
                workspace::load_git_status(runner.as_ref(), &cwd),
            )
            .await
            {
                Ok(status) => Ok(status),
                Err(_) => {
                    tracing::warn!(%thread_id, "workspace lookup timed out");
                    Ok(WorkspaceGitStatusProbe::Unavailable)
                }
            };
            SessionLookup {
                generation,
                thread_id,
                revision,
                value,
            }
        }));
    }

    pub(super) fn has_pending_session_hydration(&self) -> bool {
        self.session_hydration.goal_task.is_some()
            || self.session_hydration.workspace_task.is_some()
            || self.session_hydration.session_list_task.is_some()
            || self.session_hydration.rate_limits_task.is_some()
            || self.session_hydration.rate_limits_refresh_due
            || self.has_pending_goal_rate_limit_recovery()
    }

    pub(super) async fn poll_session_hydration<S>(&mut self, app_server: &S) -> bool
    where
        S: AppShellBackend,
    {
        let mut changed = false;
        if self
            .session_hydration
            .goal_task
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            && let Some(task) = self.session_hydration.goal_task.take()
        {
            let lookup = match task.await {
                Ok(lookup) => Some(lookup),
                Err(err) => {
                    if !err.is_cancelled() {
                        tracing::warn!(%err, "goal lookup task failed");
                    }
                    None
                }
            };
            if let Some(lookup) = lookup
                && lookup.generation == self.session_hydration.generation
                && lookup.thread_id == self.thread_id
                && lookup.revision == self.session_hydration.goal_revision
            {
                match lookup.value {
                    Ok(goal) => {
                        self.record_active_goal(goal);
                        changed = true;
                    }
                    Err(err) => tracing::warn!(%err, "failed to hydrate session goal"),
                }
            }
        }

        if self
            .session_hydration
            .workspace_task
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            && let Some(task) = self.session_hydration.workspace_task.take()
        {
            let lookup = match task.await {
                Ok(lookup) => Some(lookup),
                Err(err) => {
                    if !err.is_cancelled() {
                        tracing::warn!(%err, "workspace lookup task failed");
                    }
                    None
                }
            };
            if let Some(lookup) = lookup
                && lookup.generation == self.session_hydration.generation
                && lookup.thread_id == self.thread_id
                && lookup.revision == self.session_hydration.workspace_revision
            {
                match lookup.value {
                    Ok(probe) => {
                        self.record_workspace_git_probe(probe);
                        changed = true;
                    }
                    Err(err) => tracing::warn!(%err, "failed to refresh workspace status"),
                }
            }
        }

        if self
            .session_hydration
            .session_list_task
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            && let Some(task) = self.session_hydration.session_list_task.take()
        {
            self.session_hydration.session_list_params = None;
            let lookup = match task.await {
                Ok(lookup) => Some(lookup),
                Err(err) => {
                    if !err.is_cancelled() {
                        tracing::warn!(%err, "session list refresh task failed");
                    }
                    None
                }
            };
            if let Some(lookup) = lookup
                && lookup.generation == self.session_hydration.generation
                && lookup.thread_id == self.thread_id
            {
                changed |= self.finish_session_list_refresh(
                    lookup.revision,
                    lookup.value.load,
                    lookup.value.result,
                );
            }
        }

        if self
            .session_hydration
            .rate_limits_task
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            && let Some(task) = self.session_hydration.rate_limits_task.take()
        {
            let lookup = match task.await {
                Ok(lookup) => Some(lookup),
                Err(err) => {
                    if !err.is_cancelled() {
                        tracing::warn!(%err, "rate-limit refresh task failed");
                    }
                    None
                }
            };
            if let Some(lookup) = lookup
                && lookup.generation == self.session_hydration.generation
                && lookup.thread_id == self.thread_id
                && lookup.revision == self.session_hydration.rate_limits_revision
            {
                match lookup.value {
                    Ok(response) => {
                        self.record_rate_limit_response(response);
                        changed = true;
                    }
                    Err(err) => tracing::warn!(%err, "failed to refresh rate limits"),
                }
            }
        }

        if self.workspace_status_refresh_due && self.active_turn_id.is_none() {
            self.start_workspace_status_refresh();
        }
        if self.session_hydration.rate_limits_refresh_due
            && self.session_hydration.rate_limits_task.is_none()
        {
            self.start_rate_limits_hydration(app_server);
        }
        changed |= self.poll_goal_rate_limit_recovery().await;
        self.maybe_start_goal_rate_limit_recovery(app_server);
        changed
    }

    pub(super) fn start_workspace_status_refresh(&mut self) {
        self.start_workspace_hydration();
    }

    pub(super) fn poll_workspace_status_if_visible(&mut self) {
        if self.dashboard_visible && self.dashboard_route == DashboardRoute::Status {
            self.start_workspace_hydration();
        }
    }

    pub(super) fn cancel_session_hydration(&mut self) {
        if let Some(task) = self.session_hydration.goal_task.take() {
            task.abort();
        }
        if let Some(task) = self.session_hydration.workspace_task.take() {
            task.abort();
        }
        if let Some(task) = self.session_hydration.session_list_task.take() {
            task.abort();
        }
        self.session_hydration.session_list_params = None;
        if let Some(task) = self.session_hydration.rate_limits_task.take() {
            task.abort();
        }
        self.cancel_goal_rate_limit_recovery();
    }

    pub(super) fn invalidate_session_list_refresh(&mut self) {
        if let Some(task) = self.session_hydration.session_list_task.take() {
            task.abort();
        }
        self.session_hydration.session_list_params = None;
        self.session_hydration.session_list_revision =
            self.session_hydration.session_list_revision.wrapping_add(1);
    }

    pub(super) fn record_active_goal(&mut self, goal: Option<ThreadGoal>) {
        self.session_hydration.goal_revision = self.session_hydration.goal_revision.wrapping_add(1);
        self.active_goal = goal;
        self.goal_status_changed_for_rate_limit_recovery();
    }

    pub(super) fn record_workspace_git_probe(&mut self, probe: WorkspaceGitStatusProbe) {
        self.session_hydration.workspace_revision =
            self.session_hydration.workspace_revision.wrapping_add(1);
        match probe {
            WorkspaceGitStatusProbe::Found(status) => {
                if let Some(git_root) = status.git_root.as_deref() {
                    self.diff_store.set_git_root(git_root);
                    self.refresh_open_diff_view();
                }
                self.workspace_git_status = Some(status);
            }
            WorkspaceGitStatusProbe::NotRepository => {
                self.diff_store.confirm_no_git_root();
                self.refresh_open_diff_view();
                self.workspace_git_status = None;
            }
            WorkspaceGitStatusProbe::Unavailable => {
                self.workspace_git_status = None;
            }
        }
        self.workspace_status_refresh_due = false;
    }

    pub(super) fn reset_workspace_git_status(&mut self) {
        self.session_hydration.workspace_revision =
            self.session_hydration.workspace_revision.wrapping_add(1);
        self.workspace_git_status = None;
        self.workspace_status_refresh_due = false;
    }

    pub(super) fn mark_workspace_status_refresh_due(&mut self) {
        self.session_hydration.workspace_revision =
            self.session_hydration.workspace_revision.wrapping_add(1);
        self.workspace_status_refresh_due = true;
    }

    pub(super) fn begin_session_list_refresh(&mut self) -> u64 {
        self.session_hydration.session_list_revision =
            self.session_hydration.session_list_revision.wrapping_add(1);
        self.session_hydration.session_list_revision
    }

    pub(super) fn finish_session_list_refresh(
        &mut self,
        revision: u64,
        load: SessionListLoad,
        result: std::result::Result<ThreadListResponse, String>,
    ) -> bool {
        if revision != self.session_hydration.session_list_revision {
            return false;
        }
        match result {
            Ok(response) => match load {
                SessionListLoad::Replace => self
                    .session_list
                    .replace_thread_page(response.data, response.next_cursor),
                SessionListLoad::Append => self
                    .session_list
                    .append_thread_page(response.data, response.next_cursor),
            },
            Err(err) => self.session_list.set_error(err),
        }
        true
    }

    pub(super) fn record_rate_limit_response(&mut self, response: GetAccountRateLimitsResponse) {
        self.rate_limits_refreshed_for_goal_recovery(&response);
        self.session_hydration.rate_limits_loaded = true;
        self.session_hydration.rate_limits_refresh_due = false;
        self.session_hydration.rate_limits_revision =
            self.session_hydration.rate_limits_revision.wrapping_add(1);
        self.rate_limit_reset_credits = response
            .rate_limit_reset_credits
            .as_ref()
            .map(|credits| credits.available_count);
        self.rate_limits = app_server_rate_limit_snapshots(response);
    }

    pub(super) fn request_rate_limits_refresh(&mut self) {
        self.session_hydration.rate_limits_refresh_due = true;
        self.session_hydration.rate_limits_revision =
            self.session_hydration.rate_limits_revision.wrapping_add(1);
    }
}
