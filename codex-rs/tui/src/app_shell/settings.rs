use super::DashboardRouteStep;
use super::ShellState;
use super::design::palette;
use super::design::selection_style;
use super::permission_modes::ConfiguredPermissions;
use super::permission_modes::PermissionMode;
use super::selector::ReasoningEffortValue;
use crate::app_theme::TuiAppTheme;
use crate::text_input::EditableText;
use crate::text_input::TextInputAction;
use codex_protocol::openai_models::ReasoningEffort;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::cell::Cell;

mod background;
mod controller;
mod menu;
mod persistence;

pub(super) use background::SettingsUpdate;
pub(super) use persistence::persist_settings_update;

#[derive(Debug, Clone, Default)]
pub(super) struct SettingsState {
    selected: SettingsAction,
    pub(super) custom_permissions: Option<ConfiguredPermissions>,
    pub(super) focused: bool,
    edit: Option<EditableText>,
    feedback: Option<String>,
    pub(super) reveal_selection: Cell<bool>,
    pub(super) effort_save: EffortSave,
}

/// Keep the last committed stop while a previous effort write is in flight.
#[derive(Debug, Clone, Default)]
pub(super) enum EffortSave {
    #[default]
    Idle,
    Saving,
    Queued(ReasoningEffortValue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsAction {
    PermissionMode(PermissionMode),
    CustomPermissions,
    AppTheme,
    Theme,
    Animations,
    GraphicalMath,
    Tooltips,
    McpServers,
    Plugins,
}

impl Default for SettingsAction {
    fn default() -> Self {
        Self::PermissionMode(PermissionMode::Ask)
    }
}

#[derive(Clone, Copy)]
enum SettingRow {
    Heading(&'static str),
    Action(SettingsAction),
    Spacer,
}

// Navigation, rendering, and hit testing use the same ordered rows.
const ROWS: &[SettingRow] = &[
    SettingRow::Heading("Permissions"),
    SettingRow::Action(SettingsAction::PermissionMode(PermissionMode::Ask)),
    SettingRow::Action(SettingsAction::PermissionMode(PermissionMode::AutoReview)),
    SettingRow::Action(SettingsAction::PermissionMode(PermissionMode::FullAccess)),
    SettingRow::Action(SettingsAction::PermissionMode(PermissionMode::ReadOnly)),
    SettingRow::Action(SettingsAction::CustomPermissions),
    SettingRow::Spacer,
    SettingRow::Heading("Appearance"),
    SettingRow::Action(SettingsAction::AppTheme),
    SettingRow::Action(SettingsAction::Theme),
    SettingRow::Action(SettingsAction::Animations),
    SettingRow::Action(SettingsAction::Tooltips),
    SettingRow::Spacer,
    SettingRow::Heading("Experimental"),
    SettingRow::Action(SettingsAction::GraphicalMath),
    SettingRow::Spacer,
    SettingRow::Heading("Integrations"),
    SettingRow::Action(SettingsAction::McpServers),
    SettingRow::Action(SettingsAction::Plugins),
];

impl ShellState {
    pub(super) fn equation_preference(&self) -> std::path::PathBuf {
        self.codex_home.join("better-codex").join("graphical-math")
    }

    fn toggle_graphical_math(&mut self) {
        let path = self.equation_preference();
        let graphics = self.graphics.get_mut();
        let result = if graphics.enabled() {
            std::fs::remove_file(&path)
                .or_else(|err| {
                    if err.kind() == std::io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(err)
                    }
                })
                .map(|()| graphics.disable())
        } else {
            if let Err(reason) = graphics.enable() {
                self.settings.set_error(reason);
                return;
            }
            let result = std::fs::create_dir_all(self.codex_home.join("better-codex"))
                .and_then(|()| std::fs::write(&path, []));
            if result.is_err() {
                graphics.disable();
            }
            result
        };
        if let Err(err) = result {
            self.settings
                .set_error(format!("Could not save the equation preference: {err}"));
        }
        self.transcript_render_cache.get_mut().clear();
    }
}

impl SettingsState {
    pub(super) fn set_custom_permissions(&mut self, permissions: Option<ConfiguredPermissions>) {
        if permissions.is_none() && self.selected == SettingsAction::CustomPermissions {
            self.focus_action(SettingsAction::default());
        }
        self.custom_permissions = permissions;
    }

    fn rows(&self) -> impl DoubleEndedIterator<Item = &SettingRow> + Clone {
        ROWS.iter().filter(|row| {
            !matches!(row, SettingRow::Action(SettingsAction::CustomPermissions))
                || self.custom_permissions.is_some()
        })
    }

    pub(super) fn lines(&self, shell: &ShellState, width: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::with_capacity(ROWS.len() + 2);
        for row in self.rows() {
            match *row {
                SettingRow::Heading(title) => {
                    let spans = vec![
                        "◆ ".fg(palette::focus()),
                        title.to_uppercase().fg(palette::muted()).bold(),
                    ];
                    lines.push(spans.into());
                }
                SettingRow::Spacer => lines.push(Line::default()),
                SettingRow::Action(action) => {
                    let selected = self.focused && self.selected == action;
                    lines.push(setting_row(action, selected, shell));
                    if self.selected == action {
                        if let Some(edit) = &self.edit {
                            lines.push(Line::from(vec![
                                "  Edit ".fg(palette::cyan()),
                                edit.text_with_cursor_window(width.saturating_sub(7).max(1))
                                    .into(),
                            ]));
                        }
                        if let Some(message) = &self.feedback {
                            lines.push(Line::from(message.clone().fg(palette::error())));
                        }
                    }
                }
            }
        }
        lines
    }

    fn selected_line(&self) -> usize {
        self.rows()
            .position(|row| matches!(row, SettingRow::Action(action) if *action == self.selected))
            .unwrap_or_default()
    }

    pub(super) fn selected_line_range(&self) -> std::ops::RangeInclusive<usize> {
        let selected = self.selected_line();
        selected
            ..=selected + usize::from(self.edit.is_some()) + usize::from(self.feedback.is_some())
    }

    pub(super) fn action_at_line(&self, line: usize) -> Option<SettingsAction> {
        let range = self.selected_line_range();
        let index = if line > *range.end() {
            line - (range.end() - range.start())
        } else if line > *range.start() {
            return None;
        } else {
            line
        };
        match self.rows().nth(index)? {
            SettingRow::Action(action) => Some(*action),
            SettingRow::Heading(_) | SettingRow::Spacer => None,
        }
    }

    pub(super) fn move_up(&mut self) {
        let previous = self
            .rows()
            .take(self.selected_line())
            .filter_map(SettingRow::action)
            .last();
        if let Some(action) = previous {
            self.focus_action(action);
        }
    }

    pub(super) fn move_down(&mut self) {
        let next = self
            .rows()
            .skip(self.selected_line() + 1)
            .find_map(SettingRow::action);
        if let Some(action) = next {
            self.focus_action(action);
        }
    }

    pub(super) fn step_section(&mut self, direction: DashboardRouteStep) {
        let rows: Vec<_> = self.rows().collect();
        let starts: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| matches!(row, SettingRow::Heading(_)).then_some(index + 1))
            .collect();
        let selected = self.selected_line();
        let section = starts.partition_point(|&start| start <= selected) - 1;
        let target = match direction {
            DashboardRouteStep::Next => (section + 1) % starts.len(),
            DashboardRouteStep::Previous => (section + starts.len() - 1) % starts.len(),
        };
        if let Some(action) = rows[starts[target]].action() {
            self.focus_action(action);
        }
    }

    pub(super) fn selected_action(&self) -> SettingsAction {
        self.selected
    }

    pub(super) fn start_theme_edit(&mut self, current_value: String) {
        self.focus_action(SettingsAction::Theme);
        self.edit = Some(EditableText::new(current_value));
    }

    pub(super) fn focus_action(&mut self, action: SettingsAction) {
        self.selected = action;
        self.edit = None;
        self.feedback = None;
        self.reveal_selection.set(true);
    }

    pub(super) fn editing(&self) -> bool {
        self.edit.is_some()
    }

    pub(super) fn push_edit_char(&mut self, ch: char) {
        if let Some(edit) = &mut self.edit {
            edit.insert_char(ch);
        }
    }

    pub(super) fn insert_edit_text(&mut self, text: &str) {
        if let Some(edit) = &mut self.edit {
            edit.insert_str(text);
        }
    }

    pub(super) fn edit_text(&mut self, action: TextInputAction) {
        if let Some(edit) = &mut self.edit {
            edit.apply(action);
        }
    }

    pub(super) fn cancel_edit(&mut self) {
        self.edit = None;
    }

    pub(super) fn edit_value(&self) -> Option<String> {
        self.edit.as_ref().map(|edit| edit.text().trim().to_owned())
    }

    pub(super) fn finish_edit(&mut self, draft: &str) {
        if self.edit_value().as_deref() == Some(draft) {
            self.edit = None;
        }
    }

    pub(super) fn set_error(&mut self, message: impl Into<String>) {
        self.feedback = Some(message.into());
        self.reveal_selection.set(true);
    }
}

impl SettingRow {
    fn action(&self) -> Option<SettingsAction> {
        match self {
            Self::Action(action) => Some(*action),
            Self::Heading(_) | Self::Spacer => None,
        }
    }
}

fn setting_row(action: SettingsAction, selected: bool, shell: &ShellState) -> Line<'static> {
    let marker = if selected {
        "› ".fg(palette::focus()).bold()
    } else {
        "  ".into()
    };
    let choice = |label: &str, checked: bool| {
        let mark = if checked { "●" } else { "○" };
        format!("{mark} {label}")
    };
    let label = match action {
        SettingsAction::PermissionMode(mode) => {
            let mut label = choice(mode.caption(), Some(mode) == shell.permission_mode());
            if !shell.permission_modes.contains(&mode) {
                label.push_str(" (unavailable)");
            }
            label
        }
        SettingsAction::CustomPermissions => choice("⚙ Custom", shell.custom_permissions_active()),
        SettingsAction::AppTheme => format!("Theme: {} ▾", app_theme_label(shell.app_theme)),
        SettingsAction::Theme => format!(
            "Syntax theme: {} ▾",
            shell.tui_theme.as_deref().unwrap_or("default")
        ),
        SettingsAction::GraphicalMath => {
            choice("Render LaTeX as images", shell.graphics.borrow().enabled())
        }
        SettingsAction::Animations => choice("Animations", shell.animations),
        SettingsAction::Tooltips => choice("Tooltips", shell.show_tooltips),
        SettingsAction::McpServers => format!("MCP servers: {} ▾", shell.mcp_inventory.label()),
        SettingsAction::Plugins => format!("Plugins: {} ▾", shell.plugin_inventory.label()),
    };
    let color = if matches!(
        action,
        SettingsAction::PermissionMode(PermissionMode::FullAccess)
    ) {
        palette::error()
    } else {
        palette::text()
    };
    let line = Line::from(vec![marker, label.fg(color)]);
    if selected {
        line.set_style(selection_style())
    } else {
        line
    }
}

fn app_theme_label(theme: TuiAppTheme) -> &'static str {
    match theme {
        TuiAppTheme::TokyoNight => "Tokyo Night",
        TuiAppTheme::GruvboxDark => "Gruvbox Dark",
        TuiAppTheme::CatppuccinMocha => "Catppuccin Mocha",
        TuiAppTheme::Monochrome => "Monochrome",
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
        ReasoningEffort::Persistent => "Persistent".to_string(),
        ReasoningEffort::Custom(effort) => effort.clone(),
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
