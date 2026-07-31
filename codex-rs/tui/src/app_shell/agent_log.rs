use super::ShellState;
use super::agent_activity::AgentActivity;
use super::agent_activity::AgentLifecycleStatus;
use super::agent_log_format::thread_to_agent_log_lines;
use super::backend::AppShellBackend;
use crate::legacy_core::config::Config;
use crate::session_transcript::RawReasoningVisibility;
use crate::session_transcript::TranscriptLines;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use codex_app_server_protocol::Thread;
use codex_config::types::TuiAppTheme;
use codex_protocol::ThreadId;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use std::cell::Cell;
use std::cell::RefCell;
use tokio::task::JoinHandle;

const AGENT_LOG_PAGE_STEP: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentLogTarget {
    pub(super) thread_id: String,
    pub(super) display_name: String,
    pub(super) path: String,
    pub(super) task_summary: Option<String>,
    pub(super) status: AgentLifecycleStatus,
}

impl From<&AgentActivity> for AgentLogTarget {
    fn from(agent: &AgentActivity) -> Self {
        Self {
            thread_id: agent.thread_id.clone(),
            display_name: agent.display_name().to_string(),
            path: agent
                .path
                .as_ref()
                .map_or_else(|| agent.thread_id.clone(), ToString::to_string),
            task_summary: agent.task_summary.clone(),
            status: agent.status,
        }
    }
}

pub(super) struct AgentLogState {
    pub(super) target: AgentLogTarget,
    load_task: Option<JoinHandle<Result<Thread>>>,
    lines: TranscriptLines,
    error: Option<String>,
    raw_reasoning_visibility: RawReasoningVisibility,
    wrapped_cache: RefCell<Option<WrappedLogCache>>,
    scroll: Cell<usize>,
    scroll_max: Cell<usize>,
}

struct WrappedLogCache {
    width: usize,
    lines: TranscriptLines,
}

pub(super) struct AgentLogViewport {
    pub(super) lines: TranscriptLines,
    pub(super) visual_lines: usize,
    pub(super) scroll: usize,
}

impl AgentLogState {
    fn loading(
        target: AgentLogTarget,
        load_task: JoinHandle<Result<Thread>>,
        raw_reasoning_visibility: RawReasoningVisibility,
    ) -> Self {
        Self {
            target,
            load_task: Some(load_task),
            lines: Vec::new(),
            error: None,
            raw_reasoning_visibility,
            wrapped_cache: RefCell::new(None),
            scroll: Cell::new(0),
            scroll_max: Cell::new(0),
        }
    }

    fn failed(target: AgentLogTarget, message: impl Into<String>) -> Self {
        Self {
            target,
            load_task: None,
            lines: Vec::new(),
            error: Some(message.into()),
            raw_reasoning_visibility: RawReasoningVisibility::Hidden,
            wrapped_cache: RefCell::new(None),
            scroll: Cell::new(0),
            scroll_max: Cell::new(0),
        }
    }

    pub(super) fn is_loading(&self) -> bool {
        self.load_task.is_some()
    }

    pub(super) fn lines(&self) -> &[ratatui::text::Line<'static>] {
        &self.lines
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(super) fn scroll(&self) -> usize {
        self.scroll.get().min(self.scroll_max.get())
    }

    pub(super) fn set_scroll_max(&self, scroll_max: usize) {
        self.scroll_max.set(scroll_max);
        self.scroll.set(self.scroll.get().min(scroll_max));
    }

    pub(super) fn ready_viewport(&self, width: usize, height: usize) -> AgentLogViewport {
        let width = width.max(1);
        let needs_wrap = self
            .wrapped_cache
            .borrow()
            .as_ref()
            .is_none_or(|cache| cache.width != width);
        if needs_wrap {
            let lines = word_wrap_lines(self.lines.clone(), RtOptions::new(width));
            *self.wrapped_cache.borrow_mut() = Some(WrappedLogCache { width, lines });
        }
        let cache = self.wrapped_cache.borrow();
        let lines = cache
            .as_ref()
            .map_or(&[][..], |cache| cache.lines.as_slice());
        let visual_lines = lines.len();
        self.set_scroll_max(visual_lines.saturating_sub(height));
        let scroll = self.scroll();
        AgentLogViewport {
            lines: lines.iter().skip(scroll).take(height).cloned().collect(),
            visual_lines,
            scroll,
        }
    }

    fn scroll_up(&self, amount: usize) {
        self.scroll.set(self.scroll().saturating_sub(amount));
    }

    fn scroll_down(&self, amount: usize) {
        self.scroll.set(
            self.scroll()
                .saturating_add(amount)
                .min(self.scroll_max.get()),
        );
    }

    fn scroll_to_top(&self) {
        self.scroll.set(0);
    }

    fn scroll_to_bottom(&self) {
        self.scroll.set(self.scroll_max.get());
    }

    fn cancel(&mut self) {
        if let Some(task) = self.load_task.take() {
            task.abort();
        }
    }

    async fn poll(&mut self, app_theme: TuiAppTheme) -> bool {
        let ready = self
            .load_task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished);
        if !ready {
            return false;
        }
        let Some(task) = self.load_task.take() else {
            return false;
        };
        match task.await {
            Ok(Ok(thread)) => {
                let _active_theme = crate::app_theme::activate(app_theme);
                match thread_to_agent_log_lines(&thread, self.raw_reasoning_visibility) {
                    Ok(lines) => {
                        self.lines = lines;
                        self.error = None;
                    }
                    Err(message) => {
                        self.lines.clear();
                        self.error = Some(message);
                    }
                }
                self.wrapped_cache.borrow_mut().take();
            }
            Ok(Err(err)) => {
                self.error = Some(format!("Could not load the agent log: {err}"));
            }
            Err(err) if err.is_cancelled() => return false,
            Err(err) => {
                self.error = Some(format!("Agent log task failed: {err}"));
            }
        }
        true
    }
}

