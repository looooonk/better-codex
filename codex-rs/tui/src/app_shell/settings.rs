use super::dashboard::dashboard_value;
use super::design::palette;
use super::design::selection_style;
use super::integrations::McpInventorySummary;
use super::integrations::PluginInventorySummary;
use crate::text_input::EditableText;
use crate::text_input::TextInputAction;
use codex_app_server_protocol::AskForApproval;
use codex_protocol::openai_models::ReasoningEffort;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;

mod background;
mod controller;
mod persistence;
mod tabs;

pub(super) use background::SettingsUpdate;
pub(super) use persistence::persist_settings_update;
pub(super) use tabs::SettingsTabs;

const ULTRA_REASONING_CONCURRENCY_WARNING_THRESHOLD: usize = 8;
const SETTINGS_PAGE_LINE_COUNT: usize = 5;

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
    draft: EditableText,
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
        let mut lines = SettingsTabs::new(width).lines(self.page).to_vec();

        if let Some(edit) = &self.edit {
            let label = format!("edit {}", edit.action.label());
            let prefix_width = label.len() + 1;
            let draft = edit
                .draft
                .text_with_cursor_window(width.saturating_sub(prefix_width).max(1));
            lines.push(Line::from(vec![
                label.cyan(),
                " ".dim(),
                dashboard_value(&draft, width, prefix_width).into(),
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
        if lines.len() < SETTINGS_PAGE_LINE_COUNT {
            lines.resize_with(SETTINGS_PAGE_LINE_COUNT, Line::default);
        }
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
            SettingsPage::Model => SettingsPage::Integrations,
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

    pub(super) fn select_at(&mut self, line: usize, column: usize, width: usize) -> bool {
        self.focused = true;
        let page = matches!(line, 0 | 1)
            .then(|| SettingsTabs::new(width).page_at(column))
            .flatten();
        if let Some(page) = page {
            self.set_page(page);
            return false;
        }
        if matches!(line, 0 | 1) {
            return false;
        }
        if line < self.action_line_start() {
            return false;
        }
        let action_line = line.saturating_sub(self.action_line_start());
        if action_line >= self.actions().len() {
            return false;
        }
        self.selected = action_line;
        self.edit = None;
        self.feedback = None;
        true
    }

    pub(super) fn start_edit(&mut self, action: SettingsAction, current_value: String) {
        self.focus_action(action);
        self.edit = Some(SettingsEdit {
            action,
            draft: EditableText::new(current_value),
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
            edit.draft.insert_char(ch);
        }
    }

    pub(super) fn insert_edit_text(&mut self, text: &str) {
        if let Some(edit) = &mut self.edit {
            edit.draft.insert_str(text);
        }
    }

    pub(super) fn edit_text(&mut self, action: TextInputAction) {
        if let Some(edit) = &mut self.edit {
            edit.draft.apply(action);
        }
    }

    pub(super) fn cancel_edit(&mut self) {
        self.edit = None;
    }

    pub(super) fn take_edit(&mut self) -> Option<(SettingsAction, String)> {
        self.edit
            .take()
            .map(|edit| (edit.action, edit.draft.into_text().trim().to_string()))
    }

    pub(super) fn edit_value(&self) -> Option<(SettingsAction, String)> {
        self.edit
            .as_ref()
            .map(|edit| (edit.action, edit.draft.text().trim().to_string()))
    }

    pub(super) fn finish_edit(&mut self, action: SettingsAction, draft: &str) {
        if self.edit_value().as_ref() == Some(&(action, draft.to_string())) {
            self.edit = None;
        }
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

    fn action_line_start(&self) -> usize {
        2usize
            .saturating_add(usize::from(self.edit.is_some()))
            .saturating_add(usize::from(self.feedback.is_some()))
    }

    fn set_page(&mut self, page: SettingsPage) {
        if self.page != page {
            self.page = page;
            self.selected = 0;
            self.edit = None;
            self.feedback = None;
        }
    }
}

impl SettingsPage {
    const ALL: [Self; 4] = [
        Self::Model,
        Self::Permissions,
        Self::Appearance,
        Self::Integrations,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Model => "Model",
            Self::Permissions => "Permissions",
            Self::Appearance => "Appearance",
            Self::Integrations => "Integrations",
        }
    }
}

impl SettingsAction {
    fn label(self) -> &'static str {
        match self {
            Self::Model => "Model",
            Self::ReasoningEffort => "Reasoning",
            Self::ServiceTier => "Service tier",
            Self::ApprovalPolicy => "Approval",
            Self::Theme => "Syntax theme",
            Self::Animations => "Animations",
            Self::Tooltips => "Tooltips",
            Self::McpServers => "MCP servers",
            Self::Plugins => "Plugins",
        }
    }
}

fn setting_row(
    action: SettingsAction,
    selected: bool,
    view: &SettingsView,
    width: usize,
) -> Line<'static> {
    let marker = if selected {
        "›".fg(palette::FOCUS).bold()
    } else {
        " ".into()
    };
    let label = action.label();
    let value = match action {
        SettingsAction::Model => view.model.clone(),
        SettingsAction::ReasoningEffort => view
            .reasoning_effort
            .as_ref()
            .map(reasoning_effort_label)
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
    let line = Line::from(vec![
        marker,
        " ".into(),
        dashboard_value(&text, width, /*prefix_width*/ 2).fg(if selected {
            palette::TEXT
        } else {
            palette::MUTED
        }),
    ]);
    if selected {
        line.set_style(selection_style())
    } else {
        line
    }
}

pub(super) fn approval_policy_label(policy: AskForApproval) -> &'static str {
    match policy {
        AskForApproval::UnlessTrusted => "untrusted",
        AskForApproval::OnRequest => "on-request",
        AskForApproval::Never => "never",
        AskForApproval::Granular { .. } => "granular",
    }
}

pub(super) fn reasoning_effort_label(effort: &ReasoningEffort) -> String {
    match effort {
        ReasoningEffort::None => "None".to_string(),
        ReasoningEffort::Minimal => "Minimal".to_string(),
        ReasoningEffort::Low => "Low".to_string(),
        ReasoningEffort::Medium => "Medium".to_string(),
        ReasoningEffort::High => "High".to_string(),
        ReasoningEffort::XHigh => "Extra high".to_string(),
        ReasoningEffort::Max => "Max".to_string(),
        ReasoningEffort::Ultra => "Ultra".to_string(),
        ReasoningEffort::Custom(effort) => effort.clone(),
    }
}

pub(super) fn ultra_reasoning_concurrency_warning(
    effort: Option<&ReasoningEffort>,
    max_concurrent_threads_per_session: usize,
) -> Option<String> {
    if effort != Some(&ReasoningEffort::Ultra)
        || max_concurrent_threads_per_session < ULTRA_REASONING_CONCURRENCY_WARNING_THRESHOLD
    {
        return None;
    }

    let max_subagents = max_concurrent_threads_per_session.saturating_sub(1);
    Some(format!(
        "Ultra reasoning may proactively use multiple agents. This session is configured for \
         {max_concurrent_threads_per_session} concurrent threads with up to {max_subagents} \
         subagents which can increase usage quickly. Consider setting \
         features.multi_agent_v2.max_concurrent_threads_per_session below 8."
    ))
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
