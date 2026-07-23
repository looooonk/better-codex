use super::DashboardRouteStep;
use super::LocalSlashCommand;
use super::LocalSlashCommandOutcome;
use super::PendingElicitation;
use super::ShellState;
use super::TRANSCRIPT_PAGE_SCROLL_STEP;
use super::TRANSCRIPT_SELECTION_STEP;
use super::approval_action_from_key;
use super::backend::AppShellBackend;
use super::dashboard_route_from_key;
use super::dashboard_route_step_from_key;
use super::elicitation_action_from_key;
use super::is_composer_newline_key;
use super::navigation::DashboardRoute;
use super::shell_command::ShellCommand;
use crate::key_hint;
use crate::legacy_core::config::Config;
use crate::text_input::text_input_action_from_key;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;

#[derive(Clone, Copy)]
enum ActiveInputRoute {
    DiffView,
    ToolOutput,
    AgentLog,
    Selector,
    CommandPalette,
    AccountAuth,
    SafetyBuffering,
    SessionDelete,
    Approval,
    ElicitationChoice,
    ElicitationEditing,
    ExternalAgentImport,
    McpManagement,
    PluginManagement,
    UserInput,
}

impl ShellState {
    fn active_input_route(&self) -> Option<ActiveInputRoute> {
        if self.diff_view.is_some() {
            Some(ActiveInputRoute::DiffView)
        } else if self.tool_output.is_some() {
            Some(ActiveInputRoute::ToolOutput)
        } else if self.agent_log.is_some() {
            Some(ActiveInputRoute::AgentLog)
        } else if self.selector.is_some() {
            Some(ActiveInputRoute::Selector)
        } else if self.command_palette.is_some() {
            Some(ActiveInputRoute::CommandPalette)
        } else if self.pending_account_auth.is_some() {
            Some(ActiveInputRoute::AccountAuth)
        } else if self.safety_buffering_modal_lines().is_some() {
            Some(ActiveInputRoute::SafetyBuffering)
        } else if self.pending_session_delete.is_some() {
            Some(ActiveInputRoute::SessionDelete)
        } else if self.pending_approval.is_some() {
            Some(ActiveInputRoute::Approval)
        } else if self.pending_elicitation.is_some() {
            if self
                .pending_elicitation
                .as_ref()
                .is_some_and(PendingElicitation::editing)
            {
                Some(ActiveInputRoute::ElicitationEditing)
            } else {
                Some(ActiveInputRoute::ElicitationChoice)
            }
        } else if self.pending_external_agent_import.is_some() {
            Some(ActiveInputRoute::ExternalAgentImport)
        } else if self.pending_mcp_management.is_some() {
            Some(ActiveInputRoute::McpManagement)
        } else if self.pending_plugin_management.is_some() {
            Some(ActiveInputRoute::PluginManagement)
        } else if self.pending_user_input.is_some() {
            Some(ActiveInputRoute::UserInput)
        } else {
            None
        }
    }

    fn plain_text_repeat_enabled(&self) -> bool {
        match self.active_input_route() {
            Some(ActiveInputRoute::ElicitationEditing | ActiveInputRoute::UserInput) => true,
            Some(ActiveInputRoute::McpManagement) => self
                .pending_mcp_management
                .as_ref()
                .is_some_and(super::mcp_management::McpManagementState::editing),
            Some(ActiveInputRoute::AccountAuth) => self
                .pending_account_auth
                .as_ref()
                .is_some_and(super::account_auth::AccountAuthState::editing),
            Some(_) => false,
            None if self.dashboard_route == DashboardRoute::Sessions
                && self.session_list.focused =>
            {
                self.session_list.search_active() || self.session_list.renaming()
            }
            None if self.dashboard_route == DashboardRoute::Status && self.settings.focused => {
                self.settings.editing()
            }
            None => !self.dashboard_focused() && self.transcript_selection.is_none(),
        }
    }

