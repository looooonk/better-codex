use super::super::ShellState;
use super::super::backend::AppShellBackend;
use super::SettingsAction;
use super::SettingsView;
use super::approval_policy_label;
use super::reasoning_effort_label;
use super::ultra_reasoning_concurrency_warning;
use crate::config_update::build_model_selection_edits;
use crate::config_update::build_service_tier_selection_edits;
use crate::config_update::build_syntax_theme_edit;
use crate::config_update::clear_config_value;
use crate::config_update::replace_config_value;
use crate::render::highlight::validate_theme_name;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
use codex_protocol::openai_models::ReasoningEffort;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

impl ShellState {
    pub(in crate::app_shell) fn settings_view(&self) -> SettingsView {
        SettingsView {
            model: self.model.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            service_tier: self.service_tier.clone(),
            approval_policy: self.approval_policy,
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
        match key.code {
            KeyCode::Esc => {
                self.settings.cancel_edit();
            }
            KeyCode::Enter => {
                if let Some((action, draft)) = self.settings.take_edit() {
                    self.apply_settings_edit(action, draft, app_server).await?;
                }
            }
            KeyCode::Backspace => {
                self.settings.backspace_edit();
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.settings.push_edit_char(ch);
            }
            KeyCode::Char(_)
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
            SettingsAction::Theme => {
                self.settings
                    .start_edit(action, self.tui_theme.clone().unwrap_or_default());
            }
            SettingsAction::ReasoningEffort => self.open_reasoning_selector(),
            SettingsAction::ApprovalPolicy => self.open_approval_selector(),
            SettingsAction::Animations => {
                self.animations = !self.animations;
                app_server
                    .write_config(vec![replace_config_value(
                        "tui.animations",
                        serde_json::json!(self.animations),
                    )])
                    .await?;
                self.settings.set_info(format!(
                    "animations {}",
                    if self.animations {
                        "enabled"
                    } else {
                        "disabled"
                    }
                ));
            }
            SettingsAction::Tooltips => {
                self.show_tooltips = !self.show_tooltips;
                app_server
                    .write_config(vec![replace_config_value(
                        "tui.show_tooltips",
                        serde_json::json!(self.show_tooltips),
                    )])
                    .await?;
                self.settings.set_info(format!(
                    "startup tooltips {}",
                    if self.show_tooltips {
                        "enabled"
                    } else {
                        "disabled"
                    }
                ));
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

    async fn apply_settings_edit<S>(
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
                self.apply_model(draft, app_server).await?;
            }
            SettingsAction::ServiceTier => {
                if draft.chars().any(char::is_whitespace) {
                    self.settings
                        .set_error("service tier cannot contain whitespace");
                    return Ok(());
                }
                let service_tier = (!draft.is_empty()).then_some(draft.clone());
                self.apply_service_tier(service_tier, app_server).await?;
            }
            SettingsAction::Theme => {
                let theme = (!draft.is_empty()).then_some(draft.clone());
                if let Some(warning) =
                    validate_theme_name(theme.as_deref(), Some(self.codex_home.as_path()))
                {
                    self.settings.set_error(warning);
                    return Ok(());
                }
                self.tui_theme = theme.clone();
                let edit = match theme.as_deref() {
                    Some(theme) => build_syntax_theme_edit(theme),
                    None => clear_config_value("tui.theme"),
                };
                app_server.write_config(vec![edit]).await?;
                let label = theme.as_deref().unwrap_or("default");
                self.settings.set_info(format!("theme set to {label}"));
            }
            SettingsAction::ReasoningEffort
            | SettingsAction::ApprovalPolicy
            | SettingsAction::Animations
            | SettingsAction::Tooltips
            | SettingsAction::McpServers
            | SettingsAction::Plugins => {}
        }
        Ok(())
    }

    pub(in crate::app_shell) async fn apply_model<S>(
        &mut self,
        model: String,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
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
        app_server.write_config(edits).await?;
        app_server
            .thread_settings_update(self.thread_settings_update_params(
                Some(model.clone()),
                effort.clone(),
                service_tier_update,
            ))
            .await?;
        if let Some(collaboration_mode) = self.collaboration_mode.as_mut() {
            **collaboration_mode = collaboration_mode.with_updates(
                Some(model.clone()),
                Some(effort.clone()),
                /*developer_instructions*/ None,
            );
        }
        self.model = model.clone();
        self.reasoning_effort = effort;
        self.service_tier = service_tier;
        self.settings.set_info(format!("model set to {model}"));
        Ok(())
    }

    pub(in crate::app_shell) async fn apply_service_tier<S>(
        &mut self,
        service_tier: Option<String>,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        self.service_tier = service_tier.clone();
        app_server
            .write_config(build_service_tier_selection_edits(service_tier.as_deref()))
            .await?;
        app_server
            .thread_settings_update(self.thread_settings_update_params(
                /*model*/ None,
                /*effort*/ None,
                Some(service_tier.clone()),
            ))
            .await?;
        let label = service_tier.as_deref().unwrap_or("default");
        self.settings
            .set_info(format!("service tier set to {label}"));
        Ok(())
    }

    pub(in crate::app_shell) async fn apply_reasoning_effort<S>(
        &mut self,
        effort: Option<ReasoningEffort>,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let thread_effort = effort.clone().or_else(|| {
            self.available_models
                .iter()
                .find(|preset| preset.model == self.model)
                .map(|preset| preset.default_reasoning_effort.clone())
        });
        self.reasoning_effort = effort.clone();
        app_server
            .write_config(build_model_selection_edits(&self.model, effort.as_ref()))
            .await?;
        app_server
            .thread_settings_update(self.thread_settings_update_params(
                /*model*/ None,
                thread_effort.clone(),
                /*service_tier*/ None,
            ))
            .await?;
        if let Some(collaboration_mode) = self.collaboration_mode.as_mut() {
            **collaboration_mode = collaboration_mode.with_updates(
                /*model*/ None,
                Some(effort.clone()),
                /*developer_instructions*/ None,
            );
        }
        if let Some(warning) = ultra_reasoning_concurrency_warning(
            thread_effort.as_ref(),
            self.max_concurrent_threads_per_session,
        ) {
            self.push_status(warning);
        }
        let label = effort
            .as_ref()
            .map(reasoning_effort_label)
            .unwrap_or_else(|| "default".to_string());
        self.settings.set_info(format!("reasoning set to {label}"));
        Ok(())
    }

    pub(in crate::app_shell) async fn apply_approval_policy<S>(
        &mut self,
        policy: AskForApproval,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        self.approval_policy = policy;
        app_server
            .write_config(vec![replace_config_value(
                "approval_policy",
                serde_json::to_value(policy)?,
            )])
            .await?;
        app_server
            .thread_settings_update(self.thread_settings_update_params(
                /*model*/ None, /*effort*/ None, /*service_tier*/ None,
            ))
            .await?;
        self.settings.set_info(format!(
            "approval policy set to {}",
            approval_policy_label(policy)
        ));
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
