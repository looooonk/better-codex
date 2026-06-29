use super::dashboard::dashboard_value;
use super::integrations::McpInventorySummary;
use super::integrations::PluginInventorySummary;
use crate::text_formatting::truncate_text;
use codex_app_server_protocol::AskForApproval;
use codex_protocol::openai_models::ReasoningEffort;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

mod controller;

#[derive(Debug, Clone, Default)]
pub(super) struct SettingsState {
    page: SettingsPage,
    selected: usize,
    pub(super) focused: bool,
    edit: Option<SettingsEdit>,
    feedback: Option<SettingsFeedback>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SettingsPage {
    #[default]
    Model,
    Permissions,
    Appearance,
    Integrations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsAction {
    Model,
    ReasoningEffort,
    ServiceTier,
    ApprovalPolicy,
    Theme,
    Animations,
    Tooltips,
    McpServers,
    Plugins,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SettingsView {
    pub(super) model: String,
    pub(super) reasoning_effort: Option<ReasoningEffort>,
    pub(super) service_tier: Option<String>,
    pub(super) approval_policy: AskForApproval,
    pub(super) theme: Option<String>,
    pub(super) animations: bool,
    pub(super) show_tooltips: bool,
    pub(super) mcp_inventory: McpInventorySummary,
    pub(super) plugin_inventory: PluginInventorySummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SettingsEdit {
    action: SettingsAction,
    draft: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SettingsFeedbackTone {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SettingsFeedback {
    tone: SettingsFeedbackTone,
    message: String,
}

impl SettingsState {
    pub(super) fn lines(&self, view: &SettingsView, width: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let focus = if self.focused {
            "focused"
        } else {
            "ctrl+3 focus"
        };
        lines.push(Line::from(vec![
            focus.cyan(),
            " ".dim(),
            self.page.label().dim(),
            " ".dim(),
            format!("{} fields", self.actions().len()).dim(),
        ]));
        lines.push(Line::from(vec![
            tab("Model", self.page == SettingsPage::Model),
            "  ".dim(),
            tab("Permissions", self.page == SettingsPage::Permissions),
            "  ".dim(),
            tab("Appearance", self.page == SettingsPage::Appearance),
            "  ".dim(),
            tab("Integrations", self.page == SettingsPage::Integrations),
        ]));

        if let Some(edit) = &self.edit {
            let label = format!("edit {}", edit.action.label());
            let prefix_width = label.len() + 1;
            lines.push(Line::from(vec![
                label.cyan(),
                " ".dim(),
                dashboard_value(&edit.draft, width, prefix_width).into(),
            ]));
        }
        if let Some(feedback) = &self.feedback {
            let line = dashboard_value(&feedback.message, width, /*prefix_width*/ 0);
            let span = match feedback.tone {
                SettingsFeedbackTone::Info => line.green(),
                SettingsFeedbackTone::Error => line.red(),
            };
            lines.push(Line::from(span));
        }

        let remaining = 8usize.saturating_sub(lines.len());
        for (index, action) in self.actions().iter().take(remaining).enumerate() {
            lines.push(setting_row(*action, index == self.selected, view, width));
        }
        lines.push(Line::from("Enter edit/cycle  Tab page  Esc composer".dim()));
        lines
    }

    pub(super) fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub(super) fn move_down(&mut self) {
        self.selected = self
            .selected
            .saturating_add(1)
            .min(self.actions().len().saturating_sub(1));
    }

    pub(super) fn next_page(&mut self) {
        self.page = match self.page {
            SettingsPage::Model => SettingsPage::Permissions,
            SettingsPage::Permissions => SettingsPage::Appearance,
            SettingsPage::Appearance => SettingsPage::Integrations,
            SettingsPage::Integrations => SettingsPage::Model,
        };
        self.selected = 0;
        self.edit = None;
    }

    pub(super) fn previous_page(&mut self) {
        self.page = match self.page {
            SettingsPage::Model => SettingsPage::Appearance,
            SettingsPage::Permissions => SettingsPage::Model,
            SettingsPage::Appearance => SettingsPage::Permissions,
            SettingsPage::Integrations => SettingsPage::Appearance,
        };
        self.selected = 0;
        self.edit = None;
    }

    pub(super) fn selected_action(&self) -> SettingsAction {
        self.actions()[self.selected.min(self.actions().len().saturating_sub(1))]
    }

    pub(super) fn start_edit(&mut self, action: SettingsAction, current_value: String) {
        self.focus_action(action);
        self.edit = Some(SettingsEdit {
            action,
            draft: current_value,
        });
        self.feedback = None;
    }

    pub(super) fn focus_action(&mut self, action: SettingsAction) {
        self.page = match action {
            SettingsAction::Model
            | SettingsAction::ReasoningEffort
            | SettingsAction::ServiceTier => SettingsPage::Model,
            SettingsAction::ApprovalPolicy => SettingsPage::Permissions,
            SettingsAction::Theme | SettingsAction::Animations | SettingsAction::Tooltips => {
                SettingsPage::Appearance
            }
            SettingsAction::McpServers | SettingsAction::Plugins => SettingsPage::Integrations,
        };
        self.selected = self
            .actions()
            .iter()
            .position(|candidate| *candidate == action)
            .unwrap_or(0);
        self.edit = None;
        self.feedback = None;
    }

    pub(super) fn editing(&self) -> bool {
        self.edit.is_some()
    }

    pub(super) fn push_edit_char(&mut self, ch: char) {
        if let Some(edit) = &mut self.edit {
            edit.draft.push(ch);
        }
    }

    pub(super) fn backspace_edit(&mut self) {
        if let Some(edit) = &mut self.edit {
            edit.draft.pop();
        }
    }

    pub(super) fn cancel_edit(&mut self) {
        self.edit = None;
    }

    pub(super) fn take_edit(&mut self) -> Option<(SettingsAction, String)> {
        self.edit
            .take()
            .map(|edit| (edit.action, edit.draft.trim().to_string()))
    }

    pub(super) fn set_info(&mut self, message: impl Into<String>) {
        self.feedback = Some(SettingsFeedback {
            tone: SettingsFeedbackTone::Info,
            message: message.into(),
        });
    }

    pub(super) fn set_error(&mut self, message: impl Into<String>) {
        self.feedback = Some(SettingsFeedback {
            tone: SettingsFeedbackTone::Error,
            message: message.into(),
        });
    }

    fn actions(&self) -> &'static [SettingsAction] {
        match self.page {
            SettingsPage::Model => &[
                SettingsAction::Model,
                SettingsAction::ReasoningEffort,
                SettingsAction::ServiceTier,
            ],
            SettingsPage::Permissions => &[SettingsAction::ApprovalPolicy],
            SettingsPage::Appearance => &[
                SettingsAction::Theme,
                SettingsAction::Animations,
                SettingsAction::Tooltips,
            ],
            SettingsPage::Integrations => &[SettingsAction::McpServers, SettingsAction::Plugins],
        }
    }
}

impl SettingsPage {
    fn label(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Permissions => "permissions",
            Self::Appearance => "appearance",
            Self::Integrations => "integrations",
        }
    }
}

impl SettingsAction {
    fn label(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::ReasoningEffort => "reasoning",
            Self::ServiceTier => "service tier",
            Self::ApprovalPolicy => "approval",
            Self::Theme => "theme",
            Self::Animations => "animations",
            Self::Tooltips => "tooltips",
            Self::McpServers => "mcp servers",
            Self::Plugins => "plugins",
        }
    }
}

fn tab(label: &'static str, active: bool) -> Span<'static> {
    if active {
        label.cyan().bold()
    } else {
        label.dim()
    }
}

fn setting_row(
    action: SettingsAction,
    selected: bool,
    view: &SettingsView,
    width: usize,
) -> Line<'static> {
    let marker = if selected {
        ">".cyan().bold()
    } else {
        " ".dim()
    };
    let label = action.label();
    let value = match action {
        SettingsAction::Model => view.model.clone(),
        SettingsAction::ReasoningEffort => view
            .reasoning_effort
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "default".to_string()),
        SettingsAction::ServiceTier => view
            .service_tier
            .clone()
            .filter(|tier| !tier.trim().is_empty())
            .unwrap_or_else(|| "default".to_string()),
        SettingsAction::ApprovalPolicy => approval_policy_label(view.approval_policy).to_string(),
        SettingsAction::Theme => view.theme.clone().unwrap_or_else(|| "default".to_string()),
        SettingsAction::Animations => on_off(view.animations).to_string(),
        SettingsAction::Tooltips => on_off(view.show_tooltips).to_string(),
        SettingsAction::McpServers => view.mcp_inventory.label(),
        SettingsAction::Plugins => view.plugin_inventory.label(),
    };
    let text = format!("{label}: {value}");
    Line::from(vec![
        marker,
        " ".dim(),
        truncate_text(&text, width.saturating_sub(2)).into(),
    ])
}

pub(super) fn approval_policy_label(policy: AskForApproval) -> &'static str {
    match policy {
        AskForApproval::UnlessTrusted => "untrusted",
        AskForApproval::OnRequest => "on-request",
        AskForApproval::Never => "never",
        AskForApproval::Granular { .. } => "granular",
    }
}

pub(super) fn next_approval_policy(policy: AskForApproval) -> AskForApproval {
    match policy {
        AskForApproval::UnlessTrusted => AskForApproval::OnRequest,
        AskForApproval::OnRequest => AskForApproval::Never,
        AskForApproval::Never | AskForApproval::Granular { .. } => AskForApproval::UnlessTrusted,
    }
}

pub(super) fn next_reasoning_effort(effort: Option<ReasoningEffort>) -> Option<ReasoningEffort> {
    match effort {
        None => Some(ReasoningEffort::Minimal),
        Some(ReasoningEffort::Minimal) => Some(ReasoningEffort::Low),
        Some(ReasoningEffort::Low) => Some(ReasoningEffort::Medium),
        Some(ReasoningEffort::Medium) => Some(ReasoningEffort::High),
        Some(ReasoningEffort::High) => Some(ReasoningEffort::XHigh),
        Some(ReasoningEffort::XHigh) => Some(ReasoningEffort::Ultra),
        Some(ReasoningEffort::Ultra | ReasoningEffort::None | ReasoningEffort::Custom(_)) => None,
    }
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}
