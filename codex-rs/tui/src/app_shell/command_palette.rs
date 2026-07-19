use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use super::is_unmodified_action_key;
use super::navigation::DashboardRoute;
use super::settings::SettingsAction;
use crate::key_hint;
use crate::legacy_core::config::Config;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandPaletteAction {
    NewSession,
    CopyTranscript,
    ClearTranscript,
    SelectLatestTranscript,
    ScrollTranscriptTop,
    ScrollTranscriptBottom,
    InterruptTurn,
    SwitchModel,
    ChangePermissions,
    ResumeThread,
    ForkThread,
    ImportExternalAgentConfig,
    CompactContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandPaletteEntry {
    pub(super) action: CommandPaletteAction,
    pub(super) title: &'static str,
    pub(super) detail: &'static str,
    pub(super) enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CommandPaletteState {
    selected: usize,
}

impl CommandPaletteState {
    pub(super) fn selected(&self) -> usize {
        self.selected
    }

    pub(super) fn move_up(&mut self, entries: &[CommandPaletteEntry]) {
        if entries.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    pub(super) fn move_down(&mut self, entries: &[CommandPaletteEntry]) {
        let Some(max_index) = entries.len().checked_sub(1) else {
            self.selected = 0;
            return;
        };
        self.selected = self.selected.saturating_add(1).min(max_index);
    }

    pub(super) fn select_last(&mut self, entries: &[CommandPaletteEntry]) {
        self.selected = entries.len().saturating_sub(1);
    }

    pub(super) fn select(&mut self, index: usize, entries: &[CommandPaletteEntry]) {
        self.selected = index.min(entries.len().saturating_sub(1));
    }

    pub(super) fn selected_action(
        &self,
        entries: &[CommandPaletteEntry],
    ) -> Option<CommandPaletteAction> {
        entries
            .get(self.selected)
            .filter(|entry| entry.enabled)
            .map(|entry| entry.action)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CommandPaletteContext {
    pub(super) active_turn: bool,
    pub(super) can_copy_transcript: bool,
    pub(super) has_transcript: bool,
}

pub(super) fn command_palette_entries(context: CommandPaletteContext) -> Vec<CommandPaletteEntry> {
    vec![
        CommandPaletteEntry {
            action: CommandPaletteAction::NewSession,
            title: "New session",
            detail: "Start an empty session in the current workspace",
            enabled: true,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::CopyTranscript,
            title: "Copy transcript item",
            detail: "Copy selection or latest assistant message",
            enabled: context.can_copy_transcript,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ClearTranscript,
            title: "Clear visible transcript",
            detail: "Keep the thread, reset the app surface",
            enabled: context.has_transcript,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::SelectLatestTranscript,
            title: "Select latest transcript item",
            detail: "Enter transcript selection mode",
            enabled: context.has_transcript,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ScrollTranscriptTop,
            title: "Jump transcript to top",
            detail: "Show the oldest retained transcript rows",
            enabled: context.has_transcript,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ScrollTranscriptBottom,
            title: "Jump transcript to bottom",
            detail: "Return to the live conversation tail",
            enabled: context.has_transcript,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::InterruptTurn,
            title: "Interrupt active turn",
            detail: "Stop the current agent turn",
            enabled: context.active_turn,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::SwitchModel,
            title: "Switch model",
            detail: "Open model settings",
            enabled: true,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ChangePermissions,
            title: "Change permissions",
            detail: "Open approval policy settings",
            enabled: true,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ResumeThread,
            title: "Resume thread",
            detail: "Open native session list",
            enabled: true,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ForkThread,
            title: "Fork thread",
            detail: "Open native session list",
            enabled: true,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ImportExternalAgentConfig,
            title: "Import Claude Code setup",
            detail: "Review detected setup before importing",
            enabled: true,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::CompactContext,
            title: "Compact context",
            detail: "Compact the current thread context",
            enabled: context.has_transcript && !context.active_turn,
        },
    ]
}

impl ShellState {
    pub(super) async fn handle_command_palette_key<S>(
        &mut self,
        key: KeyEvent,
        config: &Config,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        if key_hint::ctrl(KeyCode::Char('p')).is_press(key) {
            self.close_command_palette();
            return Ok(());
        }
        if !is_unmodified_action_key(key) {
            return Ok(());
        }
        match key.code {
            KeyCode::Esc => {
                self.close_command_palette();
            }
            KeyCode::Enter => {
                self.execute_selected_command_palette_action(config, app_server)
                    .await?;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let entries = self.command_palette_entries();
                if let Some(palette) = &mut self.command_palette {
                    palette.move_up(&entries);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let entries = self.command_palette_entries();
                if let Some(palette) = &mut self.command_palette {
                    palette.move_down(&entries);
                }
            }
            KeyCode::Home => {
                self.command_palette = Some(CommandPaletteState::default());
            }
            KeyCode::End => {
                let entries = self.command_palette_entries();
                if let Some(palette) = &mut self.command_palette {
                    palette.select_last(&entries);
                }
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
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
            | KeyCode::Modifier(_)
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::PageUp
            | KeyCode::PageDown => {}
        }
        Ok(())
    }

    pub(super) async fn execute_selected_command_palette_action<S>(
        &mut self,
        config: &Config,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(palette) = &self.command_palette else {
            return Ok(());
        };
        let entries = self.command_palette_entries();
        let Some(entry) = entries.get(palette.selected()) else {
            self.close_command_palette();
            return Ok(());
        };
        if !entry.enabled {
            self.push_status(format!("{}: {}", entry.title, entry.detail));
            return Ok(());
        }
        let Some(action) = palette.selected_action(&entries) else {
            return Ok(());
        };
        self.close_command_palette();
        match action {
            CommandPaletteAction::NewSession => {
                self.start_new_session(config, app_server).await?;
            }
            CommandPaletteAction::CopyTranscript => {
                self.copy_selected_transcript_with(crate::clipboard_copy::copy_to_clipboard);
            }
            CommandPaletteAction::ClearTranscript => {
                self.clear_visible_transcript();
            }
            CommandPaletteAction::SelectLatestTranscript => {
                self.select_latest_transcript_item();
            }
            CommandPaletteAction::ScrollTranscriptTop => {
                self.scroll_transcript_to_top();
            }
            CommandPaletteAction::ScrollTranscriptBottom => {
                self.scroll_transcript_to_bottom();
            }
            CommandPaletteAction::InterruptTurn => {
                self.interrupt_active_turn(app_server).await?;
            }
            CommandPaletteAction::SwitchModel => {
                self.set_dashboard_route(DashboardRoute::Status);
                self.dashboard_scroll.set(0);
                self.session_list.focused = false;
                self.settings.focused = true;
                self.settings.focus_action(SettingsAction::Model);
                self.open_model_selector();
            }
            CommandPaletteAction::ChangePermissions => {
                self.set_dashboard_route(DashboardRoute::Status);
                self.dashboard_scroll.set(0);
                self.session_list.focused = false;
                self.settings.focused = true;
                self.settings.focus_action(SettingsAction::ApprovalPolicy);
                self.open_approval_selector();
            }
            CommandPaletteAction::ResumeThread => {
                self.set_dashboard_route(DashboardRoute::Sessions);
                self.dashboard_scroll.set(0);
                self.settings.focused = false;
                self.session_list.focused = true;
                self.start_session_list_refresh(app_server);
                self.push_status("press r to resume selected session");
            }
            CommandPaletteAction::ForkThread => {
                self.set_dashboard_route(DashboardRoute::Sessions);
                self.dashboard_scroll.set(0);
                self.settings.focused = false;
                self.session_list.focused = true;
                self.start_session_list_refresh(app_server);
                self.push_status("press f to fork selected session");
            }
            CommandPaletteAction::ImportExternalAgentConfig => {
                self.start_external_agent_import_review(app_server).await?;
            }
            CommandPaletteAction::CompactContext => {
                let request = app_server.thread_compact_start_in_background(self.thread_id);
                self.start_backend_action(
                    ActionGroup::Compaction,
                    "starting context compaction",
                    async move {
                        BackendActionResult::Compaction {
                            result: request.await,
                        }
                    },
                );
            }
        }
        Ok(())
    }
}
