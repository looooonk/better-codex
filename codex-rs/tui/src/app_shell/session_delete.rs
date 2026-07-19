use super::ShellState;
use super::backend::AppShellBackend;
use super::design::palette;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadSourceKind;
use codex_protocol::ThreadId;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::style::Stylize;
use ratatui::text::Line;

const DESCENDANT_PAGE_SIZE: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingSessionDelete {
    thread_id: ThreadId,
    title: String,
    descendant_count: usize,
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
                "Delete session: ".fg(palette::TEXT).bold(),
                self.title.clone().fg(palette::WARNING).bold(),
            ]),
            Line::from(format!("Session ID: {}", self.thread_id).fg(palette::MUTED)),
            Line::from(descendant_label.fg(palette::ERROR)),
            Line::from("This permanently deletes all persisted history in that subtree.".bold()),
            "".into(),
            Line::from(vec![
                "> ".fg(palette::FOCUS).bold(),
                "Delete permanently ".fg(palette::ERROR).bold(),
                "Enter/y".fg(palette::MUTED),
            ]),
            Line::from(vec![
                "  ".into(),
                "Cancel ".fg(palette::TEXT),
                "Esc/n".fg(palette::MUTED),
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
    pub(super) async fn start_session_delete_confirmation<S>(
        &mut self,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(thread_id) = self.session_list.selected_thread_id() else {
            self.push_status("no session selected");
            return Ok(());
        };
        if self.session_list.selected_is_current(self.thread_id) {
            self.push_error("cannot delete the active session");
            return Ok(());
        }
        let title = self
            .session_list
            .selected_title()
            .unwrap_or("untitled thread")
            .to_string();
        let descendant_count = count_descendants(app_server, thread_id)
            .await
            .wrap_err("failed to inspect the session subtree before deletion")?;
        self.pending_session_delete = Some(PendingSessionDelete {
            thread_id,
            title,
            descendant_count,
        });
        Ok(())
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
                self.confirm_session_delete(app_server).await?;
            }
            KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                self.pending_session_delete = None;
                self.push_status("session deletion cancelled");
            }
            _ => {}
        }
        Ok(())
    }

    async fn confirm_session_delete<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(pending) = self.pending_session_delete.take() else {
            return Ok(());
        };
        if pending.thread_id == self.thread_id {
            self.push_error("cannot delete the active session");
            return Ok(());
        }
        app_server.thread_delete(pending.thread_id).await?;
        self.invalidate_session_list_refresh();
        self.session_list.remove_thread(pending.thread_id);
        self.push_status(format!("deleted session {}", pending.thread_id));
        Ok(())
    }
}

async fn count_descendants<S>(app_server: &mut S, thread_id: ThreadId) -> Result<usize>
where
    S: AppShellBackend,
{
    let mut count = 0;
    for archived in [false, true] {
        let mut cursor = None;
        loop {
            let response = app_server
                .thread_list(ThreadListParams {
                    cursor,
                    limit: Some(DESCENDANT_PAGE_SIZE),
                    sort_key: None,
                    sort_direction: None,
                    model_providers: None,
                    source_kinds: Some(all_thread_source_kinds()),
                    archived: Some(archived),
                    cwd: None,
                    use_state_db_only: true,
                    search_term: None,
                    parent_thread_id: None,
                    ancestor_thread_id: Some(thread_id.to_string()),
                })
                .await?;
            count += response.data.len();
            let Some(next_cursor) = response.next_cursor else {
                break;
            };
            cursor = Some(next_cursor);
        }
    }
    Ok(count)
}

fn all_thread_source_kinds() -> Vec<ThreadSourceKind> {
    vec![
        ThreadSourceKind::Cli,
        ThreadSourceKind::VsCode,
        ThreadSourceKind::Exec,
        ThreadSourceKind::AppServer,
        ThreadSourceKind::SubAgent,
        ThreadSourceKind::SubAgentReview,
        ThreadSourceKind::SubAgentCompact,
        ThreadSourceKind::SubAgentThreadSpawn,
        ThreadSourceKind::SubAgentOther,
        ThreadSourceKind::Unknown,
    ]
}
