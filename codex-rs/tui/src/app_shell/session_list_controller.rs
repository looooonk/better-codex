use super::AgentActivityState;
use super::McpInventorySummary;
use super::PluginInventorySummary;
use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use super::is_unmodified_action_key;
use super::is_unmodified_key_event;
use super::is_unmodified_key_press;
use super::sessions::SessionSearchOutcome;
use crate::app_server_session::AppServerStartedThread;
use crate::legacy_core::config::Config;
use crate::text_input::text_input_action_from_key;
use crate::token_usage::TokenUsage;
use codex_protocol::ThreadId;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

impl ShellState {
    pub(super) async fn handle_session_list_key<S>(
        &mut self,
        key: KeyEvent,
        config: &Config,
        app_server: &mut S,
    ) -> Result<bool>
    where
        S: AppShellBackend,
    {
        if self.session_list.renaming() {
            return self
                .handle_session_rename_key(key, app_server)
                .map(|()| true);
        }
        if self.session_list.search_active() {
            if self.handle_session_search_key(key) == SessionSearchOutcome::RefreshList {
                self.start_session_list_refresh(app_server);
            }
            return Ok(true);
        }
        if !is_unmodified_action_key(key) {
            return Ok(matches!(
                key.code,
                KeyCode::Esc
                    | KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Enter
                    | KeyCode::Char('k' | 'j' | '/' | 'v' | 'r' | 'f' | 'a' | 'u' | 'd' | 'n')
                    | KeyCode::PageUp
                    | KeyCode::PageDown
            ));
        }
        match key.code {
            KeyCode::Esc => {
                self.session_list.focused = false;
                Ok(true)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.session_list.move_selection_up();
                Ok(true)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.session_list.move_selection_down() && self.session_list.has_more() {
                    self.start_session_list_next_page(app_server);
                }
                Ok(true)
            }
            KeyCode::Enter => {
                self.resume_selected_session(config, app_server);
                Ok(true)
            }
            KeyCode::Char('/') => {
                self.session_list.start_search();
                Ok(true)
            }
            KeyCode::Char('v') => {
                self.session_list.toggle_archived();
                self.start_session_list_refresh(app_server);
                Ok(true)
            }
            KeyCode::Char('r') => {
                self.resume_selected_session(config, app_server);
                Ok(true)
            }
            KeyCode::Char('f') => {
                self.fork_selected_session(config, app_server).await?;
                Ok(true)
            }
            KeyCode::Char('a') if !self.session_list.show_archived() => {
                self.archive_selected_session(app_server).await?;
                Ok(true)
            }
            KeyCode::Char('u') if self.session_list.show_archived() => {
                self.unarchive_selected_session(app_server).await?;
                Ok(true)
            }
            KeyCode::Char('d') => {
                self.start_session_delete_confirmation(app_server);
                Ok(true)
            }
            KeyCode::Char('n') if !self.session_list.show_archived() => {
                self.session_list.start_rename();
                Ok(true)
            }
            KeyCode::PageUp => {
                for _ in 0..5 {
                    self.session_list.move_selection_up();
                }
                Ok(true)
            }
            KeyCode::PageDown => {
                let mut reached_end = false;
                for _ in 0..5 {
                    reached_end |= !self.session_list.move_selection_down();
                }
                if reached_end && self.session_list.has_more() {
                    self.start_session_list_next_page(app_server);
                }
                Ok(true)
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_)
            | KeyCode::Tab
            | KeyCode::BackTab => Ok(false),
        }
    }

