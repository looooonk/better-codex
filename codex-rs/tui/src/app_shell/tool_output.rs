use super::ShellState;
use super::ToolBlockStatus;
use super::TranscriptKind;
use crate::session_transcript::TranscriptLines;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use std::cell::Cell;
use std::cell::RefCell;

const TOOL_OUTPUT_PAGE_STEP: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ToolOutputTarget {
    pub(super) item_id: String,
    pub(super) title: String,
    pub(super) status: ToolBlockStatus,
}

pub(super) struct ToolOutputState {
    pub(super) target: ToolOutputTarget,
    output: String,
    wrapped_cache: RefCell<Option<WrappedOutputCache>>,
    scroll: Cell<usize>,
    scroll_max: Cell<usize>,
    follow_tail: Cell<bool>,
}

struct WrappedOutputCache {
    width: usize,
    lines: TranscriptLines,
}

pub(super) struct ToolOutputViewport {
    pub(super) lines: TranscriptLines,
    pub(super) visual_lines: usize,
    pub(super) scroll: usize,
}

impl ToolOutputState {
    fn new(target: ToolOutputTarget, output: String) -> Self {
        Self {
            target,
            output,
            wrapped_cache: RefCell::new(None),
            scroll: Cell::new(0),
            scroll_max: Cell::new(0),
            follow_tail: Cell::new(true),
        }
    }

    pub(super) fn output(&self) -> &str {
        &self.output
    }

    pub(super) fn scroll(&self) -> usize {
        self.scroll.get().min(self.scroll_max.get())
    }

    pub(super) fn ready_viewport(&self, width: usize, height: usize) -> ToolOutputViewport {
        let width = width.max(1);
        let needs_wrap = self
            .wrapped_cache
            .borrow()
            .as_ref()
            .is_none_or(|cache| cache.width != width);
        if needs_wrap {
            let normalized = self.output.replace('\r', "\n");
            let mut lines = codex_ansi_escape::ansi_escape(&normalized).lines;
            if lines.is_empty() {
                lines.push(Default::default());
            }
            let lines = word_wrap_lines(lines, RtOptions::new(width));
            *self.wrapped_cache.borrow_mut() = Some(WrappedOutputCache { width, lines });
        }
        let cache = self.wrapped_cache.borrow();
        let lines = cache
            .as_ref()
            .map_or(&[][..], |cache| cache.lines.as_slice());
        let visual_lines = lines.len();
        let scroll_max = visual_lines.saturating_sub(height);
        self.scroll_max.set(scroll_max);
        if self.follow_tail.get() {
            self.scroll.set(scroll_max);
        } else {
            self.scroll.set(self.scroll.get().min(scroll_max));
        }
        let scroll = self.scroll();
        ToolOutputViewport {
            lines: lines.iter().skip(scroll).take(height).cloned().collect(),
            visual_lines,
            scroll,
        }
    }

    fn replace_output(&mut self, output: String, status: ToolBlockStatus) {
        self.output = output;
        self.target.status = status;
        self.wrapped_cache.get_mut().take();
    }

    fn append_output(&mut self, delta: &str, status: ToolBlockStatus) {
        self.output.push_str(delta);
        self.target.status = status;
        self.wrapped_cache.get_mut().take();
    }

    fn update_status(&mut self, status: ToolBlockStatus) {
        self.target.status = status;
    }

    fn scroll_up(&self, amount: usize) {
        self.follow_tail.set(false);
        self.scroll.set(self.scroll().saturating_sub(amount));
    }

    fn scroll_down(&self, amount: usize) {
        let scroll = self
            .scroll()
            .saturating_add(amount)
            .min(self.scroll_max.get());
        self.scroll.set(scroll);
        self.follow_tail.set(scroll == self.scroll_max.get());
    }

    fn scroll_to_top(&self) {
        self.follow_tail.set(false);
        self.scroll.set(0);
    }

    fn scroll_to_bottom(&self) {
        self.follow_tail.set(true);
        self.scroll.set(self.scroll_max.get());
    }
}

