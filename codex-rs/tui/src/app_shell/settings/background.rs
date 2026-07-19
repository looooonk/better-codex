use super::super::ShellState;
use super::super::backend::AppShellBackend;
use super::super::backend_actions::ActionGroup;
use super::super::backend_actions::BackendActionResult;
use super::super::selector::SelectorState;
use super::super::selector::SelectorValue;
use super::SettingsAction;
use super::ultra_reasoning_concurrency_warning;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_protocol::openai_models::ReasoningEffort;
use color_eyre::Result;

#[derive(Debug)]
pub(in crate::app_shell) struct SettingsUpdate {
    change: SettingsChange,
    edit: Option<(SettingsAction, String)>,
    selector: Option<SelectorState<SelectorValue>>,
}

#[derive(Debug)]
pub(super) enum SettingsChange {
    Model {
        model: String,
        effort: Option<ReasoningEffort>,
        service_tier: Option<String>,
    },
    ServiceTier(Option<String>),
    ReasoningEffort {
        effort: Option<ReasoningEffort>,
        thread_effort: Option<ReasoningEffort>,
    },
    ApprovalPolicy(AskForApproval),
    Theme(Option<String>),
    Animations(bool),
    Tooltips(bool),
}

impl ShellState {
    pub(super) fn schedule_settings_update<S>(
        &mut self,
        app_server: &S,
        change: SettingsChange,
        edits: Vec<ConfigEdit>,
        thread_update: Option<ThreadSettingsUpdateParams>,
    ) where
        S: AppShellBackend,
    {
        let update = SettingsUpdate {
            change,
            edit: self.settings.edit_value(),
            selector: self.selector.clone(),
        };
        let persist = app_server.persist_settings_update_in_background(edits, thread_update);
        self.start_backend_action(ActionGroup::Settings, "saving settings", async move {
            let result = persist.await;
            BackendActionResult::Settings { update, result }
        });
    }

    pub(in crate::app_shell) fn complete_settings_update(
        &mut self,
        update: SettingsUpdate,
        result: Result<()>,
    ) {
        if let Err(err) = result {
            self.settings
                .set_error(format!("failed to save settings: {err:#}"));
            self.report_action_error("failed to save settings", err);
            return;
        }
        match update.change {
            SettingsChange::Model {
                model,
                effort,
                service_tier,
            } => {
                if let Some(collaboration_mode) = self.collaboration_mode.as_mut() {
                    **collaboration_mode = collaboration_mode.with_updates(
                        Some(model.clone()),
                        Some(effort.clone()),
                        /*developer_instructions*/ None,
                    );
                }
                self.model = model;
                self.reasoning_effort = effort;
                self.service_tier = service_tier;
            }
            SettingsChange::ServiceTier(service_tier) => self.service_tier = service_tier,
            SettingsChange::ReasoningEffort {
                effort,
                thread_effort,
            } => {
                self.reasoning_effort = effort.clone();
                if let Some(collaboration_mode) = self.collaboration_mode.as_mut() {
                    **collaboration_mode = collaboration_mode.with_updates(
                        /*model*/ None,
                        Some(effort),
                        /*developer_instructions*/ None,
                    );
                }
                if let Some(warning) = ultra_reasoning_concurrency_warning(
                    thread_effort.as_ref(),
                    self.max_concurrent_threads_per_session,
                ) {
                    self.push_status(warning);
                }
            }
            SettingsChange::ApprovalPolicy(policy) => self.approval_policy = policy,
            SettingsChange::Theme(theme) => self.tui_theme = theme,
            SettingsChange::Animations(animations) => self.animations = animations,
            SettingsChange::Tooltips(show_tooltips) => self.show_tooltips = show_tooltips,
        }
        if let Some((action, draft)) = update.edit {
            self.settings.finish_edit(action, &draft);
        }
        if update.selector.is_some() && self.selector == update.selector {
            self.selector = None;
        }
        self.settings.set_info("settings saved");
        self.status = "ready".to_string();
    }
}
