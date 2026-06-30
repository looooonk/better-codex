use super::ShellState;
use super::backend::AppShellBackend;
use crate::app_server_session::EXTERNAL_AGENT_CONFIG_IMPORT_IN_PROGRESS_MESSAGE;
use crate::external_agent_config_migration_flow::EXTERNAL_AGENT_CONFIG_MIGRATION_DAEMON_UNAVAILABLE_MESSAGE;
use crate::external_agent_config_migration_flow::EXTERNAL_AGENT_CONFIG_MIGRATION_NO_ITEMS_MESSAGE;
use crate::external_agent_config_migration_flow::EXTERNAL_AGENT_CONFIG_MIGRATION_REMOTE_UNAVAILABLE_MESSAGE;
use crate::external_agent_config_migration_flow::external_agent_config_migration_finished_lines;
use crate::external_agent_config_migration_flow::external_agent_config_migration_started_lines;
use crate::external_agent_config_migration_model::external_agent_config_migration_count_summary;
use crate::external_agent_config_migration_model::external_agent_config_migration_item_detail;
use crate::external_agent_config_migration_model::external_agent_config_migration_item_label;
use codex_app_server_protocol::ExternalAgentConfigDetectParams;
use codex_app_server_protocol::ExternalAgentConfigImportCompletedNotification;
use codex_app_server_protocol::ExternalAgentConfigMigrationItem;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::prelude::Stylize as _;
use ratatui::text::Line;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExternalAgentImportState {
    items: Vec<ExternalAgentConfigMigrationItem>,
    selected: Vec<bool>,
    focused: usize,
    error: Option<String>,
}

impl ExternalAgentImportState {
    fn new(items: Vec<ExternalAgentConfigMigrationItem>) -> Self {
        let selected = vec![true; items.len()];
        Self {
            items,
            selected,
            focused: 0,
            error: None,
        }
    }

    fn move_up(&mut self) {
        self.focused = self.focused.saturating_sub(1);
    }

    fn move_down(&mut self) {
        self.focused = self
            .focused
            .saturating_add(1)
            .min(self.items.len().saturating_sub(1));
    }

    fn toggle_focused(&mut self) {
        if let Some(selected) = self.selected.get_mut(self.focused) {
            *selected = !*selected;
        }
    }

    fn selected_items(&self) -> Vec<ExternalAgentConfigMigrationItem> {
        self.items
            .iter()
            .zip(&self.selected)
            .filter_map(|(item, selected)| selected.then_some(item.clone()))
            .collect()
    }

    fn remaining_item_count(&self) -> usize {
        self.items.len().saturating_sub(self.selected_items().len())
    }

    fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    pub(super) fn lines(&self) -> Vec<Line<'static>> {
        let selected_items = self.selected_items();
        let summary = if selected_items.is_empty() {
            "Nothing selected".to_string()
        } else {
            external_agent_config_migration_count_summary(&selected_items)
        };
        let mut lines = vec![
            Line::from(vec![
                "Review Claude Code setup".bold(),
                "  ".dim(),
                summary.cyan(),
            ]),
            Line::from(""),
        ];
        if let Some(error) = &self.error {
            lines.push(error.clone().red().into());
            lines.push(Line::from(""));
        }
        for (index, item) in self.items.iter().enumerate() {
            let selected = self.selected.get(index).copied().unwrap_or_default();
            let cursor = if index == self.focused { ">" } else { " " };
            let checkbox = if selected { "[x]" } else { "[ ]" };
            let label = external_agent_config_migration_item_label(item);
            let detail = external_agent_config_migration_item_detail(item)
                .unwrap_or_else(|| item.description.clone());
            let scope = item
                .cwd
                .as_ref()
                .map_or_else(|| "home".to_string(), |cwd| display_path(cwd));
            let row = Line::from(vec![
                cursor.cyan().bold(),
                " ".dim(),
                checkbox.cyan(),
                " ".dim(),
                label.to_string().into(),
                " - ".dim(),
                scope.dim(),
            ]);
            lines.push(if index == self.focused {
                row.bold()
            } else {
                row
            });
            lines.push(Line::from(vec!["      ".into(), detail.dim()]));
        }
        lines.push(Line::from(""));
        lines.push("Enter import  Space toggle  Esc cancel".dim().into());
        lines
    }
}