impl ShellState {
    pub(super) fn open_selected_agent_log<S>(&mut self, config: &Config, app_server: &S)
    where
        S: AppShellBackend,
    {
        let Some(target) = self.agent_activity.selected().map(AgentLogTarget::from) else {
            return;
        };
        self.open_agent_log(target, config, app_server);
    }

    pub(super) fn reload_agent_log<S>(&mut self, config: &Config, app_server: &S)
    where
        S: AppShellBackend,
    {
        let Some(open_target) = self.agent_log.as_ref().map(|log| log.target.clone()) else {
            return;
        };
        let target = self
            .agent_activity
            .agent(&open_target.thread_id)
            .map(AgentLogTarget::from)
            .unwrap_or(open_target);
        self.open_agent_log(target, config, app_server);
    }

    fn open_agent_log<S>(&mut self, target: AgentLogTarget, config: &Config, app_server: &S)
    where
        S: AppShellBackend,
    {
        self.close_agent_log();
        self.close_tool_output();
        self.close_diff_view();
        self.command_palette = None;
        self.selector = None;
        self.clear_transcript_selection();

        let thread_id = match ThreadId::from_string(&target.thread_id) {
            Ok(thread_id) => thread_id,
            Err(err) => {
                self.agent_log = Some(AgentLogState::failed(
                    target,
                    format!("Invalid agent thread id: {err}"),
                ));
                return;
            }
        };
        let visibility = if config.show_raw_agent_reasoning {
            RawReasoningVisibility::Visible
        } else {
            RawReasoningVisibility::Hidden
        };
        let load_task = tokio::spawn(app_server.thread_read_full_in_background(thread_id));
        self.agent_log = Some(AgentLogState::loading(target, load_task, visibility));
    }

    pub(super) fn close_agent_log(&mut self) {
        if let Some(mut log) = self.agent_log.take() {
            log.cancel();
        }
    }

    pub(super) fn has_pending_agent_log(&self) -> bool {
        self.agent_log
            .as_ref()
            .is_some_and(AgentLogState::is_loading)
    }

    pub(super) async fn poll_agent_log(&mut self) -> bool {
        let app_theme = self.app_theme;
        let Some(log) = &mut self.agent_log else {
            return false;
        };
        log.poll(app_theme).await
    }

    pub(super) fn handle_agent_log_key(&mut self, key: KeyEvent) -> bool {
        if self.agent_log.is_none()
            || !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            || !matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            return false;
        }
        if matches!(key.code, KeyCode::Esc) {
            self.close_agent_log();
            return true;
        }
        let Some(log) = &self.agent_log else {
            return false;
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => log.scroll_up(/*amount*/ 1),
            KeyCode::Down | KeyCode::Char('j') => log.scroll_down(/*amount*/ 1),
            KeyCode::PageUp => log.scroll_up(AGENT_LOG_PAGE_STEP),
            KeyCode::PageDown => log.scroll_down(AGENT_LOG_PAGE_STEP),
            KeyCode::Home | KeyCode::Char('g') => log.scroll_to_top(),
            KeyCode::End | KeyCode::Char('G') => log.scroll_to_bottom(),
            _ => return false,
        }
        true
    }

    pub(super) fn scroll_agent_log_up(&self) {
        if let Some(log) = &self.agent_log {
            log.scroll_up(/*amount*/ 3);
        }
    }

    pub(super) fn scroll_agent_log_down(&self) {
        if let Some(log) = &self.agent_log {
            log.scroll_down(/*amount*/ 3);
        }
    }
}

#[cfg(test)]
#[path = "agent_log_tests.rs"]
mod tests;
