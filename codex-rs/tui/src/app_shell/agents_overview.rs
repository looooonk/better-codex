use super::backend::app_shell_request_id;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use crate::app_server_session::AppServerSession;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::resume_picker::SessionSelection;
use crate::resume_picker::SessionTarget;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadLoadedListParams;
use codex_app_server_protocol::ThreadLoadedListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadStatus;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use futures::StreamExt;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

struct AgentRow {
    target: SessionTarget,
    title: String,
    cwd: String,
    model: Option<String>,
    reasoning_effort: Option<ReasoningEffort>,
    preview: String,
    status: &'static str,
    updated_at: i64,
    can_accept_input: bool,
}

#[derive(Default)]
struct Overview {
    rows: Vec<AgentRow>,
    selected: usize,
    notice: Option<String>,
}

impl Overview {
    fn replace(&mut self, rows: Vec<AgentRow>) {
        let selected_id = self.rows.get(self.selected).map(|row| row.target.thread_id);
        self.selected = rows
            .iter()
            .position(|row| Some(row.target.thread_id) == selected_id)
            .unwrap_or_else(|| self.selected.min(rows.len().saturating_sub(1)));
        self.rows = rows;
    }

    fn render(&self, area: Rect, buffer: &mut Buffer) {
        fill_rect(buffer, area, palette::base());
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .areas(area);
        fill_rect(buffer, header, palette::dark());
        Paragraph::new(Line::from(vec![
            "Better Codex".fg(palette::purple()).bold(),
            "   Agents overview".into(),
        ]))
        .render(pane_content_rect(header), buffer);
        let body = pane_content_rect(body);
        let (body, details) = if body.width >= 76 {
            let [body, _, details] = Layout::horizontal([
                Constraint::Percentage(55),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .areas(body);
            (body, details)
        } else {
            let [body, _, details] = Layout::vertical([
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Length(5),
            ])
            .areas(body);
            (body, details)
        };
        let visible = usize::from(body.height / 3).max(1);
        let start = self.selected.saturating_sub(visible.saturating_sub(1));
        let mut lines = Vec::new();
        for (index, row) in self.rows.iter().enumerate().skip(start).take(visible) {
            let marker = if index == self.selected { "> " } else { "  " };
            let title = Line::from(vec![
                marker.fg(palette::focus()),
                row.title.clone().bold(),
                format!("  [{}]", row.status).fg(palette::muted()),
            ]);
            let title = if index == self.selected {
                title.style(pane_style(palette::elevated()))
            } else {
                title
            };
            lines.push(truncate_line_with_ellipsis_if_overflow(
                title,
                usize::from(body.width),
            ));
            lines.push(truncate_line_with_ellipsis_if_overflow(
                Line::from(format!("  {}", row.cwd).dim()),
                usize::from(body.width),
            ));
            lines.push(Line::from(""));
        }
        if self.rows.is_empty() {
            lines.push(Line::from(
                "No loaded agents. Press n to start a session.".dim(),
            ));
        }
        Paragraph::new(lines).render(body, buffer);
        if let Some(row) = self.rows.get(self.selected) {
            let width = usize::from(details.width);
            let lines = [
                ("Task details", row.title.as_str()),
                ("Model", row.model.as_deref().unwrap_or("Unknown")),
                (
                    "Reasoning",
                    row.reasoning_effort
                        .as_ref()
                        .map_or("Unknown", ReasoningEffort::as_str),
                ),
                ("Project", row.cwd.as_str()),
                ("Prompt", row.preview.as_str()),
            ]
            .map(|(label, value)| {
                let value = value.chars().take(512).collect::<String>();
                let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
                truncate_line_with_ellipsis_if_overflow(
                    Line::from(vec![format!("{label}: ").fg(palette::cyan()), value.into()]),
                    width,
                )
            });
            Paragraph::new(lines.to_vec()).render(details, buffer);
        }
        fill_rect(buffer, footer, palette::dark());
        let footer_text = self
            .notice
            .as_deref()
            .unwrap_or("Up/Down select   Enter open   n new   r refresh   Esc exit");
        Paragraph::new(footer_text)
            .style(pane_style(palette::dark()))
            .render(pane_content_rect(footer), buffer);
    }
}

pub(crate) async fn run_agents_overview(
    tui: &mut Tui,
    app_server: &mut AppServerSession,
) -> Result<Option<SessionSelection>> {
    tui.enter_alt_screen()?;
    let mut state = Overview::default();
    let mut events = tui.event_stream();
    let mut refresh = tokio::time::interval(Duration::from_secs(5));
    type Refresh = Pin<Box<dyn Future<Output = Result<(Vec<AgentRow>, Option<String>)>> + Send>>;
    let mut loading: Option<Refresh> = None;
    tui.frame_requester().schedule_frame();
    let mut deferred = Vec::new();
    let selection = loop {
        tokio::select! {
            _ = refresh.tick(), if loading.is_none() => {
                let client = app_server.request_handle();
                loading = Some(Box::pin(async move {
                    tokio::time::timeout(Duration::from_secs(30), load_agents(client)).await?
                }));
            }
            result = async { loading.as_mut().expect("refresh is active").await }, if loading.is_some() => {
                loading = None;
                match result {
                    Ok((rows, notice)) => { state.replace(rows); state.notice = notice; }
                    Err(error) => state.notice = Some(format!("Could not refresh agents: {error}. Press r to retry.")),
                }
                tui.frame_requester().schedule_frame();
            }
            event = app_server.next_event() => {
                match event {
                    None => color_eyre::eyre::bail!("app-server disconnected"),
                    Some(AppServerEvent::Disconnected { message }) => color_eyre::eyre::bail!(message),
                    Some(event @ AppServerEvent::ServerRequest(_)) => {
                        deferred.push(event);
                        if deferred.len() >= 256 {
                            color_eyre::eyre::bail!("too many pending server requests; reopen the session directly");
                        }
                    }
                    Some(AppServerEvent::ServerNotification(notification)) => {
                        if let ServerNotification::ServerRequestResolved(resolved) = notification.as_ref() {
                            deferred.retain(|event| match event {
                                AppServerEvent::ServerRequest(request) => request.id() != &resolved.request_id,
                                _ => true,
                            });
                        }
                        if matches!(*notification, ServerNotification::ThreadStatusChanged(_)
                            | ServerNotification::ThreadStarted(_) | ServerNotification::ThreadClosed(_)
                            | ServerNotification::ThreadDeleted(_)) {
                            refresh.reset_immediately();
                        }
                    }
                    Some(AppServerEvent::Lagged { .. }) => refresh.reset_immediately(),
                }
            }
            event = events.next() => {
                match event {
                    None => break None,
                    Some(TuiEvent::Key(key)) if key.kind == KeyEventKind::Press => {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => break None,
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break None,
                            KeyCode::Char('n') => break Some(SessionSelection::StartFresh),
                            KeyCode::Enter => if let Some(row) = state.rows.get(state.selected) {
                                if row.can_accept_input {
                                    break Some(SessionSelection::Resume(row.target.clone()));
                                }
                                state.notice = Some("This agent cannot accept direct input. Open its parent session to inspect it.".to_string());
                            },
                            KeyCode::Up | KeyCode::Char('k') => state.selected = state.selected.saturating_sub(1),
                            KeyCode::Down | KeyCode::Char('j') => state.selected = (state.selected + 1).min(state.rows.len().saturating_sub(1)),
                            KeyCode::Char('r') => refresh.reset_immediately(),
                            _ => {}
                        }
                        tui.frame_requester().schedule_frame();
                    }
                    Some(TuiEvent::Draw | TuiEvent::Resize) => {
                        let height = tui.terminal.size()?.height;
                        tui.draw(height, |frame| state.render(frame.area(), frame.buffer))?;
                    }
                    Some(_) => {}
                }
            }
        }
    };
    app_server.prepend_events(deferred);
    Ok(selection)
}

/// Supplies bounded loaded-thread pages and metadata for the overview.
trait AgentsReader: Clone {
    fn loaded(
        &self,
        params: ThreadLoadedListParams,
    ) -> impl Future<Output = Result<ThreadLoadedListResponse, TypedRequestError>> + Send;
    fn thread(
        &self,
        thread_id: String,
    ) -> impl Future<Output = Result<ThreadReadResponse, TypedRequestError>> + Send;
}

impl AgentsReader for AppServerRequestHandle {
    fn loaded(
        &self,
        params: ThreadLoadedListParams,
    ) -> impl Future<Output = Result<ThreadLoadedListResponse, TypedRequestError>> + Send {
        self.request_typed(ClientRequest::ThreadLoadedList {
            request_id: app_shell_request_id("agents-overview"),
            params,
        })
    }

