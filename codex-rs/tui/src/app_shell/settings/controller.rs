use super::super::ShellState;
use super::super::backend::AppShellBackend;
use super::SettingsAction;
use super::SettingsView;
use super::approval_policy_label;
use super::next_approval_policy;
use super::next_reasoning_effort;
use crate::config_update::build_model_selection_edits;
use crate::config_update::build_service_tier_selection_edits;
use crate::config_update::build_syntax_theme_edit;
use crate::config_update::clear_config_value;
use crate::config_update::replace_config_value;
use crate::render::highlight::validate_theme_name;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
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
            KeyCode::Up => {
                self.settings.move_up();
                Ok(true)
            }
            KeyCode::Down => {
                self.settings.move_down();
                Ok(true)
            }
            KeyCode::Tab | KeyCode::Right => {
                self.settings.next_page();
                Ok(true)
            }
            KeyCode::BackTab | KeyCode::Left => {
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

    async fn activate_selected_setting<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let action = self.settings.selected_action();
        match action {
            SettingsAction::Model => {
                self.settings.start_edit(action, self.model.clone());
            }
            SettingsAction::ServiceTier => {
                self.settings
                    .start_edit(action, self.service_tier.clone().unwrap_or_default());
            }
            SettingsAction::Theme => {
                self.settings
                    .start_edit(action, self.tui_theme.clone().unwrap_or_default());
            }
            SettingsAction::ReasoningEffort => {
                let effort = next_reasoning_effort(self.reasoning_effort.clone());
                self.apply_reasoning_effort(effort, app_server).await?;
            }
            SettingsAction::ApprovalPolicy => {
                let policy = next_approval_policy(self.approval_policy);
                self.apply_approval_policy(policy, app_server).await?;
            }
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
                self.model = draft.clone();
                app_server
                    .write_config(build_model_selection_edits(
                        &draft,
                        self.reasoning_effort.as_ref(),
                    ))
                    .await?;
                app_server
                    .thread_settings_update(self.thread_settings_update_params(
                        Some(draft.clone()),
                        None,
                        None,
                    ))
                    .await?;
                self.settings.set_info(format!("model set to {draft}"));
            }
            SettingsAction::ServiceTier => {
                if draft.chars().any(char::is_whitespace) {
                    self.settings
                        .set_error("service tier cannot contain whitespace");
                    return Ok(());
                }
                let service_tier = (!draft.is_empty()).then_some(draft.clone());
                self.service_tier = service_tier.clone();
                app_server
                    .write_config(build_service_tier_selection_edits(service_tier.as_deref()))
                    .await?;
                app_server
                    .thread_settings_update(self.thread_settings_update_params(
                        None,
                        None,
                        Some(service_tier.clone()),
                    ))
                    .await?;
                let label = service_tier.as_deref().unwrap_or("default");
                self.settings
                    .set_info(format!("service tier set to {label}"));
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
            | SettingsAction::Tooltips => {}
        }
        Ok(())
    }

    async fn apply_reasoning_effort<S>(
        &mut self,
        effort: Option<ReasoningEffort>,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        self.reasoning_effort = effort.clone();
        app_server
            .write_config(build_model_selection_edits(&self.model, effort.as_ref()))
            .await?;
        app_server
            .thread_settings_update(self.thread_settings_update_params(None, effort.clone(), None))
            .await?;
        let label = effort
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "default".to_string());
        self.settings.set_info(format!("reasoning set to {label}"));
        Ok(())
    }

    async fn apply_approval_policy<S>(
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
            .thread_settings_update(self.thread_settings_update_params(None, None, None))
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