    async fn handle_active_input_route<S>(
        &mut self,
        key: KeyEvent,
        config: &Config,
        app_server: &mut S,
    ) -> Result<Option<bool>>
    where
        S: AppShellBackend,
    {
        let Some(route) = self.active_input_route() else {
            return Ok(None);
        };
        match route {
            ActiveInputRoute::DiffView => {
                self.handle_diff_view_key(key);
            }
            ActiveInputRoute::ToolOutput => {
                self.handle_tool_output_key(key);
            }
            ActiveInputRoute::AgentLog => {
                if key.kind == KeyEventKind::Press
                    && key.modifiers == KeyModifiers::NONE
                    && matches!(key.code, KeyCode::Char('r'))
                {
                    self.reload_agent_log(config, app_server);
                } else {
                    self.handle_agent_log_key(key);
                }
            }
            ActiveInputRoute::Selector => self.handle_selector_key(key, app_server).await?,
            ActiveInputRoute::CommandPalette => {
                self.handle_command_palette_key(key, config, app_server)
                    .await?;
            }
            ActiveInputRoute::AccountAuth => {
                return self
                    .handle_account_auth_key(key, app_server)
                    .await
                    .map(Some);
            }
            ActiveInputRoute::SafetyBuffering => {
                self.handle_safety_buffering_key(key, config, app_server)
                    .await;
            }
            ActiveInputRoute::SessionDelete => {
                self.handle_session_delete_key(key, app_server).await?;
            }
            ActiveInputRoute::Approval => {
                if let Some(action) = self
                    .pending_approval
                    .as_ref()
                    .and_then(|pending| approval_action_from_key(pending, key))
                {
                    self.handle_pending_approval_action(app_server, action)
                        .await?;
                }
            }
            ActiveInputRoute::ElicitationChoice => {
                if let Some(action) = elicitation_action_from_key(key) {
                    self.handle_pending_elicitation_action(app_server, action)
                        .await?;
                }
            }
            ActiveInputRoute::ElicitationEditing | ActiveInputRoute::UserInput => {
                return self.handle_user_input_key(key, app_server).await.map(Some);
            }
            ActiveInputRoute::ExternalAgentImport => {
                self.handle_external_agent_import_key(key, app_server)
                    .await?;
            }
            ActiveInputRoute::McpManagement => {
                self.handle_mcp_management_key(key, app_server).await?;
            }
            ActiveInputRoute::PluginManagement => {
                self.handle_plugin_management_key(key, app_server).await?;
            }
        }
        Ok(Some(false))
    }

