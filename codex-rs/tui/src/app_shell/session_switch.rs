use super::ShellState;
use super::backend::AppShellBackend;
use crate::app_server_session::AppServerStartedThread;
use crate::legacy_core::config::Config;
use codex_app_server_protocol::ThreadStartSource;
use codex_protocol::ThreadId;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use std::path::PathBuf;

impl ShellState {
    pub(super) async fn start_new_session<S>(
        &mut self,
        config: &Config,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        if self.block_session_switch_if_busy() {
            return Ok(());
        }

        let mut session_config = config.clone();
        session_config.model = Some(self.model.clone());
        session_config.model_reasoning_effort = self.reasoning_effort.clone();
        session_config.service_tier = self.service_tier.clone();
        session_config.personality = self.personality;
        session_config.approvals_reviewer = self.approvals_reviewer;
        session_config.cwd = AbsolutePathBuf::from_absolute_path(PathBuf::from(&self.cwd))
            .wrap_err("current session cwd is not absolute")?;
        session_config.workspace_roots = self.runtime_workspace_roots.clone();
        session_config.workspace_roots_explicit = true;
        session_config
            .permissions
            .approval_policy
            .set(self.approval_policy.to_core())
            .wrap_err("current approval policy is not allowed for a new session")?;

        self.finish_subscription_cleanup().await;
        let started = app_server
            .start_thread_with_session_start_source(&session_config, Some(ThreadStartSource::Clear))
            .await?;
        self.complete_session_switch(started, app_server).await;
        self.session_list.focused = false;
        self.settings.focused = false;
        self.agents_focused = false;
        Ok(())
    }

    pub(super) async fn complete_session_switch<S>(
        &mut self,
        started: AppServerStartedThread,
        app_server: &S,
    ) where
        S: AppShellBackend,
    {
        self.cancel_agent_history().await;
        let previous_thread_ids = self.tracked_thread_ids();
        self.replace_started_session(started);
        self.prepare_replaced_session_cleanup(app_server, previous_thread_ids);
        self.start_replaced_session_hydration(app_server);
        self.start_session_list_refresh(app_server);
    }

    pub(super) async fn resume_session<S>(
        &mut self,
        config: &Config,
        app_server: &mut S,
        thread_id: ThreadId,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        if self.block_session_switch_if_busy() {
            return Ok(());
        }
        if thread_id == self.thread_id {
            self.push_status("session is already open");
            return Ok(());
        }
        self.finish_subscription_cleanup().await;
        let started = app_server.resume_thread(config.clone(), thread_id).await?;
        self.complete_session_switch(started, app_server).await;
        Ok(())
    }
}
