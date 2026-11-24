//! Conversation-only folding of adjacent tool invocations and their output.

use super::ShellState;
use super::ToolBlockStatus;
use super::TranscriptKind;
use super::TranscriptLine;
use super::design::palette;
use super::design::selection_style;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::terminal_hyperlinks::HyperlinkLine;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::TurnStatus;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::ops::Range;

#[derive(Debug, Clone)]
pub(super) enum ToolAction {
    Command,
    Edit,
    Search,
    ViewImage,
    GenerateImage,
    Wait,
    Call(String),
    Agent(CollabAgentTool),
}

impl ToolAction {
    fn label(&self, status: ToolBlockStatus) -> String {
        // The subject names the operation when its result is unavailable or it did not succeed.
        let (present, past, subject) = match self {
            Self::Command => ("Running command", "Ran command", "Command"),
            Self::Edit => ("Editing files", "Edited files", "File edit"),
            Self::Search => ("Searching web", "Searched web", "Web search"),
            Self::ViewImage => ("Viewing image", "Viewed image", "Image view"),
            Self::GenerateImage => ("Generating image", "Generated image", "Image generation"),
            Self::Wait => ("Waiting", "Waited", "Wait"),
            Self::Agent(tool) => match tool {
                CollabAgentTool::SpawnAgent => ("Starting agent", "Started agent", "Agent startup"),
                CollabAgentTool::SendInput | CollabAgentTool::SendMessage => {
                    ("Messaging agent", "Messaged agent", "Agent message")
                }
                CollabAgentTool::ResumeAgent => ("Resuming agent", "Resumed agent", "Agent resume"),
                CollabAgentTool::Wait => ("Waiting for agents", "Waited for agents", "Agent wait"),
                CollabAgentTool::CloseAgent => ("Closing agent", "Closed agent", "Agent close"),
                CollabAgentTool::FollowupTask => (
                    "Assigning follow-up",
                    "Assigned follow-up",
                    "Follow-up assignment",
                ),
                CollabAgentTool::InterruptAgent => (
                    "Interrupting agent",
                    "Interrupted agent",
                    "Agent interruption",
                ),
                CollabAgentTool::ListAgents => ("Listing agents", "Listed agents", "Agent listing"),
            },
            Self::Call(tool) => {
                return match status {
                    ToolBlockStatus::Running => format!("Calling {tool}..."),
                    ToolBlockStatus::Success => format!("Called {tool}"),
                    ToolBlockStatus::Fail => format!("Call to {tool} failed"),
                    ToolBlockStatus::Interrupted => format!("Call to {tool} interrupted"),
                    ToolBlockStatus::Unconfirmed => format!("No result recorded for {tool}"),
                };
            }
        };
        match status {
            ToolBlockStatus::Running => format!("{present}..."),
            ToolBlockStatus::Success => past.into(),
            ToolBlockStatus::Fail => format!("{subject} failed"),
            ToolBlockStatus::Interrupted => format!("{subject} interrupted"),
            ToolBlockStatus::Unconfirmed => format!("{subject} result unavailable"),
        }
    }
}

pub(super) fn tool_action(item: &ThreadItemPayload) -> Option<ToolAction> {
    Some(match item {
        ThreadItemPayload::CommandExecution { .. } => ToolAction::Command,
        ThreadItemPayload::WebSearch(_) => ToolAction::Search,
        ThreadItemPayload::ImageView { .. } => ToolAction::ViewImage,
        ThreadItemPayload::ImageGeneration(_) => ToolAction::GenerateImage,
        ThreadItemPayload::Sleep(_) => ToolAction::Wait,
        ThreadItemPayload::McpToolCall { server, tool, .. } => {
            ToolAction::Call(format!("{server}/{tool}"))
        }
        ThreadItemPayload::FunctionCallOutput {
            name: tool,
            namespace,
            ..
        }
        | ThreadItemPayload::DynamicToolCall {
            tool, namespace, ..
        } => match (namespace.as_deref(), tool.as_str()) {
            (None | Some("functions"), "exec_command" | "shell" | "shell_command") => {
                ToolAction::Command
            }
            (None | Some("functions"), "apply_patch") => ToolAction::Edit,
            (Some("web"), "run" | "search_query") => ToolAction::Search,
            (Some(namespace), _) => ToolAction::Call(format!("{namespace}/{tool}")),
            (None, _) => ToolAction::Call(tool.clone()),
        },
        ThreadItemPayload::CollabAgentToolCall { tool, .. } => ToolAction::Agent(tool.clone()),
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
    pub(super) fn tool_action(&self) -> Option<&ToolAction> {
        match self.kind {
            TranscriptKind::Diff => Some(&ToolAction::Edit),
            _ => self.tool_action.as_ref(),
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
    action: &'a ToolAction,
    status: ToolBlockStatus,
}

impl ToolGroup<'_> {
    pub(super) fn header(&self, width: u16, selected: bool) -> HyperlinkLine {
        let marker = if self.expanded { "⌄ " } else { "▸ " };
        let mut line = Line::from(vec![
            marker.fg(palette::cyan()).bold(),
            self.action.label(self.status).fg(match self.status {
                ToolBlockStatus::Fail => palette::error(),
                ToolBlockStatus::Interrupted | ToolBlockStatus::Unconfirmed => palette::warning(),
                ToolBlockStatus::Running | ToolBlockStatus::Success => palette::text(),
            }),
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
    /// A turn can end without a final item notification. Only settled item results prove success.
    pub(super) fn finish_pending_tool_calls(&mut self, status: &TurnStatus) {
        let terminal = match status {
            TurnStatus::InProgress => return,
            TurnStatus::Interrupted => ToolBlockStatus::Interrupted,
            TurnStatus::Failed | TurnStatus::Completed => ToolBlockStatus::Unconfirmed,
        };
        let mut output_ids = Vec::new();
        for line in self
            .transcript
            .iter_mut()
            .rev()
            .take_while(|line| line.kind != TranscriptKind::Separator)
        {
            if line.tool_action().is_some() && line.tool_status == Some(ToolBlockStatus::Running) {
                line.tool_status = Some(terminal);
                line.mark_render_changed();
                if let Some(id) = &line.item_id {
                    output_ids.push(id.clone());
                }
            }
        }
        for id in output_ids {
            self.update_output_status_for_item(&id, terminal);
        }
    }

    pub(super) fn tool_groups(&self) -> impl Iterator<Item = ToolGroup<'_>> {
        let mut items = self.transcript.iter().enumerate().peekable();
        std::iter::from_fn(move || {
            let (start, first, mut action) =
                items.find_map(|(i, line)| line.tool_action().map(|action| (i, line, action)))?;
            let mut status = first.tool_status.unwrap_or(ToolBlockStatus::Unconfirmed);
            let mut call_id = first.item_id.as_deref();
            while let Some(&(_, line)) = items.peek() {
                if let Some(next) = line.tool_action() {
                    // A later, faster parallel call may finish before an earlier call.
                    let next_status = line.tool_status.unwrap_or(ToolBlockStatus::Unconfirmed);
                    if status != ToolBlockStatus::Running || next_status == ToolBlockStatus::Running
                    {
                        action = next;
                        status = next_status;
                    }
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
                action,
                status,
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
