use super::ShellState;
use super::agent_activity::AgentLifecycleStatus;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use crate::app_server_session::AppServerStartedThread;
use crate::app_server_session::check_worktree_source;
use crate::app_server_session::start_managed_worktree_thread;
use crate::legacy_core::config::Config;
use crate::managed_worktree::PreparedWorktree;
use crate::managed_worktree::WorktreeMode;
use crate::terminal_visualization_instructions::with_terminal_visualization_instructions;
use codex_protocol::ThreadId;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use std::path::PathBuf;
use std::time::Duration;
use tokio::task::JoinHandle;

pub(super) struct PendingWorktree {
    thread_id: ThreadId,
    cwd: PathBuf,
    mode: WorktreeMode,
    task: WorktreeTask,
}

enum WorktreeTask {
    Creating(JoinHandle<Result<PreparedWorktree>>),
    Starting {
        prepared: Box<PreparedWorktree>,
        request: JoinHandle<Result<AppServerStartedThread>>,
    },
}

impl PendingWorktree {
    fn is_finished(&self) -> bool {
        match &self.task {
            WorktreeTask::Creating(task) => task.is_finished(),
            WorktreeTask::Starting { request, .. } => request.is_finished(),
        }
    }
}

impl ShellState {
    pub(super) fn run_worktree_command<S: AppShellBackend>(
        &mut self,
        args: &str,
        config: &Config,
        app_server: &S,
    ) -> Result<()> {
        let mode = match args.trim() {
            "new" => WorktreeMode::New,
            "fork" => WorktreeMode::Fork,
            _ => {
                self.push_system("Usage: /worktree new (fresh conversation) or /worktree fork (preserve conversation)");
                return Ok(());
            }
        };
        if app_server.uses_remote_workspace()
            || self.resume_cwd_runtime.uses_remote_workspace_or_environment
        {
            return Err(eyre!(
                "Managed worktrees are only supported for local sessions"
            ));
        }
        if self.pending_worktree.is_some() {
            return Err(eyre!("A worktree is already being created"));
        }
        if self.block_session_switch_if_busy() {
            return Ok(());
        }
        self.check_worktree_idle()?;
        let loader = self
            .worktree_loader
            .clone()
            .ok_or_else(|| eyre!("Worktree configuration is unavailable for this session"))?;
        let client = app_server
            .app_server_request_handle()
            .ok_or_else(|| eyre!("Worktrees are unavailable for this connection"))?;
        let source = self.current_session_config(config)?;
        let thread_id = self.thread_id;
        let thread_ids = self.tracked_thread_ids();
        let cwd = PathBuf::from(&self.cwd);
        let source_cwd = cwd.clone();
        self.pending_worktree = Some(PendingWorktree {
            thread_id: self.thread_id,
            cwd,
            mode,
            task: WorktreeTask::Creating(tokio::spawn(async move {
                tokio::time::timeout(
                    Duration::from_secs(/*secs*/ 30),
                    check_worktree_source(&client, thread_id, &thread_ids),
                )
                .await??;
                loader.prepare(&source, source_cwd).await
            })),
        });
        self.push_system("Creating managed worktree");
        Ok(())
    }

    fn check_worktree_idle(&self) -> Result<()> {
        if self.session_unavailable_reason.is_some() || !self.can_accept_direct_input {
            return Err(eyre!(
                "The current session is unavailable or does not accept direct input"
            ));
        }
        if self.active_turn_id.is_some()
            || self.composer.has_queued_messages()
            || !self.composer.is_empty()
            || self.has_pending_queue_mutation()
            || self.has_pending_backend_action(ActionGroup::TurnStart)
            || self.has_pending_backend_action(ActionGroup::TurnSteer)
            || self.has_pending_backend_action(ActionGroup::QueueHydration)
            || self.has_pending_backend_action(ActionGroup::SessionSwitch)
            || self.has_pending_backend_action(ActionGroup::ConversationBranch)
            || self.pending_mcp_management.is_some()
            || self.pending_plugin_management.is_some()
            || self.side_parent.is_some()
            || self.pending_shell_command.is_some()
            || self.voice.has_work()
            || self.recap.has_work()
            || self.has_pending_backend_action(ActionGroup::Settings)
            || self.has_pending_backend_action(ActionGroup::Workspace)
            || self.pending_approval.is_some()
            || self.pending_elicitation.is_some()
            || self.pending_user_input.is_some()
        {
            return Err(eyre!(
                "Creating a worktree requires an idle session without queued input, active voice, or pending requests"
            ));
        }
        if self.agent_activity.ordered_agents().iter().any(|agent| {
            agent.thread_id != self.thread_id.to_string()
                && matches!(
                    agent.status,
                    AgentLifecycleStatus::Running | AgentLifecycleStatus::PendingInit
                )
        }) {
            return Err(eyre!("Wait for other agents before creating a worktree"));
        }
        Ok(())
    }

