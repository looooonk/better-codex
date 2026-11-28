use super::ShellState;
use super::agent_log::AgentLogTarget;
use super::agent_trace::Message;
use super::agent_trace::Trace;
use super::backend::AppShellBackend;
use crate::legacy_core::config::Config;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::task::JoinHandle;

mod view;
pub(super) use view::render;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Message,
    Lane,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Message(usize),
    Lane(usize),
    EarlierLanes,
    LaterLanes,
}

#[derive(Default)]
struct DetailRows {
    width: u16,
    lines: Vec<crate::terminal_hyperlinks::HyperlinkLine>,
}

struct Detail {
    message: Message,
    route: String,
    links: super::agent_text::AgentTextLinks,
    rows: std::cell::RefCell<DetailRows>,
    cwd: std::path::PathBuf,
    scroll: usize,
    max_scroll: Cell<usize>,
}

pub(super) struct AgentPanel {
    pub(super) trace: Arc<Trace>,
    task: Option<JoinHandle<color_eyre::Result<Trace>>>,
    refresh_at: Instant,
    error: bool,
    follow: bool,
    focus: Focus,
    message: Option<usize>,
    lane: usize,
    first_row: Cell<usize>,
    first_lane: Cell<usize>,
    page: Cell<usize>,
    columns: Cell<usize>,
    detail: Option<Detail>,
}

impl Default for AgentPanel {
    fn default() -> Self {
        Self {
            trace: Arc::default(),
            task: None,
            refresh_at: Instant::now(),
            error: false,
            follow: false,
            focus: Focus::Message,
            message: None,
            lane: 0,
            first_row: Cell::new(0),
            first_lane: Cell::new(0),
            page: Cell::new(1),
            columns: Cell::new(1),
            detail: None,
        }
    }
}

impl Drop for AgentPanel {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

impl AgentPanel {
    fn activate(&mut self, hit: Hit) -> Option<AgentLogTarget> {
        match hit {
            Hit::Message(i) => {
                let message = self.trace.messages.get(i).filter(|msg| msg.readable())?;
                self.follow = false;
                self.message = Some(i);
                self.focus = Focus::Message;
                self.detail = Some(Detail {
                    route: self.route(message),
                    rows: Default::default(),
                    links: Default::default(),
                    cwd: self.trace.lanes[message.from].cwd.clone(),
                    message: message.clone(),
                    scroll: 0,
                    max_scroll: Cell::new(0),
                });
            }
            Hit::Lane(i) => {
                self.lane = i;
                self.focus = Focus::Lane;
                return Some(self.trace.lanes.get(i)?.target.clone());
            }
            Hit::EarlierLanes => self.select_lane(self.first_lane.get().saturating_sub(1)),
            Hit::LaterLanes => self.select_lane(self.first_lane.get() + self.columns.get()),
        }
        None
    }

    pub(super) fn link_at(&self, p: Position) -> Option<crate::terminal_hyperlinks::ClickTarget> {
        self.detail.as_ref()?.links.at(p)
    }

    fn route(&self, msg: &Message) -> String {
        format!(
            "{} → {}",
            self.trace.lanes[msg.from].target.display_name,
            self.trace.lanes[msg.to].target.display_name
        )
    }

    fn select_lane(&mut self, index: usize) {
        self.follow = false;
        self.focus = Focus::Lane;
        self.lane = index.min(self.trace.lanes.len().saturating_sub(1));
        reveal(&self.first_lane, self.lane, self.columns.get());
    }

    fn navigate(&mut self, key: KeyCode) {
        let page = self.page.get().max(1);
        if let Some(detail) = &mut self.detail {
            detail.scroll = next_index(key, detail.scroll, detail.max_scroll.get(), page * 3);
            return;
        }
        match key {
            KeyCode::Left | KeyCode::Char('h') => self.select_lane(self.lane.saturating_sub(1)),
            KeyCode::Right | KeyCode::Char('l') => self.select_lane(self.lane.saturating_add(1)),
            KeyCode::Tab => {
                if self.focus == Focus::Message {
                    self.select_lane(self.lane);
                } else {
                    self.focus = Focus::Message;
                }
            }
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageDown
            | KeyCode::PageUp
            | KeyCode::Char('g' | 'G' | 'j' | 'k') => {
                self.navigate_messages(key, page);
            }
            _ => {}
        }
    }
    fn navigate_messages(&mut self, key: KeyCode, page: usize) {
        self.focus = Focus::Message;
        self.follow = matches!(key, KeyCode::End | KeyCode::Char('G'));
        let messages = &self.trace.messages;
        let first = self.first_row.get();
        let max_first = messages.len().saturating_sub(page);
        let mut row = next_index(key, first, max_first, page);
        let selected = match key {
            KeyCode::Up | KeyCode::Char('k') => {
                let start = self.message.unwrap_or(first).min(messages.len());
                let found = messages[..start].iter().rposition(Message::readable);
                if found.is_none() && self.message.is_some() {
                    row = 0;
                }
                found
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let start = self.message.map_or(first, |i| i + 1).min(messages.len());
                let found = messages[start..]
                    .iter()
                    .position(Message::readable)
                    .map(|i| start + i);
                if found.is_none() && self.message.is_some() {
                    row = max_first;
                }
                found
            }
            _ => {
                let visible = &messages[row..(row + page).min(messages.len())];
                let found = if self.follow {
                    visible.iter().rposition(Message::readable)
                } else {
                    visible.iter().position(Message::readable)
                };
                found.map(|i| row + i)
            }
        };
        self.message = selected.or(self.message.filter(|i| (row..row + page).contains(i)));
        if let Some(index) = self.message {
            let step = matches!(key, KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k'));
            if !step || selected.is_none() {
                self.first_row.set(row);
            }
            reveal(&self.first_row, index, page);
            let msg = &messages[index];
            let first = self.first_lane.get();
            let last = first + self.columns.get();
            if msg.from.min(msg.to) >= last || msg.from.max(msg.to) < first {
                reveal(&self.first_lane, msg.from, self.columns.get());
            }
        } else {
            self.first_row.set(row);
        }
    }
}

fn reveal(offset: &Cell<usize>, index: usize, count: usize) {
    offset.set(
        offset
            .get()
            .clamp(index.saturating_sub(count.saturating_sub(1)), index),
    );
}

fn next_index(key: KeyCode, index: usize, max: usize, page: usize) -> usize {
    match key {
        KeyCode::Up | KeyCode::Char('k') => index.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => index.saturating_add(1),
        KeyCode::PageUp => index.saturating_sub(page),
        KeyCode::PageDown => index.saturating_add(page),
        KeyCode::Home | KeyCode::Char('g') => 0,
        KeyCode::End | KeyCode::Char('G') => max,
        _ => index,
    }
    .min(max)
}

impl ShellState {
    pub(super) fn open_agent_panel(&mut self, backend: &impl AppShellBackend) {
        self.close_agent_log();
        self.clear_transcript_selection();
        let mut panel = AgentPanel::default();
        panel.task = Some(tokio::spawn(
            backend.agent_trace_in_background(self.thread_id, panel.trace.clone()),
        ));
        self.agent_panel = Some(panel);
    }

