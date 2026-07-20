use super::ShellState;
use super::backend::AppShellBackend;
use crate::app_server_session::AgentHistoryTask;
use crate::app_server_session::AgentHistoryUpdate;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::ThreadId;
use std::collections::HashSet;

const MAX_AGENT_HISTORY_UPDATES_PER_TICK: usize = 4;
const MAX_ACTIVE_AGENT_THREADS: usize = 256;

impl ShellState {
    pub(super) fn install_agent_history(
        &mut self,
        threads: Vec<Thread>,
        mut task: Option<AgentHistoryTask>,
    ) {
        debug_assert!(self.agent_history_task.is_none());
        // Child hydration is bounded and omits file-change items, so it cannot reconcile the
        // session edit history even when every child request succeeds.
        if !threads.is_empty() || task.is_some() {
            self.diff_store.mark_history_truncated();
            self.refresh_open_diff_view();
        }
        self.agent_activity.hydrate_threads(threads);
        if let Some(task) = &mut task {
            task.start();
        } else {
            self.agent_activity.finish_history_hydration();
        }
        self.agent_history_task = task;
    }

    pub(super) fn has_pending_agent_history(&self) -> bool {
        self.agent_history_task.is_some()
    }

    pub(super) fn drain_agent_history_updates(&mut self) -> bool {
        let updates = self
            .agent_history_task
            .as_mut()
            .map(|task| task.drain_updates(MAX_AGENT_HISTORY_UPDATES_PER_TICK))
            .unwrap_or_default();
        let changed = !updates.is_empty();
        for update in updates {
            self.apply_agent_history_update(update);
        }
        changed
    }

    pub(super) async fn poll_agent_history<S>(&mut self, app_server: &S) -> bool
    where
        S: AppShellBackend,
    {
        let changed = self.drain_agent_history_updates();
        let task_ready = self
            .agent_history_task
            .as_ref()
            .is_some_and(|task| task.is_finished() && task.updates_empty());
        if !task_ready {
            return changed;
        }
        let Some(task) = self.agent_history_task.take() else {
            return changed;
        };
        match task.finish().await {
            Ok(()) => {}
            Err(err) if err.is_cancelled() => {}
            Err(err) => tracing::warn!(%err, "resumed agent history task failed"),
        }
        self.agent_activity.finish_history_hydration();
        self.flush_deferred_unsubscribes(app_server);
        true
    }

    pub(super) async fn cancel_agent_history(&mut self) {
        if let Some(task) = self.agent_history_task.take() {
            self.active_agent_thread_ids.extend(task.cancel().await);
        }
    }

    pub(super) fn tracked_thread_ids(&self) -> Vec<ThreadId> {
        let task_thread_ids = self
            .agent_history_task
            .as_ref()
            .map(AgentHistoryTask::subscribed_thread_ids)
            .unwrap_or_default();
        let mut seen = HashSet::new();
        std::iter::once(self.thread_id)
            .chain(
                self.active_agent_thread_ids
                    .iter()
                    .chain(&task_thread_ids)
                    .filter_map(|thread_id| ThreadId::from_string(thread_id).ok()),
            )
            .chain(self.deferred_unsubscribe_thread_ids.iter().copied())
            .filter(|thread_id| seen.insert(thread_id.to_string()))
            .collect()
    }

    pub(super) fn prepare_replaced_session_cleanup<S>(
        &mut self,
        app_server: &S,
        previous_thread_ids: Vec<ThreadId>,
    ) where
        S: AppShellBackend,
    {
        let retained = std::iter::once(self.thread_id.to_string())
            .chain(self.active_agent_thread_ids.iter().cloned())
            .collect::<HashSet<_>>();
        let history_pending = self.has_pending_agent_history();
        let mut immediate = Vec::new();
        let mut deferred = Vec::new();
        for thread_id in previous_thread_ids {
            if retained.contains(&thread_id.to_string()) {
                continue;
            }
            if !history_pending {
                immediate.push(thread_id);
            } else {
                deferred.push(thread_id);
            }
        }
        self.deferred_unsubscribe_thread_ids = deferred;
        self.start_subscription_cleanup(app_server, immediate);
    }

