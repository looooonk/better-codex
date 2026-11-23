//! Conversation-only folding of adjacent tool invocations and their output.

use super::ShellState;
use super::TranscriptKind;
use super::TranscriptLine;
use super::design::palette;
use super::design::selection_style;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::terminal_hyperlinks::HyperlinkLine;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::ThreadItemPayload;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::ops::Range;

pub(super) fn activity_label(item: &ThreadItemPayload) -> Option<String> {
    Some(match item {
        ThreadItemPayload::CommandExecution { .. } => "Running command".into(),
        ThreadItemPayload::WebSearch(_) => "Searching web".into(),
        ThreadItemPayload::ImageView { .. } => "Viewing image".into(),
        ThreadItemPayload::ImageGeneration(_) => "Generating image".into(),
        ThreadItemPayload::Sleep(_) => "Waiting".into(),
        ThreadItemPayload::McpToolCall { server, tool, .. } => format!("Calling {server}/{tool}"),
        ThreadItemPayload::FunctionCallOutput {
            name: tool,
            namespace,
            ..
        }
        | ThreadItemPayload::DynamicToolCall {
            tool, namespace, ..
        } => match (namespace.as_deref(), tool.as_str()) {
            (None | Some("functions"), "exec_command" | "shell" | "shell_command") => {
                "Running command".into()
            }
            (None | Some("functions"), "apply_patch") => "Editing files".into(),
            (Some("web"), "run" | "search_query") => "Searching web".into(),
            (Some(namespace), _) => format!("Calling {namespace}/{tool}"),
            (None, _) => format!("Calling {tool}"),
        },
        ThreadItemPayload::CollabAgentToolCall { tool, .. } => match tool {
            CollabAgentTool::SpawnAgent => "Starting agent",
            CollabAgentTool::SendInput | CollabAgentTool::SendMessage => "Messaging agent",
            CollabAgentTool::ResumeAgent => "Resuming agent",
            CollabAgentTool::Wait => "Waiting for agents",
            CollabAgentTool::CloseAgent => "Closing agent",
            CollabAgentTool::FollowupTask => "Assigning follow-up",
            CollabAgentTool::InterruptAgent => "Interrupting agent",
            CollabAgentTool::ListAgents => "Listing agents",
        }
        .into(),
        ThreadItemPayload::UserMessage { .. }
        | ThreadItemPayload::HookPrompt { .. }
        | ThreadItemPayload::AgentMessage { .. }
        | ThreadItemPayload::Plan { .. }
        | ThreadItemPayload::Reasoning { .. }
        | ThreadItemPayload::FileChange { .. }
        | ThreadItemPayload::SubAgentActivity { .. }
        | ThreadItemPayload::EnteredReviewMode { .. }
        | ThreadItemPayload::ExitedReviewMode { .. }
        | ThreadItemPayload::ContextCompaction { .. } => return None,
    })
}

impl TranscriptLine {
    pub(super) fn activity_label(&self) -> Option<&str> {
        match self.kind {
            TranscriptKind::Diff => Some("Editing files"),
            _ => self.tool_label.as_deref(),
        }
    }

    pub(super) fn is_tool_child_of(&self, call_id: Option<&str>) -> bool {
        matches!(self.kind, TranscriptKind::Output | TranscriptKind::Error)
            && call_id.is_some()
            && self.item_id.as_deref() == call_id
    }
}

pub(super) struct ToolGroup<'a> {
    pub(super) rows: Range<usize>,
    pub(super) expanded: bool,
    label: &'a str,
}

impl ToolGroup<'_> {
    pub(super) fn header(&self, width: u16, selected: bool) -> HyperlinkLine {
        let marker = if self.expanded { "⌄ " } else { "> " };
        let mut line = Line::from(vec![
            marker.fg(palette::cyan()).bold(),
            format!("{}...", self.label).fg(palette::text()),
        ]);
        if selected {
            line = line.style(selection_style());
        }
        HyperlinkLine::new(truncate_line_with_ellipsis_if_overflow(
            line,
            usize::from(width),
        ))
    }
}

impl ShellState {
    pub(super) fn tool_groups(&self) -> impl Iterator<Item = ToolGroup<'_>> {
        let mut items = self.transcript.iter().enumerate().peekable();
        std::iter::from_fn(move || {
            let (start, first, mut label) =
                items.find_map(|(i, line)| line.activity_label().map(|label| (i, line, label)))?;
            let mut call_id = first.item_id.as_deref();
            while let Some(&(_, line)) = items.peek() {
                if let Some(next) = line.activity_label() {
                    label = next;
                    call_id = line.item_id.as_deref();
                } else if !line.is_tool_child_of(call_id) {
                    break;
                }
                items.next();
            }
            let end = items.peek().map_or(self.transcript.len(), |(i, _)| *i);
            Some(ToolGroup {
                rows: start..end,
                expanded: first.tool_group_expanded,
                label,
            })
        })
    }

    pub(super) fn tool_group_start(&self, index: usize) -> Option<usize> {
        self.tool_groups()
            .find(|group| group.rows.contains(&index))
            .map(|group| group.rows.start)
    }

    pub(super) fn toggle_tool_group(&mut self, start: usize) {
        self.clear_transcript_text_selection();
        if let Some(selected) = self.transcript_selection
            && self.transcript[start].tool_group_expanded
            && self.tool_group_start(selected) == Some(start)
        {
            self.transcript_selection = Some(start);
        }
        let first = &mut self.transcript[start];
        first.tool_group_expanded = !first.tool_group_expanded;
        first.mark_render_changed();
    }

    pub(super) fn select_adjacent_tool_group(&mut self, key: crossterm::event::KeyCode) {
        let starts: Vec<_> = self.tool_groups().map(|group| group.rows.start).collect();
        let selected = self.transcript_selection.unwrap_or_default();
        let next = if key == crossterm::event::KeyCode::BackTab {
            starts
                .iter()
                .rev()
                .find(|&&start| start < selected)
                .or(starts.last())
        } else {
            starts
                .iter()
                .find(|&&start| start > selected)
                .or(starts.first())
        };
        if let Some(&next) = next {
            self.transcript_selection = Some(next);
            self.transcript_selection_needs_reveal.set(true);
        }
    }
}

#[cfg(test)]
#[path = "tool_groups_tests.rs"]
mod tests;