    pub(super) async fn poll_agent_panel(&mut self, backend: &impl AppShellBackend) -> bool {
        let Some(panel) = &mut self.agent_panel else {
            return false;
        };
        if panel.task.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(task) = panel.task.take()
        {
            match task.await {
                Ok(Ok(trace)) => {
                    for lane in trace.lanes.iter().skip(1) {
                        self.agent_activity.assign_color(&lane.target.thread_id);
                    }
                    let first_load = panel.trace.lanes.is_empty();
                    panel.trace = Arc::new(trace);
                    if panel.follow {
                        panel.navigate(KeyCode::End);
                    } else {
                        panel.message = panel.message.or(first_load.then_some(0)).filter(|i| {
                            panel.trace.messages.get(*i).is_some_and(Message::readable)
                        });
                    }
                    panel.lane = panel.lane.min(panel.trace.lanes.len().saturating_sub(1));
                    panel.error = false;
                }
                _ => panel.error = true,
            }
            let busy = self.active_turn_id.is_some()
                || panel
                    .trace
                    .lanes
                    .iter()
                    .any(super::agent_trace::Lane::running);
            panel.refresh_at = Instant::now() + Duration::from_secs(if busy { 2 } else { 10 });
            return true;
        }
        if panel.task.is_none() && Instant::now() >= panel.refresh_at {
            panel.task = Some(tokio::spawn(
                backend.agent_trace_in_background(self.thread_id, panel.trace.clone()),
            ));
        }
        false
    }

    pub(super) fn agent_panel_key(
        &mut self,
        key: KeyEvent,
        config: &Config,
        backend: &impl AppShellBackend,
    ) {
        if key.kind == KeyEventKind::Release
            || !matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            return;
        }
        let Some(panel) = &mut self.agent_panel else {
            return;
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                if panel.detail.take().is_none() {
                    self.agent_panel = None;
                }
            }
            KeyCode::Enter if panel.detail.is_none() => {
                let hit = if panel.focus == Focus::Lane || panel.trace.messages.is_empty() {
                    Hit::Lane(panel.lane)
                } else {
                    let Some(index) = panel.message else {
                        return;
                    };
                    Hit::Message(index)
                };
                if let Some(target) = panel.activate(hit) {
                    self.open_agent_log(target, config, backend);
                }
            }
            KeyCode::Char('r') if panel.task.is_none() => {
                panel.task = Some(tokio::spawn(
                    backend.agent_trace_in_background(self.thread_id, panel.trace.clone()),
                ));
            }
            key => panel.navigate(key),
        }
    }

    pub(super) fn agent_panel_click(
        &mut self,
        screen: Rect,
        position: Position,
        config: &Config,
        backend: &impl AppShellBackend,
    ) {
        let Some(panel) = &mut self.agent_panel else {
            return;
        };
        let layout = view::Layout::new(panel, screen);
        if !layout.geometry.modal.contains(position) {
            if panel.detail.take().is_none() {
                self.agent_panel = None;
            }
        } else if panel.detail.is_none()
            && let Some(hit) = layout.hit(panel, position)
            && let Some(target) = panel.activate(hit)
        {
            self.open_agent_log(target, config, backend);
        }
    }

    pub(super) fn scroll_agent_panel(&mut self, key: KeyCode) {
        if let Some(panel) = &mut self.agent_panel {
            if panel.detail.is_some() {
                panel.navigate(key);
                return;
            }
            let page = panel.page.get().max(1);
            let first = panel.first_row.get();
            let next = next_index(
                key,
                first,
                panel.trace.messages.len().saturating_sub(page),
                1,
            );
            panel.first_row.set(next);
            panel.message = panel.message.filter(|i| (next..next + page).contains(i));
            panel.focus = Focus::Message;
            panel.follow = false;
        }
    }
}

#[cfg(test)]
mod tests;
