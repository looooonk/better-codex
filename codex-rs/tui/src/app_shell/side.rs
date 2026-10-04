use super::ShellClientConfig;
use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use crate::app_server_session::AppServerStartedThread;
use crate::legacy_core::config::Config;
use codex_protocol::ThreadId;
use color_eyre::Result;

impl ShellState {
    pub(super) fn header_status(&self) -> std::borrow::Cow<'_, str> {
        let status = self.voice.status_label().unwrap_or(&self.status);
        match (self.side_parent.is_some(), self.daybreak_enabled) {
            (true, _) => format!("side | {status}").into(),
            (false, true) => format!("Daybreak | {status}").into(),
            (false, false) => status.into(),
        }
    }

    pub(super) async fn run_side_command<S: AppShellBackend>(
        &mut self,
        args: &str,
        config: &Config,
        app_server: &mut S,
    ) -> Result<()> {
        if self.side_parent.is_some() {
            if args.is_empty() || args == "return" {
                self.return_from_side(app_server).await?;
            } else {
                self.push_status(
                    "A side conversation is already open. Use /side return to go back.",
                );
            }
            return Ok(());
        }
        if args == "return" {
            self.push_status("You are already in the main conversation");
            return Ok(());
        }
        if self.reject_direct_input() || self.reject_unavailable_session_action() {
            return Ok(());
        }
        if self.voice.has_work()
            || self.has_pending_backend_actions()
            || self.pending_shell_command.is_some()
        {
            self.push_status(
                "Wait for pending actions or stop voice before opening a side conversation",
            );
            return Ok(());
        }
        let config = self.current_session_config(config)?;
        let parent_id = self.thread_id;
        let question = args.trim().to_string();
        let fork = app_server.fork_side_thread_in_background(config, parent_id);
        self.start_backend_action(
            ActionGroup::ConversationBranch,
            "opening side conversation",
            async move {
                BackendActionResult::SideFork {
                    parent_id,
                    question,
                    result: fork.await,
                }
            },
        );
        Ok(())
    }

    pub(super) fn complete_side_fork<S: AppShellBackend>(
        &mut self,
        app_server: &S,
        parent_id: ThreadId,
        question: String,
        result: Result<AppServerStartedThread>,
    ) {
        let started = match result {
            Ok(started) => started,
            Err(error) => {
                self.report_action_error("failed to open side conversation", error);
                return;
            }
        };
        if self.thread_id != parent_id || self.side_parent.is_some() {
            self.start_subscription_cleanup(app_server, vec![started.session.thread_id]);
            return;
        }
        let mut child = ShellState::new(
            started.session,
            self.model.clone(),
            self.available_models.clone(),
            ShellClientConfig {
                codex_home: self.codex_home.clone(),
                config_path: self.client_config_path.clone(),
                app_theme: self.app_theme,
                tui_theme: self.tui_theme.clone(),
                animations: self.animations,
                show_tooltips: self.show_tooltips,
            },
            self.resume_cwd_runtime.clone(),
            self.max_concurrent_threads_per_session,
        );
        child.voice = self.voice.for_new_session();
        child.keybindings = self.keybindings.clone();
        child.pets = self.pets.clone();
        child.status_surfaces = self.status_surfaces.clone();
        child.automatic_recap.enabled = self.automatic_recap.enabled;
        child.workspace_command_runner = self.workspace_command_runner.clone();
        child.dashboard_visible = false;
        child.composer.clear();
        child.transcript.clear();
        child.push_system("Side conversation. The main conversation keeps running. Use /side return or Ctrl+C to go back.");
        child.status = "side conversation".to_string();
        child.terminal_clear_requested.set(true);
        let parent = std::mem::replace(self, child);
        self.side_parent = Some(Box::new(parent));
        self.start_replaced_session_hydration(app_server);
        if !question.is_empty() {
            self.submit_prompt(app_server, question);
        }
    }

    pub(super) async fn return_from_side<S: AppShellBackend>(
        &mut self,
        app_server: &mut S,
    ) -> Result<()> {
        if self.side_parent.is_none() {
            return Ok(());
        }
        if self.has_pending_backend_action(ActionGroup::TurnStart)
            || self.has_pending_backend_action(ActionGroup::TurnSteer)
        {
            self.push_status(
                "Wait for the side message to start before returning to the main conversation",
            );
            return Ok(());
        }
        if self.has_pending_backend_action(ActionGroup::Workspace)
            || self.has_pending_backend_action(ActionGroup::Settings)
        {
            self.push_status("Wait for the side workspace action to finish before returning to the main conversation");
            return Ok(());
        }
        if self.pending_shell_command.is_some() {
            self.push_status(
                "Finish or cancel the side shell command before returning to the main conversation",
            );
            return Ok(());
        }
        if self.active_turn_id.is_some() {
            self.interrupt_active_turn(app_server).await?;
        }
        self.cancel_agent_history().await;
        let thread_id = self.thread_id;
        let draft = self.composer.clone_without_queue();
        let image_draft = draft
            .has_images()
            .then(|| (draft.submission_text(), draft.submission_items("")));
        let status_surfaces = self.status_surfaces.clone();
        let parent = self.side_parent.take().expect("side parent exists");
        *self = *parent;
        self.status_surfaces = status_surfaces;
        if let Some((prompt, images)) = image_draft {
            self.composer.restore_failed_submission(&prompt);
            self.composer.restore_input_images(&images);
        }
        self.terminal_clear_requested.set(true);
        self.push_status("returned to main conversation");
        app_server.unsubscribe_thread(thread_id).await?;
        Ok(())
    }
}
