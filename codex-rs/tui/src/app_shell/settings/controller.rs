use super::super::ShellState;
use super::super::backend::AppShellBackend;
use super::super::is_unmodified_action_key;
use super::super::is_unmodified_key_event;
use super::super::is_unmodified_key_press;
use super::super::reasoning_aura::ReasoningAuraTone;
use super::SettingsAction;
use super::SettingsView;
use super::background::SettingsChange;
use crate::config_update::build_model_selection_edits;
use crate::config_update::build_service_tier_selection_edits;
use crate::config_update::build_syntax_theme_edit;
use crate::config_update::clear_config_value;
use crate::config_update::replace_config_value;
use crate::render::highlight::validate_theme_name;
use crate::text_input::text_input_action_from_key;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
use codex_protocol::openai_models::ReasoningEffort;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;

impl ShellState {
    pub(in crate::app_shell) fn settings_view(&self) -> SettingsView {
        SettingsView {
            model: self.model.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            service_tier: self.service_tier.clone(),
            approval_policy: self.approval_policy,
            app_theme: self.app_theme,
            theme: self.tui_theme.clone(),
            animations: self.animations,
            show_tooltips: self.show_tooltips,
            mcp_inventory: self.mcp_inventory.clone(),
            plugin_inventory: self.plugin_inventory.clone(),
        }
    }

