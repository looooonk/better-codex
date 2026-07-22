use super::DashboardRoute;
use super::ShellState;
use codex_app_server_protocol::ThreadStatus;
use codex_protocol::ThreadId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RemoteThreadLifecycle {
    Archived,
    Deleted,
    Unarchived,
    Closed,
}

impl RemoteThreadLifecycle {
    fn unavailable_reason(self) -> &'static str {
        match self {
            Self::Archived => "archived remotely",
            Self::Deleted => "deleted remotely",
            Self::Unarchived => "unarchived remotely and must be resumed",
            Self::Closed => "closed remotely",
        }
    }

    fn status(self) -> &'static str {
        match self {
            Self::Archived => "session archived",
            Self::Deleted => "session deleted",
            Self::Unarchived => "session must be resumed",
            Self::Closed => "session closed",
        }
    }
}

impl ShellState {
    pub(super) fn handle_remote_thread_status(&mut self, thread_id: &str, status: ThreadStatus) {
        if thread_id != self.thread_id.to_string() || self.session_unavailable_reason.is_some() {
            return;
        }
        match status {
            ThreadStatus::NotLoaded => {
                self.mark_active_session_unavailable(RemoteThreadLifecycle::Closed)
            }
            ThreadStatus::Idle => self.status = "ready".to_string(),
            ThreadStatus::SystemError => self.status = "error".to_string(),
            ThreadStatus::Active { active_flags } => {
                self.status = if active_flags.is_empty() {
                    "thinking"
                } else {
                    "waiting"
                }
                .to_string();
            }
        }
    }

    pub(super) fn handle_remote_thread_lifecycle(
        &mut self,
        thread_id: &str,
        lifecycle: RemoteThreadLifecycle,
    ) {
        let Ok(thread_id) = ThreadId::from_string(thread_id) else {
            tracing::warn!(
                thread_id,
                "ignored lifecycle notification with invalid thread id"
            );
            return;
        };
        match lifecycle {
            RemoteThreadLifecycle::Archived if !self.session_list.show_archived() => {
                self.session_list.remove_thread(thread_id);
            }
            RemoteThreadLifecycle::Deleted => self.session_list.remove_thread(thread_id),
            RemoteThreadLifecycle::Unarchived if self.session_list.show_archived() => {
                self.session_list.remove_thread(thread_id);
            }
            RemoteThreadLifecycle::Archived
            | RemoteThreadLifecycle::Unarchived
            | RemoteThreadLifecycle::Closed => {}
        }
        if thread_id != self.thread_id
            || lifecycle == RemoteThreadLifecycle::Unarchived
                && self.session_unavailable_reason.is_none()
        {
            return;
        }
        self.mark_active_session_unavailable(lifecycle);
    }

    pub(super) fn reject_unavailable_session_action(&mut self) -> bool {
        let Some(reason) = self.session_unavailable_reason else {
            return false;
        };
        self.push_error(format!(
            "active session was {reason}; choose or start another session"
        ));
        true
    }

    fn mark_active_session_unavailable(&mut self, lifecycle: RemoteThreadLifecycle) {
        let reason = lifecycle.unavailable_reason();
        let was_available = self.session_unavailable_reason.replace(reason).is_none();
        self.finish_streaming_plan();
        self.finish_streaming_assistant();
        self.clear_active_turn();
        self.clear_interactive_requests();
        self.pending_session_delete = None;
        self.safety_buffering.clear();
        self.close_agent_log();
        self.close_tool_output();
        self.close_diff_view();
        self.selector = None;
        self.command_palette = None;
        self.dashboard_visible = true;
        self.dashboard_route = DashboardRoute::Sessions;
        self.dashboard_scroll.set(0);
        self.settings.focused = false;
        self.agents_focused = false;
        self.session_list.focused = true;
        let message = format!("active session was {reason}; choose or start another session");
        if was_available {
            self.push_error(message);
        } else {
            self.push_status(message);
        }
        self.status = lifecycle.status().to_string();
    }
}
