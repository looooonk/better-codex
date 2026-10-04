use super::modal_view;
use crate::daemon_startup::CompatibilityError;
use crate::legacy_core::config::Config;
use crate::tui::TuiEvent;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::io;
use std::io::IsTerminal;
use tokio_stream::StreamExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DaemonRecovery {
    Independent,
    Restart,
    Cancel,
}

struct RecoveryState {
    selected: usize,
    restart_available: bool,
}

impl RecoveryState {
    fn key(&mut self, key: KeyEvent) -> Option<DaemonRecovery> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'd'))
        {
            return Some(DaemonRecovery::Cancel);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('3') => Some(DaemonRecovery::Cancel),
            KeyCode::Char('1') => Some(DaemonRecovery::Independent),
            KeyCode::Char('2') if self.restart_available => {
                self.selected = 1;
                None
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                None
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(2);
                None
            }
            KeyCode::Enter => match self.selected {
                0 => Some(DaemonRecovery::Independent),
                1 if self.restart_available => Some(DaemonRecovery::Restart),
                1 => None,
                _ => Some(DaemonRecovery::Cancel),
            },
            _ => None,
        }
    }

    fn lines(&self, issue: &CompatibilityError) -> Vec<Line<'static>> {
        let mut lines = vec![
            "Background server has incompatible feature settings"
                .bold()
                .into(),
            issue.reason.clone().into(),
            "".into(),
        ];
        if let Some(features) = &issue.restart_features {
            lines.push("Restart will use these shared feature settings:".into());
            lines.extend(
                features
                    .iter()
                    .map(|(name, enabled)| format!("  {name} = {enabled}").dim().into()),
            );
            lines.push("These settings persist for other clients. Restart may interrupt active or queued work.".into());
            lines.push("".into());
        }
        for (index, label) in [
            "Run without daemon this time",
            "Restart with these settings",
            "Cancel",
        ]
        .into_iter()
        .enumerate()
        {
            let selected = if self.selected == index { ">" } else { " " };
            let suffix = if index == 1 && !self.restart_available {
                " (unavailable for this server)"
            } else {
                ""
            };
            lines.push(format!("{selected} {}. {label}{suffix}", index + 1).into());
        }
        lines.push("".into());
        lines.push(
            "Select 2, then press Enter to confirm a shared restart. Esc cancels."
                .dim()
                .into(),
        );
        lines
    }
}

pub(crate) async fn recover_daemon(
    config: &Config,
    issue: &CompatibilityError,
    managed: bool,
) -> io::Result<DaemonRecovery> {
    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        return Ok(DaemonRecovery::Cancel);
    }
    let initialized = crate::tui::init()?;
    let mut guard = crate::TerminalRestoreGuard::new();
    let mut tui = crate::tui::Tui::new(
        initialized.terminal,
        initialized.enhanced_keys_supported,
        initialized.stderr_guard,
    );
    tui.enter_alt_screen()?;
    let mut state = RecoveryState {
        selected: 2,
        restart_available: managed && issue.restart_features.is_some(),
    };
    let mut events = tui.event_stream();
    let result = loop {
        let height = tui.terminal.size()?.height;
        tui.draw(height, |frame| {
            let _theme = crate::app_theme::activate(crate::app_theme::configured(config));
            modal_view::render_modal(
                frame.area(),
                "Background server",
                state.lines(issue),
                frame.buffer,
            );
        })?;
        match events.next().await {
            Some(TuiEvent::Key(key)) => {
                if let Some(selection) = state.key(key) {
                    break selection;
                }
            }
            Some(_) => {}
            None => break DaemonRecovery::Cancel,
        }
    };
    guard.restore().map_err(io::Error::other)?;
    Ok(result)
}

#[cfg(test)]
#[path = "daemon_recovery_tests.rs"]
mod tests;