impl ShellState {
    pub(super) async fn start_external_agent_import_review<S>(
        &mut self,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        if app_server.uses_remote_workspace() {
            self.push_error(EXTERNAL_AGENT_CONFIG_MIGRATION_REMOTE_UNAVAILABLE_MESSAGE);
            return Ok(());
        }
        if !app_server.uses_embedded_app_server() {
            self.push_error(EXTERNAL_AGENT_CONFIG_MIGRATION_DAEMON_UNAVAILABLE_MESSAGE);
            return Ok(());
        }
        if app_server.external_agent_config_import_in_progress() {
            self.push_error(EXTERNAL_AGENT_CONFIG_IMPORT_IN_PROGRESS_MESSAGE);
            return Ok(());
        }

        self.push_status("checking for Claude Code setup");
        let cwd = PathBuf::from(&self.cwd);
        match app_server
            .external_agent_config_detect(ExternalAgentConfigDetectParams {
                include_home: true,
                cwds: Some(vec![cwd]),
            })
            .await
        {
            Ok(response) if response.items.is_empty() => {
                self.push_status(EXTERNAL_AGENT_CONFIG_MIGRATION_NO_ITEMS_MESSAGE);
            }
            Ok(response) => {
                self.pending_external_agent_import =
                    Some(ExternalAgentImportState::new(response.items));
                self.push_status("review Claude Code setup before importing");
            }
            Err(err) => {
                self.push_error(format!("Could not check for Claude Code setup: {err}"));
            }
        }
        Ok(())
    }

    pub(super) async fn handle_external_agent_import_key<S>(
        &mut self,
        key: KeyEvent,
        app_server: &mut S,
    ) -> Result<bool>
    where
        S: AppShellBackend,
    {
        match key.code {
            KeyCode::Esc => {
                self.pending_external_agent_import = None;
                self.push_status("Claude Code import cancelled");
                Ok(true)
            }
            KeyCode::Up => {
                if let Some(pending) = &mut self.pending_external_agent_import {
                    pending.move_up();
                }
                Ok(true)
            }
            KeyCode::Down => {
                if let Some(pending) = &mut self.pending_external_agent_import {
                    pending.move_down();
                }
                Ok(true)
            }
            KeyCode::Char(' ') => {
                if let Some(pending) = &mut self.pending_external_agent_import {
                    pending.toggle_focused();
                }
                Ok(true)
            }
            KeyCode::Enter => {
                self.submit_external_agent_import(app_server).await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub(super) fn report_external_agent_import_finished(
        &mut self,
        notification: &ExternalAgentConfigImportCompletedNotification,
    ) {
        self.push_plain_status_lines(external_agent_config_migration_finished_lines(notification));
    }

    async fn submit_external_agent_import<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(pending) = self.pending_external_agent_import.as_mut() else {
            return Ok(());
        };
        let selected_items = pending.selected_items();
        if selected_items.is_empty() {
            pending.set_error("Select at least one item to import.");
            return Ok(());
        }
        match app_server
            .external_agent_config_import(selected_items.clone())
            .await
        {
            Ok(()) => {
                let remaining_item_count = pending.remaining_item_count();
                self.pending_external_agent_import = None;
                self.push_plain_status_lines(external_agent_config_migration_started_lines(
                    &selected_items,
                    remaining_item_count,
                ));
            }
            Err(err) => {
                pending.set_error(format!("Import failed: {err}"));
            }
        }
        Ok(())
    }

    fn push_plain_status_lines(&mut self, lines: Vec<Line<'static>>) {
        for line in lines {
            self.push_status(line_to_plain_text(&line));
        }
    }
}

fn display_path(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .map_or_else(|| path.display().to_string(), ToString::to_string)
}

fn line_to_plain_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}
