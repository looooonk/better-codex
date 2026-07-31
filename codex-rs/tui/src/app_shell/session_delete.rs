use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use super::design::palette;
use codex_protocol::ThreadId;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::style::Stylize;
use ratatui::text::Line;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingSessionDelete {
    pub(super) thread_id: ThreadId,
    pub(super) title: String,
    pub(super) descendant_count: usize,
}

impl PendingSessionDelete {
    pub(super) fn lines(&self) -> Vec<Line<'static>> {
        let descendant_label = match self.descendant_count {
            0 => "No spawned descendants will be deleted.".to_string(),
            1 => "1 spawned descendant will also be deleted.".to_string(),
            count => format!("{count} spawned descendants will also be deleted."),
        };
        vec![
            Line::from(vec![
                "Delete session: ".fg(palette::text()).bold(),
                self.title.clone().fg(palette::warning()).bold(),
            ]),
            Line::from(format!("Session ID: {}", self.thread_id).fg(palette::muted())),
            Line::from(descendant_label.fg(palette::error())),
            Line::from("This permanently deletes all persisted history in that subtree.".bold()),
            "".into(),
            Line::from(vec![
                "> ".fg(palette::focus()).bold(),
                "Delete permanently ".fg(palette::error()).bold(),
                "Enter/y".fg(palette::muted()),
            ]),
            Line::from(vec![
                "  ".into(),
                "Cancel ".fg(palette::text()),
                "Esc/n".fg(palette::muted()),
            ]),
        ]
    }

    pub(super) fn click_key_at(&self, line: usize) -> Option<KeyCode> {
        match line {
            5 => Some(KeyCode::Enter),
            6 => Some(KeyCode::Esc),
            _ => None,
        }
    }
}

impl ShellState {
    pub(super) fn start_session_delete_confirmation<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        let Some(thread_id) = self.session_list.selected_thread_id() else {
            self.push_status("no session selected");
            return;
        };
        if self.session_list.selected_is_current(self.thread_id) {
            self.push_error("cannot delete the active session");
            return;
        }
        let title = self
            .session_list
            .selected_title()
            .unwrap_or("untitled thread")
            .to_string();
        let request = app_server.thread_descendant_count_in_background(thread_id);
        self.start_backend_action(
            ActionGroup::SessionDelete,
            "inspecting session",
            async move {
                BackendActionResult::DescendantCount {
                    thread_id,
                    title,
                    result: request.await,
                }
            },
        );
    }

    pub(super) async fn handle_session_delete_key<S>(
        &mut self,
        key: KeyEvent,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
            return Ok(());
        }
        match key.code {
            KeyCode::Enter | KeyCode::Char('y' | 'Y') => {
                self.confirm_session_delete(app_server);
            }
            KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                self.pending_session_delete = None;
                self.push_status("session deletion cancelled");
            }
            _ => {}
        }
        Ok(())
    }

    fn confirm_session_delete<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        let Some(pending) = self.pending_session_delete.as_ref() else {
            return;
        };
        if pending.thread_id == self.thread_id {
            self.push_error("cannot delete the active session");
            return;
        }
        let thread_id = pending.thread_id;
        let request = app_server.thread_delete_in_background(thread_id);
        self.start_backend_action(ActionGroup::SessionDelete, "deleting session", async move {
            BackendActionResult::SessionDelete {
                thread_id,
                result: request.await,
            }
        });
    }

    pub(super) fn complete_session_delete_inspection(
        &mut self,
        thread_id: ThreadId,
        title: String,
        result: Result<usize>,
    ) {
        match result {
            Ok(descendant_count) => {
                self.pending_session_delete = Some(PendingSessionDelete {
                    thread_id,
                    title,
                    descendant_count,
                });
                self.status = "confirm deletion".to_string();
            }
            Err(err) => self
                .report_action_error("failed to inspect the session subtree before deletion", err),
        }
    }

    pub(super) fn complete_session_delete(&mut self, thread_id: ThreadId, result: Result<()>) {
        match result {
            Ok(()) => {
                self.pending_session_delete = None;
                self.invalidate_session_list_refresh();
                self.session_list.remove_thread(thread_id);
                self.push_status(format!("deleted session {thread_id}"));
            }
            Err(err) => self.report_action_error("failed to delete session", err),
        }
    }
}