    fn handle_session_search_key(&mut self, key: KeyEvent) -> SessionSearchOutcome {
        if let Some(action) = text_input_action_from_key(key) {
            return self.session_list.edit_search(action);
        }
        if (matches!(key.code, KeyCode::Esc | KeyCode::Enter) && !is_unmodified_key_press(key))
            || (key.code == KeyCode::Backspace && !is_unmodified_key_event(key))
            || (matches!(key.code, KeyCode::Up | KeyCode::Down) && !is_unmodified_action_key(key))
        {
            return SessionSearchOutcome::LocalFilterOnly;
        }
        match key.code {
            KeyCode::Esc => {
                self.session_list.clear_search();
                SessionSearchOutcome::RefreshList
            }
            KeyCode::Enter => {
                self.session_list.stop_search();
                SessionSearchOutcome::RefreshList
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.session_list.push_search_char(ch);
                SessionSearchOutcome::LocalFilterOnly
            }
            KeyCode::Char(_) => SessionSearchOutcome::LocalFilterOnly,
            KeyCode::Up => {
                self.session_list.move_selection_up();
                SessionSearchOutcome::LocalFilterOnly
            }
            KeyCode::Down => {
                self.session_list.move_selection_down();
                SessionSearchOutcome::LocalFilterOnly
            }
            KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_)
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::PageUp
            | KeyCode::PageDown => SessionSearchOutcome::LocalFilterOnly,
        }
    }

    fn handle_session_rename_key<S>(&mut self, key: KeyEvent, app_server: &S) -> Result<()>
    where
        S: AppShellBackend,
    {
        if let Some(action) = text_input_action_from_key(key) {
            self.session_list.edit_rename(action);
            return Ok(());
        }
        if (matches!(key.code, KeyCode::Esc | KeyCode::Enter) && !is_unmodified_key_press(key))
            || (key.code == KeyCode::Backspace && !is_unmodified_key_event(key))
        {
            return Ok(());
        }
        match key.code {
            KeyCode::Esc => {
                self.session_list.cancel_rename();
            }
            KeyCode::Enter => {
                let Some(thread_id) = self.session_list.selected_thread_id() else {
                    self.session_list.cancel_rename();
                    return Ok(());
                };
                let Some(name) = self.session_list.rename_draft() else {
                    return Ok(());
                };
                if name.is_empty() {
                    self.push_error("session name cannot be empty");
                    return Ok(());
                }
                let request = app_server.thread_set_name_in_background(thread_id, name.clone());
                self.start_backend_action(
                    ActionGroup::SessionRename,
                    "renaming session",
                    async move {
                        BackendActionResult::SessionRename {
                            thread_id,
                            name,
                            result: request.await,
                        }
                    },
                );
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.session_list.push_rename_char(ch);
            }
            KeyCode::Char(_) => {}
            KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_)
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Up
            | KeyCode::Down => {}
        }
        Ok(())
    }

    pub(super) fn complete_session_rename(
        &mut self,
        thread_id: ThreadId,
        name: String,
        result: Result<()>,
    ) {
        match result {
            Ok(()) => {
                if self.session_list.rename_draft().as_deref() == Some(name.as_str()) {
                    self.session_list.cancel_rename();
                }
                self.invalidate_session_list_refresh();
                self.session_list.rename_thread(thread_id, name.clone());
                if thread_id == self.thread_id {
                    self.thread_name = Some(name.clone());
                }
                self.push_status(format!("renamed session {name}"));
            }
            Err(err) => self.report_action_error("failed to rename session", err),
        }
    }

    fn resume_selected_session<S>(&mut self, config: &Config, app_server: &S)
    where
        S: AppShellBackend,
    {
        let Some(thread_id) = self.session_list.selected_thread_id() else {
            self.push_status("no session selected");
            return;
        };
        self.resume_session(config, app_server, thread_id);
    }

    async fn fork_selected_session<S>(&mut self, config: &Config, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        if self.block_session_switch_if_busy() {
            return Ok(());
        }
        let Some(thread_id) = self.session_list.selected_thread_id() else {
            self.push_status("no session selected");
            return Ok(());
        };
        let session_cwd = self.session_list.cwd_for_thread(thread_id).cloned();
        let Some(session_config) = self.session_switch_config(
            config,
            session_cwd.as_deref(),
            app_server.uses_remote_workspace(),
        ) else {
            return Ok(());
        };
        self.finish_subscription_cleanup().await;
        let started = app_server.fork_thread(session_config, thread_id).await?;
        self.complete_session_switch(started, app_server).await;
        Ok(())
    }

    pub(super) fn block_session_switch_if_busy(&mut self) -> bool {
        let message = if self.has_pending_backend_action(ActionGroup::SessionSwitch) {
            "wait for the pending session switch to finish"
        } else if self.has_pending_backend_action(ActionGroup::TurnStart) {
            "wait for the turn submission to finish"
        } else if self.has_pending_backend_action(ActionGroup::Settings) {
            "wait for settings to finish saving"
        } else if self.active_turn_id.is_some() {
            "finish or interrupt the active turn before switching sessions"
        } else if self.pending_shell_command.is_some() {
            "finish or cancel the shell command before switching sessions"
        } else if self.pending_approval.is_some()
            || self.pending_elicitation.is_some()
            || self.pending_user_input.is_some()
        {
            "resolve the pending request before switching sessions"
        } else if self.composer.has_queued_messages() {
            "finish queued messages before switching sessions"
        } else if !self.composer.is_empty() {
            "send or clear the message draft before switching sessions"
        } else {
            return false;
        };
        self.push_status(message);
        true
    }

    async fn archive_selected_session<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(thread_id) = self.session_list.selected_thread_id() else {
            self.push_status("no session selected");
            return Ok(());
        };
        if self.session_list.selected_is_current(self.thread_id) {
            self.push_error("cannot archive the active session");
            return Ok(());
        }
        app_server.thread_archive(thread_id).await?;
        self.invalidate_session_list_refresh();
        let title = self
            .session_list
            .remove_selected()
            .map(|row| row.thread_id.to_string())
            .unwrap_or_else(|| thread_id.to_string());
        self.push_status(format!("archived session {title}"));
        Ok(())
    }

    async fn unarchive_selected_session<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(thread_id) = self.session_list.selected_thread_id() else {
            self.push_status("no session selected");
            return Ok(());
        };
        app_server.thread_unarchive(thread_id).await?;
        self.invalidate_session_list_refresh();
        self.session_list.remove_selected();
        self.push_status(format!("unarchived session {thread_id}"));
        Ok(())
    }

    pub(super) fn replace_started_session(&mut self, started: AppServerStartedThread) {
        self.invalidate_session_hydration();
        self.close_agent_log();
        self.close_tool_output();
        self.close_diff_view();
        let AppServerStartedThread {
            session,
            turns,
            agent_threads,
            agent_history_task,
        } = started;
        self.thread_id = session.thread_id;
        self.session_unavailable_reason = None;
        self.thread_name = session.thread_name;
        if !session.model.is_empty() {
            self.model = session.model;
        }
        self.cwd = session.cwd.to_string_lossy().to_string();
        self.approval_policy = session.approval_policy;
        self.approvals_reviewer = session.approvals_reviewer;
        self.permission_profile = session.permission_profile;
        self.active_permission_profile = session.active_permission_profile;
        self.runtime_workspace_roots = session.runtime_workspace_roots;
        self.reasoning_effort = session.reasoning_effort;
        self.service_tier = session.service_tier;
        self.collaboration_mode = session.collaboration_mode;
        self.personality = session.personality;
        self.transcript.clear();
        self.transcript_scroll = 0;
        self.transcript_scroll_max.set(0);
        self.dashboard_scroll.set(0);
        self.transcript_selection = None;
        self.transcript_render_cache.get_mut().clear();
        self.clear_streaming_transcript();
        self.plan_explanation = None;
        self.plan_steps.clear();
        self.record_active_goal(None);
        self.composer.reset_for_session();
        self.pending_shell_command = None;
        self.command_palette = None;
        self.exit_confirmation_pending = false;
        self.pending_external_agent_import = None;
        self.pending_mcp_management = None;
        self.pending_plugin_management = None;
        self.mcp_inventory = McpInventorySummary::default();
        self.mcp_catalog = None;
        self.plugin_inventory = PluginInventorySummary::default();
        self.plugin_catalog = None;
        self.tool_activity.clear();
        self.agent_activity = AgentActivityState::default();
        self.active_agent_thread_ids.clear();
        self.deferred_unsubscribe_thread_ids.clear();
        self.subagent_activity.clear();
        self.latest_diff = None;
        self.diff_store.clear();
        self.diff_store
            .set_display_root(std::path::Path::new(&self.cwd));
        self.reset_workspace_git_status();
        self.token_usage = TokenUsage::default();
        self.context_token_usage = TokenUsage::default();
        self.model_context_window = None;
        self.active_turn_id = None;
        self.clear_interactive_requests();
        self.pending_session_delete = None;
        self.selector = None;
        self.safety_buffering.clear();
        self.status = "ready".to_string();
        self.push_system("switched session");
        self.ingest_turn_history(turns);
        self.install_agent_history(agent_threads, agent_history_task);
    }
}