impl ShellState {
    pub(super) fn open_tool_output_at(&mut self, transcript_index: usize) -> bool {
        let Some(line) = self.transcript.get(transcript_index) else {
            return false;
        };
        if line.kind != TranscriptKind::Output {
            return false;
        }
        let Some(item_id) = line.item_id.clone() else {
            return false;
        };
        let output = line.full_text.clone().unwrap_or_else(|| line.text.clone());
        let status = line.tool_status.unwrap_or(ToolBlockStatus::Success);
        let title = self
            .transcript
            .iter()
            .rev()
            .find(|candidate| {
                candidate.kind == TranscriptKind::Tool
                    && candidate.item_id.as_deref() == Some(&item_id)
            })
            .or_else(|| {
                self.transcript
                    .iter()
                    .take(transcript_index)
                    .rev()
                    .find(|candidate| candidate.kind == TranscriptKind::Tool)
            })
            .map_or_else(|| "Tool output".to_string(), |line| line.text.clone());

        self.close_agent_log();
        self.command_palette = None;
        self.selector = None;
        self.clear_transcript_selection();
        self.tool_output = Some(ToolOutputState::new(
            ToolOutputTarget {
                item_id,
                title,
                status,
            },
            output,
        ));
        true
    }

    pub(super) fn open_selected_tool_output(&mut self) -> bool {
        self.transcript_selection
            .is_some_and(|index| self.open_tool_output_at(index))
    }

    pub(super) fn close_tool_output(&mut self) {
        self.tool_output = None;
    }

    pub(super) fn replace_open_tool_output(
        &mut self,
        item_id: &str,
        output: String,
        status: ToolBlockStatus,
    ) {
        if let Some(state) = &mut self.tool_output
            && state.target.item_id == item_id
        {
            state.replace_output(output, status);
        }
    }

    pub(super) fn append_open_tool_output(
        &mut self,
        item_id: &str,
        delta: &str,
        status: ToolBlockStatus,
    ) {
        if let Some(state) = &mut self.tool_output
            && state.target.item_id == item_id
        {
            state.append_output(delta, status);
        }
    }

    pub(super) fn update_open_tool_output_status(
        &mut self,
        item_id: &str,
        status: ToolBlockStatus,
    ) {
        if let Some(state) = &mut self.tool_output
            && state.target.item_id == item_id
        {
            state.update_status(status);
        }
    }

    pub(super) fn update_open_tool_output_title(
        &mut self,
        item_id: &str,
        title: &str,
        status: ToolBlockStatus,
    ) {
        if let Some(state) = &mut self.tool_output
            && state.target.item_id == item_id
        {
            state.target.title = title.to_string();
            state.update_status(status);
        }
    }

    pub(super) fn handle_tool_output_key(&mut self, key: KeyEvent) -> bool {
        if self.tool_output.is_none()
            || !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            || !matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            return false;
        }
        if matches!(key.code, KeyCode::Esc) {
            self.close_tool_output();
            return true;
        }
        let Some(output) = &self.tool_output else {
            return false;
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => output.scroll_up(/*amount*/ 1),
            KeyCode::Down | KeyCode::Char('j') => output.scroll_down(/*amount*/ 1),
            KeyCode::PageUp => output.scroll_up(TOOL_OUTPUT_PAGE_STEP),
            KeyCode::PageDown => output.scroll_down(TOOL_OUTPUT_PAGE_STEP),
            KeyCode::Home | KeyCode::Char('g') => output.scroll_to_top(),
            KeyCode::End | KeyCode::Char('G') => output.scroll_to_bottom(),
            _ => return false,
        }
        true
    }

    pub(super) fn scroll_tool_output_up(&self) {
        if let Some(output) = &self.tool_output {
            output.scroll_up(/*amount*/ 3);
        }
    }

    pub(super) fn scroll_tool_output_down(&self) {
        if let Some(output) = &self.tool_output {
            output.scroll_down(/*amount*/ 3);
        }
    }
}

#[cfg(test)]
#[path = "tool_output_tests.rs"]
mod tests;