    pub(super) fn is_active_agent_thread(&self, thread_id: &str) -> bool {
        self.active_agent_thread_ids.contains(thread_id)
            || self
                .agent_history_task
                .as_ref()
                .is_some_and(|task| task.is_subscribed(thread_id))
    }

    pub(super) fn prepare_active_agent_thread(&mut self, thread_id: &str) -> bool {
        if !self.is_active_agent_thread(thread_id) {
            return false;
        }
        self.agent_activity.ensure_thread(thread_id);
        true
    }

    pub(super) fn mark_active_agent_threads(&mut self, item: &ThreadItem) {
        match item {
            ThreadItem::CollabAgentToolCall {
                receiver_thread_ids,
                agents_states,
                ..
            } => {
                for thread_id in receiver_thread_ids.iter().chain(agents_states.keys()) {
                    self.mark_active_agent_thread(thread_id);
                }
            }
            ThreadItem::SubAgentActivity {
                agent_thread_id, ..
            } => {
                self.mark_active_agent_thread(agent_thread_id);
            }
            _ => {}
        }
    }

    pub(super) fn mark_agent_item_live(&mut self, item: &ThreadItem) {
        match item {
            ThreadItem::CollabAgentToolCall {
                receiver_thread_ids,
                agents_states,
                ..
            } => {
                for thread_id in receiver_thread_ids.iter().chain(agents_states.keys()) {
                    self.agent_activity.mark_live_thread(thread_id);
                }
            }
            ThreadItem::SubAgentActivity {
                agent_thread_id, ..
            } => self.agent_activity.mark_live_thread(agent_thread_id),
            _ => {}
        }
    }

    fn apply_agent_history_update(&mut self, update: AgentHistoryUpdate) {
        match update {
            AgentHistoryUpdate::Discovered(snapshot) | AgentHistoryUpdate::Loaded(snapshot) => {
                self.agent_activity.hydrate_snapshots(vec![snapshot]);
            }
            AgentHistoryUpdate::Subscribed(thread_id) => {
                self.mark_active_agent_thread(&thread_id);
            }
        }
    }

    fn mark_active_agent_thread(&mut self, thread_id: &str) {
        if self.active_agent_thread_ids.len() < MAX_ACTIVE_AGENT_THREADS
            || self.active_agent_thread_ids.contains(thread_id)
        {
            self.active_agent_thread_ids.insert(thread_id.to_string());
        } else {
            tracing::warn!(%thread_id, "active agent subscription limit reached");
        }
    }

    fn flush_deferred_unsubscribes<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        let retained = std::iter::once(self.thread_id.to_string())
            .chain(self.active_agent_thread_ids.iter().cloned())
            .collect::<HashSet<_>>();
        let thread_ids = std::mem::take(&mut self.deferred_unsubscribe_thread_ids)
            .into_iter()
            .filter(|thread_id| !retained.contains(&thread_id.to_string()))
            .collect();
        self.start_subscription_cleanup(app_server, thread_ids);
    }

    pub(super) async fn finish_subscription_cleanup(&mut self) {
        let Some(task) = self.subscription_cleanup_task.take() else {
            return;
        };
        if let Err(err) = task.await {
            tracing::warn!(%err, "thread subscription cleanup task failed");
        }
    }

    fn start_subscription_cleanup<S>(&mut self, app_server: &S, thread_ids: Vec<ThreadId>)
    where
        S: AppShellBackend,
    {
        if thread_ids.is_empty() {
            return;
        }
        debug_assert!(self.subscription_cleanup_task.is_none());
        self.subscription_cleanup_task =
            Some(app_server.unsubscribe_threads_in_background(thread_ids));
    }
}