    fn thread(
        &self,
        thread_id: String,
    ) -> impl Future<Output = Result<ThreadReadResponse, TypedRequestError>> + Send {
        self.request_typed(ClientRequest::ThreadRead {
            request_id: app_shell_request_id("agents-overview"),
            params: ThreadReadParams {
                thread_id,
                include_turns: false,
            },
        })
    }
}

async fn load_agents(client: impl AgentsReader) -> Result<(Vec<AgentRow>, Option<String>)> {
    let mut cursor = None;
    let mut cursors = HashSet::new();
    let mut ids = Vec::new();
    for _ in 0..10 {
        let response = client
            .loaded(ThreadLoadedListParams {
                cursor,
                limit: Some(100),
            })
            .await?;
        if response.data.len() > 100 {
            color_eyre::eyre::bail!("agent list page exceeded its requested size");
        }
        ids.extend(response.data);
        cursor = response.next_cursor;
        let Some(next) = &cursor else {
            break;
        };
        if !cursors.insert(next.clone()) {
            color_eyre::eyre::bail!("server returned a repeated agent list cursor");
        }
    }
    ids.sort();
    ids.dedup();
    let mut reads = futures::stream::iter(ids.into_iter().map(|thread_id| {
        let client = client.clone();
        async move {
            let response = client.thread(thread_id).await?;
            let thread = response.thread;
            let status = match thread.status {
                ThreadStatus::Active { active_flags } if !active_flags.is_empty() => "needs input",
                ThreadStatus::Active { .. } => "working",
                ThreadStatus::Idle => "idle",
                ThreadStatus::NotLoaded => "unloaded",
                ThreadStatus::SystemError => "error",
            };
            Ok::<_, color_eyre::Report>(AgentRow {
                target: SessionTarget {
                    thread_id: ThreadId::from_string(&thread.id)?,
                    path: thread.path,
                },
                title: thread
                    .name
                    .or(thread.agent_nickname)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| {
                        if thread.preview.is_empty() {
                            thread.id
                        } else {
                            thread.preview.clone()
                        }
                    }),
                model: thread.model,
                reasoning_effort: thread.reasoning_effort,
                preview: thread.preview,
                cwd: thread.cwd.display().to_string(),
                status,
                updated_at: thread.updated_at,
                can_accept_input: thread.can_accept_direct_input != Some(false),
            })
        }
    }))
    .buffer_unordered(4);
    let mut rows = Vec::new();
    let mut unavailable = 0;
    while let Some(result) = reads.next().await {
        match result {
            Ok(row) => rows.push(row),
            Err(_) => unavailable += 1,
        }
    }
    rows.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.title.cmp(&b.title))
    });
    let notice = if unavailable > 0 {
        Some(format!(
            "{unavailable} agents became unavailable. Press r to refresh."
        ))
    } else if cursor.is_some() {
        Some("Showing the first 1,000 loaded agents.".to_string())
    } else {
        None
    };
    Ok((rows, notice))
}

#[cfg(test)]
#[path = "agents_overview_tests.rs"]
mod tests;