    pub(super) async fn poll_managed_worktree<S: AppShellBackend>(
        &mut self,
        config: &mut Config,
        app_server: &S,
    ) -> bool {
        if !self
            .pending_worktree
            .as_ref()
            .is_some_and(PendingWorktree::is_finished)
        {
            return false;
        }
        let pending = self
            .pending_worktree
            .take()
            .expect("completed worktree operation");
        let same_source = self.thread_id == pending.thread_id && self.cwd == pending.cwd;
        match pending.task {
            WorktreeTask::Creating(task) => {
                let mut prepared = match task.await {
                    Ok(Ok(prepared)) => prepared,
                    Ok(Err(error)) => {
                        self.push_error(error.to_string());
                        return true;
                    }
                    Err(error) => {
                        self.push_error(format!("Worktree creation failed: {error}"));
                        return true;
                    }
                };
                let ready = if same_source {
                    self.check_worktree_idle()
                } else {
                    Err(eyre!(
                        "The source session changed while creating the worktree"
                    ))
                };
                if let Err(error) = ready {
                    self.push_error(prepared.retained_error(error).to_string());
                    return true;
                }
                if !prepared.config.active_project.is_trusted() {
                    self.push_error(prepared.retained_error("The new worktree is not trusted; start Better Codex there to review its trust").to_string());
                    return true;
                }
                if pending.mode == WorktreeMode::Fork
                    && with_terminal_visualization_instructions(
                        config,
                        config.developer_instructions.clone(),
                    ) != with_terminal_visualization_instructions(
                        &prepared.config,
                        prepared.config.developer_instructions.clone(),
                    )
                {
                    self.push_error(prepared.retained_error("Developer instructions differ; start a new conversation instead of forking").to_string());
                    return true;
                }
                let Some(client) = app_server.app_server_request_handle() else {
                    self.push_error(
                        prepared
                            .retained_error("The app-server connection is unavailable")
                            .to_string(),
                    );
                    return true;
                };
                prepared.config.model = Some(self.model.clone());
                prepared.config.model_reasoning_effort = self.reasoning_effort.clone();
                prepared.config.service_tier = self.service_tier.clone();
                prepared.config.personality = self.personality;
                prepared.config.daybreak_enabled = self.daybreak_enabled;
                let destination = prepared.config.clone();
                let thread_ids = self.tracked_thread_ids();
                let thread_id = pending.thread_id;
                let mode = pending.mode;
                self.pending_worktree = Some(PendingWorktree {
                    thread_id,
                    cwd: pending.cwd,
                    mode,
                    task: WorktreeTask::Starting {
                        prepared: Box::new(prepared),
                        request: tokio::spawn(async move {
                            tokio::time::timeout(
                                Duration::from_secs(/*secs*/ 30),
                                check_worktree_source(&client, thread_id, &thread_ids),
                            )
                            .await??;
                            start_managed_worktree_thread(client, thread_id, destination, mode)
                                .await
                        }),
                    },
                });
            }
            WorktreeTask::Starting { prepared, request } => {
                let started = match request.await {
                    Ok(Ok(started)) => started,
                    Ok(Err(error)) => {
                        self.push_error(prepared.retained_error(error).to_string());
                        return true;
                    }
                    Err(error) => {
                        self.push_error(prepared.retained_error(error).to_string());
                        return true;
                    }
                };
                if let Err(error) = prepared.bind(started.session.thread_id) {
                    self.push_error(error.to_string());
                    return true;
                }
                if !same_source || self.check_worktree_idle().is_err() {
                    self.push_error(
                        prepared
                            .retained_error(format!(
                                "The source session changed; new session {} remains available",
                                started.session.thread_id
                            ))
                            .to_string(),
                    );
                    return true;
                }
                *config = prepared.config.clone();
                self.complete_session_switch(started, app_server).await;
                self.push_system(format!("Working in {}", prepared.checkout.cwd.display()));
            }
        }
        true
    }
}

#[cfg(test)]
#[path = "managed_worktree_tests.rs"]
mod tests;