    pub(in crate::app_shell) async fn handle_settings_key<S>(
        &mut self,
        key: KeyEvent,
        app_server: &mut S,
    ) -> Result<bool>
    where
        S: AppShellBackend,
    {
        if self.settings.editing() {
            return self.handle_settings_edit_key(key, app_server).await;
        }
        let is_shift_back_tab = key.kind == KeyEventKind::Press
            && key.code == KeyCode::BackTab
            && key.modifiers == KeyModifiers::SHIFT;
        if !is_unmodified_action_key(key) && !is_shift_back_tab {
            return Ok(matches!(
                key.code,
                KeyCode::Esc
                    | KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Tab
                    | KeyCode::Right
                    | KeyCode::BackTab
                    | KeyCode::Left
                    | KeyCode::Enter
                    | KeyCode::Char('k' | 'j' | 'l' | 'h' | ' ')
            ));
        }
        match key.code {
            KeyCode::Esc => {
                self.settings.focused = false;
                Ok(true)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.settings.move_up();
                Ok(true)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.settings.move_down();
                Ok(true)
            }
            KeyCode::Tab | KeyCode::Right => {
                self.settings.next_page();
                Ok(true)
            }
            KeyCode::Char('l') => {
                self.settings.next_page();
                Ok(true)
            }
            KeyCode::BackTab | KeyCode::Left => {
                self.settings.previous_page();
                Ok(true)
            }
            KeyCode::Char('h') => {
                self.settings.previous_page();
                Ok(true)
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.activate_selected_setting(app_server).await?;
                Ok(true)
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
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
            | KeyCode::PageUp
            | KeyCode::PageDown => Ok(false),
        }
    }

    async fn handle_settings_edit_key<S>(
        &mut self,
        key: KeyEvent,
        app_server: &mut S,
    ) -> Result<bool>
    where
        S: AppShellBackend,
    {
        if let Some(action) = text_input_action_from_key(key) {
            self.settings.edit_text(action);
            return Ok(true);
        }
        if (matches!(key.code, KeyCode::Esc | KeyCode::Enter) && !is_unmodified_key_press(key))
            || (key.code == KeyCode::Backspace && !is_unmodified_key_event(key))
        {
            return Ok(true);
        }
        match key.code {
            KeyCode::Esc => {
                self.settings.cancel_edit();
            }
            KeyCode::Enter => {
                if let Some((action, draft)) = self.settings.edit_value() {
                    self.apply_settings_edit(action, draft, app_server)?;
                }
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.settings.push_edit_char(ch);
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Up
            | KeyCode::Down
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
            | KeyCode::PageDown => {}
        }
        Ok(true)
    }

    pub(in crate::app_shell) async fn activate_selected_setting<S>(
        &mut self,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let action = self.settings.selected_action();
        match action {
            SettingsAction::Model => self.open_model_selector(),
            SettingsAction::ServiceTier => self.open_service_tier_selector(),
            SettingsAction::AppTheme => self.open_app_theme_selector(),
            SettingsAction::Theme => {
                self.settings
                    .start_edit(action, self.tui_theme.clone().unwrap_or_default());
            }
            SettingsAction::ReasoningEffort => self.open_reasoning_selector(),
            SettingsAction::ApprovalPolicy => self.open_approval_selector(),
            SettingsAction::Animations => {
                let animations = !self.animations;
                self.schedule_settings_update(
                    app_server,
                    SettingsChange::Animations(animations),
                    vec![replace_config_value(
                        "tui.animations",
                        serde_json::json!(animations),
                    )],
                    None,
                );
            }
            SettingsAction::Tooltips => {
                let show_tooltips = !self.show_tooltips;
                self.schedule_settings_update(
                    app_server,
                    SettingsChange::Tooltips(show_tooltips),
                    vec![replace_config_value(
                        "tui.show_tooltips",
                        serde_json::json!(show_tooltips),
                    )],
                    None,
                );
            }
            SettingsAction::McpServers => {
                if self.mcp_catalog.is_some() {
                    self.open_mcp_management();
                } else {
                    self.refresh_mcp_inventory(app_server).await;
                }
            }
            SettingsAction::Plugins => {
                if self.plugin_catalog.is_some() {
                    self.open_plugin_management();
                } else {
                    self.refresh_plugin_inventory(app_server).await;
                }
            }
        }
        Ok(())
    }

    fn apply_settings_edit<S>(
        &mut self,
        action: SettingsAction,
        draft: String,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        match action {
            SettingsAction::Model => {
                if draft.is_empty() {
                    self.settings.set_error("model cannot be empty");
                    return Ok(());
                }
                if draft.chars().any(char::is_whitespace) {
                    self.settings.set_error("model cannot contain whitespace");
                    return Ok(());
                }
                self.apply_model(draft, app_server);
            }
            SettingsAction::ServiceTier => {
                if draft.chars().any(char::is_whitespace) {
                    self.settings
                        .set_error("service tier cannot contain whitespace");
                    return Ok(());
                }
                let service_tier = (!draft.is_empty()).then_some(draft);
                self.apply_service_tier(service_tier, app_server);
            }
            SettingsAction::Theme => {
                let theme = (!draft.is_empty()).then_some(draft);
                if let Some(warning) =
                    validate_theme_name(theme.as_deref(), Some(self.codex_home.as_path()))
                {
                    self.settings.set_error(warning);
                    return Ok(());
                }
                let edit = match theme.as_deref() {
                    Some(theme) => build_syntax_theme_edit(theme),
                    None => clear_config_value("tui.theme"),
                };
                self.schedule_settings_update(
                    app_server,
                    SettingsChange::Theme(theme),
                    vec![edit],
                    None,
                );
            }
            SettingsAction::ReasoningEffort
            | SettingsAction::ApprovalPolicy
            | SettingsAction::AppTheme
            | SettingsAction::Animations
            | SettingsAction::Tooltips
            | SettingsAction::McpServers
            | SettingsAction::Plugins => {}
        }
        Ok(())
    }

    pub(in crate::app_shell) fn apply_model<S>(&mut self, model: String, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        let current_thread_effort = self
            .reasoning_effort
            .clone()
            .or_else(|| self.default_reasoning_effort(&self.model));
        let preset = self
            .available_models
            .iter()
            .find(|preset| preset.model == model);
        let effort = preset
            .map(|preset| preset.default_reasoning_effort.clone())
            .or_else(|| self.reasoning_effort.clone());
        let service_tier = match (preset, self.service_tier.as_deref()) {
            (Some(_), Some(SERVICE_TIER_DEFAULT_REQUEST_VALUE)) | (_, None) => {
                self.service_tier.clone()
            }
            (Some(preset), Some(service_tier))
                if preset
                    .service_tiers
                    .iter()
                    .any(|tier| tier.id == service_tier) =>
            {
                self.service_tier.clone()
            }
            (Some(_), Some(_)) => None,
            (None, Some(_)) => self.service_tier.clone(),
        };
        let service_tier_update = (service_tier != self.service_tier).then(|| service_tier.clone());
        let mut edits = build_model_selection_edits(&model, effort.as_ref());
        if service_tier_update.is_some() {
            edits.extend(build_service_tier_selection_edits(service_tier.as_deref()));
        }
        self.schedule_settings_update(
            app_server,
            SettingsChange::Model {
                model: model.clone(),
                effort: effort.clone(),
                aura_tone: ReasoningAuraTone::for_transition(
                    current_thread_effort.as_ref(),
                    effort.as_ref(),
                ),
                service_tier,
            },
            edits,
            Some(self.thread_settings_update_params(
                Some(model.clone()),
                effort,
                service_tier_update,
            )),
        );
    }

    pub(in crate::app_shell) fn apply_service_tier<S>(
        &mut self,
        service_tier: Option<String>,
        app_server: &mut S,
    ) where
        S: AppShellBackend,
    {
        self.schedule_settings_update(
            app_server,
            SettingsChange::ServiceTier(service_tier.clone()),
            build_service_tier_selection_edits(service_tier.as_deref()),
            Some(self.thread_settings_update_params(
                /*model*/ None,
                /*effort*/ None,
                Some(service_tier),
            )),
        );
    }

    pub(in crate::app_shell) fn apply_app_theme<S>(
        &mut self,
        app_theme: codex_config::types::TuiAppTheme,
        app_server: &mut S,
    ) where
        S: AppShellBackend,
    {
        let persist =
            app_server.persist_app_theme_in_background(self.client_config_path.clone(), app_theme);
        self.start_settings_update(SettingsChange::AppTheme(app_theme), persist);
    }

    pub(in crate::app_shell) fn apply_reasoning_effort<S>(
        &mut self,
        effort: Option<ReasoningEffort>,
        app_server: &mut S,
    ) where
        S: AppShellBackend,
    {
        let current_thread_effort = self
            .reasoning_effort
            .clone()
            .or_else(|| self.default_reasoning_effort(&self.model));
        let thread_effort = effort
            .clone()
            .or_else(|| self.default_reasoning_effort(&self.model));
        self.schedule_settings_update(
            app_server,
            SettingsChange::ReasoningEffort {
                effort: effort.clone(),
                aura_tone: ReasoningAuraTone::for_transition(
                    current_thread_effort.as_ref(),
                    thread_effort.as_ref(),
                ),
                thread_effort: thread_effort.clone(),
            },
            build_model_selection_edits(&self.model, effort.as_ref()),
            Some(self.thread_settings_update_params(
                /*model*/ None,
                thread_effort,
                /*service_tier*/ None,
            )),
        );
    }

    fn default_reasoning_effort(&self, model: &str) -> Option<ReasoningEffort> {
        self.available_models
            .iter()
            .find(|preset| preset.model == model)
            .map(|preset| preset.default_reasoning_effort.clone())
    }

    pub(in crate::app_shell) fn apply_approval_policy<S>(
        &mut self,
        policy: AskForApproval,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let mut params = self.thread_settings_update_params(
            /*model*/ None, /*effort*/ None, /*service_tier*/ None,
        );
        params.approval_policy = Some(policy);
        self.schedule_settings_update(
            app_server,
            SettingsChange::ApprovalPolicy(policy),
            vec![replace_config_value(
                "approval_policy",
                serde_json::to_value(policy)?,
            )],
            Some(params),
        );
        Ok(())
    }

    fn thread_settings_update_params(
        &self,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
        service_tier: Option<Option<String>>,
    ) -> ThreadSettingsUpdateParams {
        ThreadSettingsUpdateParams {
            thread_id: self.thread_id.to_string(),
            approval_policy: Some(self.approval_policy),
            model,
            service_tier,
            effort,
            ..Default::default()
        }
    }
}