    pub(super) async fn handle_key<S>(
        &mut self,
        key: KeyEvent,
        config: &Config,
        app_server: &mut S,
    ) -> Result<bool>
    where
        S: AppShellBackend,
    {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(false);
        }
        let is_plain_text_repeat = if key.kind == KeyEventKind::Repeat
            && let KeyCode::Char(ch) = key.code
            && !ch.is_control()
            && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            self.plain_text_repeat_enabled()
        } else {
            false
        };
        if key.kind == KeyEventKind::Repeat
            && !is_plain_text_repeat
            && !matches!(
                key.code,
                KeyCode::Backspace
                    | KeyCode::Char('\u{007f}')
                    | KeyCode::Delete
                    | KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::PageUp
                    | KeyCode::PageDown
            )
        {
            return Ok(false);
        }
        if self.handle_text_copy_shortcut_with(key, crate::clipboard_copy::copy_to_clipboard) {
            self.exit_confirmation_pending = false;
            return Ok(false);
        }
        let is_ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c'));
        if is_ctrl_c {
            if self.active_turn_id.is_some() {
                self.exit_confirmation_pending = false;
                self.interrupt_active_turn(app_server).await?;
                return Ok(false);
            }
            if self.has_pending_shell_command() {
                self.cancel_shell_command();
                self.exit_confirmation_pending = false;
                return Ok(false);
            }
            return Ok(self.confirm_exit());
        }
        if matches!(key.code, KeyCode::Esc) && self.has_text_selection() {
            self.exit_confirmation_pending = false;
            self.clear_text_selections();
            return Ok(false);
        }
        self.clear_transcript_text_selection();
        if !matches!(key.code, KeyCode::Esc) {
            self.exit_confirmation_pending = false;
        }
        if let Some(exit) = self
            .handle_active_input_route(key, config, app_server)
            .await?
        {
            return Ok(exit);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('d')) {
            self.toggle_dashboard();
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('o')) {
            self.copy_selected_transcript_with(crate::clipboard_copy::copy_to_clipboard);
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('n')) {
            self.start_new_session(config, app_server).await?;
            return Ok(false);
        }
        if let Some(route) = dashboard_route_from_key(key) {
            let route_already_visible = self.dashboard_visible && self.dashboard_route == route;
            self.dashboard_visible = true;
            self.set_dashboard_route(route);
            self.session_list.focused = route_already_visible && route == DashboardRoute::Sessions;
            self.settings.focused = route_already_visible && route == DashboardRoute::Status;
            self.agents_focused = route_already_visible && route == DashboardRoute::Agents;
            if self.dashboard_focused() {
                self.dashboard_scroll.set(0);
            }
            if route == DashboardRoute::Sessions {
                self.start_session_list_refresh(app_server);
            }
            return Ok(false);
        }
        let dashboard_text_input_focused = match self.dashboard_route {
            DashboardRoute::Sessions => {
                self.session_list.focused
                    && (self.session_list.search_active() || self.session_list.renaming())
            }
            DashboardRoute::Status => self.settings.focused && self.settings.editing(),
            DashboardRoute::Agents | DashboardRoute::Help => false,
        };
        if self.composer.is_empty()
            && (self.dashboard_focused() && !dashboard_text_input_focused
                || text_input_action_from_key(key).is_none())
            && let Some(step) =
                dashboard_route_step_from_key(key, /*allow_word_motion_fallback*/ true)
        {
            let route = match step {
                DashboardRouteStep::Previous => self.dashboard_route.previous(),
                DashboardRouteStep::Next => self.dashboard_route.next(),
            };
            self.set_dashboard_route(route);
            self.session_list.focused = false;
            self.settings.focused = false;
            self.agents_focused = false;
            return Ok(false);
        }
        let edit_previous_queued = key_hint::alt(KeyCode::Up).is_press(key);
        let edit_next_queued = key_hint::alt(KeyCode::Down).is_press(key);
        if self.composer.has_queued_messages() && (edit_previous_queued || edit_next_queued) {
            self.session_list.focused = false;
            self.settings.focused = false;
            self.agents_focused = false;
            self.clear_transcript_selection();
            if edit_previous_queued {
                self.composer.edit_previous_queued_message();
            } else {
                self.composer.edit_next_queued_message();
            }
            return Ok(false);
        }
        if self.transcript_selection.is_some()
            && let Some(handled) = self.handle_transcript_selection_key(key)
        {
            return Ok(handled);
        }
        if key.modifiers.contains(KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Up | KeyCode::Down)
        {
            self.select_latest_transcript_item();
            if matches!(key.code, KeyCode::Up) {
                self.move_transcript_selection_up(TRANSCRIPT_SELECTION_STEP);
            }
            return Ok(false);
        }
        if self.dashboard_route == DashboardRoute::Sessions
            && self.session_list.focused
            && self
                .handle_session_list_key(key, config, app_server)
                .await?
        {
            return Ok(false);
        }
        if self.dashboard_route == DashboardRoute::Status
            && self.settings.focused
            && self.handle_settings_key(key, app_server).await?
        {
            return Ok(false);
        }
        if self.dashboard_visible
            && self.dashboard_route == DashboardRoute::Agents
            && self.agents_focused
            && matches!(key.code, KeyCode::Enter)
            && matches!(key.modifiers, KeyModifiers::NONE)
        {
            self.open_selected_agent_log(config, app_server);
            return Ok(false);
        }
        if self.handle_agent_activity_key(key) {
            return Ok(false);
        }
        if self.dashboard_focused()
            && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            return Ok(false);
        }
        if !self.dashboard_focused()
            && let Some(action) = text_input_action_from_key(key)
        {
            self.composer.apply_text_input_action(action);
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('p')) {
            self.open_command_palette();
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Char('m')) {
            self.open_model_selector();
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Char('e')) {
            self.open_reasoning_selector();
            return Ok(false);
        }
        match key.code {
            KeyCode::Esc => Ok(self.confirm_exit()),
            KeyCode::Enter => {
                if is_composer_newline_key(key) {
                    let result = self.composer.insert_newline();
                    self.report_composer_insert(result);
                    return Ok(false);
                }
                if self.reject_oversized_composer() {
                    return Ok(false);
                }
                let finished_queued_edit = self.composer.finish_queued_message_edit();
                if finished_queued_edit {
                    if self.active_turn_id.is_none() && self.composer.has_queued_messages() {
                        self.submit_next_queued_message(app_server);
                    }
                    return Ok(false);
                }
                let prompt = self.composer.submission_text();
                let prompt_is_empty = prompt.trim().is_empty();
                if self.active_turn_id.is_none()
                    && self.composer.has_queued_messages()
                    && prompt_is_empty
                {
                    self.submit_next_queued_message(app_server);
                    return Ok(false);
                }
                if prompt_is_empty && self.dashboard_visible {
                    match self.dashboard_route {
                        DashboardRoute::Sessions => self.session_list.focused = true,
                        DashboardRoute::Agents => self.agents_focused = true,
                        DashboardRoute::Status => self.settings.focused = true,
                        DashboardRoute::Help => {}
                    }
                    if self.dashboard_focused() {
                        self.dashboard_scroll.set(0);
                        return Ok(false);
                    }
                }
                if !prompt_is_empty {
                    if let Some(command) = LocalSlashCommand::parse(&prompt) {
                        let outcome = self
                            .run_local_slash_command(command, prompt, config, app_server)
                            .await?;
                        return Ok(outcome == LocalSlashCommandOutcome::Exit);
                    } else if let Some(command) = ShellCommand::parse(&prompt) {
                        self.start_shell_command(command, prompt);
                    } else if self.active_turn_id.is_some() {
                        self.steer_active_turn(app_server, prompt).await?;
                    } else {
                        self.submit_prompt(app_server, prompt);
                    }
                }
                Ok(false)
            }
            KeyCode::Up => {
                self.composer.move_up_or_recall_history();
                Ok(false)
            }
            KeyCode::Down => {
                self.composer.move_down_or_recall_history();
                Ok(false)
            }
            KeyCode::PageUp => {
                self.scroll_transcript_up(TRANSCRIPT_PAGE_SCROLL_STEP);
                Ok(false)
            }
            KeyCode::PageDown => {
                self.scroll_transcript_down(TRANSCRIPT_PAGE_SCROLL_STEP);
                Ok(false)
            }
            KeyCode::Home => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_transcript_to_top();
                }
                Ok(false)
            }
            KeyCode::End => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_transcript_to_bottom();
                }
                Ok(false)
            }
            KeyCode::Char(ch) => {
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    let result = self.composer.insert_char(ch);
                    self.report_composer_insert(result);
                }
                Ok(false)
            }
            KeyCode::Tab => {
                if self.active_turn_id.is_some() && key_hint::plain(KeyCode::Tab).is_press(key) {
                    self.composer.queue_current_message();
                } else {
                    let result = self.composer.insert_str("    ");
                    self.report_composer_insert(result);
                }
                Ok(false)
            }
            KeyCode::BackTab => {
                let result = self.composer.insert_str("    ");
                self.report_composer_insert(result);
                Ok(false)
            }
            KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
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
            | KeyCode::Modifier(_) => Ok(false),
        }
    }
}
