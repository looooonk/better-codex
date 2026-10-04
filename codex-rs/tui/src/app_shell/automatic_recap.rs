use super::ShellState;
use super::backend::AppShellBackend;
use super::recap::RecapTrigger;
use super::workspace_requests::WorkspaceResponse;
use super::workspace_requests::workspace_request_id;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigValueWriteParams;
use codex_app_server_protocol::ConfigWriteResponse;
use codex_app_server_protocol::MergeStrategy;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnStatus;
use color_eyre::Result;
use std::time::Duration;
use std::time::Instant;

const RECAP_DELAY: Duration = Duration::from_secs(/*secs*/ 30 * 60);
const RETRY_DELAY: Duration = Duration::from_secs(/*secs*/ 30);

pub(super) struct AutomaticRecap {
    pub(super) enabled: bool,
    pub(super) revision: usize,
    unfocused_since: Option<Instant>,
    last_turn_finished: Option<Instant>,
    completed_turns: usize,
    last_recapped: Option<usize>,
    attempted_revision: Option<usize>,
    retry_at: Option<Instant>,
    retry_used: bool,
}

impl Default for AutomaticRecap {
    fn default() -> Self {
        Self {
            enabled: true,
            revision: 0,
            unfocused_since: None,
            last_turn_finished: None,
            completed_turns: 0,
            last_recapped: None,
            attempted_revision: None,
            retry_at: None,
            retry_used: false,
        }
    }
}

impl AutomaticRecap {
    pub(super) fn seed(&mut self, turns: &[Turn], now: Instant) {
        self.completed_turns = self.completed_turns.max(
            turns
                .iter()
                .filter(|turn| turn.status == TurnStatus::Completed)
                .count(),
        );
        if self.completed_turns > 0 {
            self.last_turn_finished.get_or_insert(now);
        }
    }

    pub(super) fn reset(&mut self) {
        *self = Self {
            enabled: self.enabled,
            unfocused_since: self.unfocused_since.map(|_| Instant::now()),
            ..Self::default()
        };
    }

    pub(super) fn note_focus(&mut self, focused: bool, now: Instant) {
        if focused {
            self.unfocused_since = None;
            self.retry_at = None;
        } else if self.unfocused_since.is_none() {
            self.unfocused_since = Some(now);
            self.attempted_revision = None;
            self.retry_used = false;
        }
    }

    pub(super) fn note_turn_finished(&mut self, status: &TurnStatus, now: Instant) {
        if *status == TurnStatus::InProgress {
            return;
        }
        self.completed_turns += usize::from(*status == TurnStatus::Completed);
        self.revision += 1;
        self.last_turn_finished = Some(now);
        self.retry_at = None;
        self.retry_used = false;
    }

    pub(super) fn ready(&self, now: Instant) -> bool {
        if !self.enabled
            || self.completed_turns < 3
            || self
                .last_recapped
                .is_some_and(|previous| self.completed_turns.saturating_sub(previous) < 2)
        {
            return false;
        }
        let (Some(unfocused), Some(finished)) = (self.unfocused_since, self.last_turn_finished)
        else {
            return false;
        };
        if self.attempted_revision == Some(self.revision) {
            return self.retry_at.is_some_and(|retry| now >= retry);
        }
        now >= unfocused.max(finished) + RECAP_DELAY
    }

    pub(super) fn mark_started(&mut self) {
        self.attempted_revision = Some(self.revision);
        self.retry_at = None;
    }
    pub(super) fn mark_recapped(&mut self) {
        self.last_recapped = Some(self.completed_turns);
        self.retry_at = None;
    }
    pub(super) fn mark_failed(&mut self, now: Instant) {
        if !self.retry_used {
            self.retry_used = true;
            self.retry_at = Some(now + RETRY_DELAY);
        }
    }
}

impl ShellState {
    pub(super) fn poll_automatic_recap<S: AppShellBackend>(
        &mut self,
        focused: bool,
        config: &crate::legacy_core::config::Config,
        app_server: &S,
    ) {
        let now = Instant::now();
        self.automatic_recap.note_focus(focused, now);
        if focused || !self.automatic_recap.enabled {
            self.recap.cancel_automatic();
            return;
        }
        if self.side_parent.is_some()
            || self.pending_worktree.is_some()
            || self.voice.has_work()
            || self.pending_shell_command.is_some()
            || self.recap.has_work()
            || self.active_turn_id.is_some()
            || self.has_pending_backend_actions()
            || !self.automatic_recap.ready(now)
        {
            return;
        }
        self.automatic_recap.mark_started();
        if let Err(error) = self.start_recap(config, app_server, RecapTrigger::Automatic) {
            tracing::debug!(%error, "automatic recap could not start");
            self.automatic_recap.mark_failed(now);
        }
    }
}

pub(super) async fn persist(
    client: AppServerRequestHandle,
    enabled: bool,
) -> Result<WorkspaceResponse> {
    let _: ConfigWriteResponse = client
        .request_typed(ClientRequest::ConfigValueWrite {
            request_id: workspace_request_id("auto-recap"),
            params: ConfigValueWriteParams {
                key_path: "tui.auto_recap".to_string(),
                value: enabled.into(),
                merge_strategy: MergeStrategy::Replace,
                file_path: None,
                expected_version: None,
            },
        })
        .await?;
    Ok(WorkspaceResponse::AutomaticRecap(enabled))
}

#[cfg(test)]
#[path = "automatic_recap_tests.rs"]
mod tests;
