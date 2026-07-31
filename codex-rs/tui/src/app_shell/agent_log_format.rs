use super::design::palette;
use crate::session_transcript::RawReasoningVisibility;
use crate::session_transcript::TranscriptLines;
use crate::session_transcript::thread_item_to_transcript_lines;
use crate::text_formatting::truncate_text;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::McpToolCallStatus;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use ratatui::style::Color;
use ratatui::style::Stylize;
use ratatui::text::Line;

const MAX_COMMAND_OUTPUT_LINES: usize = 8;
const MAX_OUTPUT_LINE_GRAPHEMES: usize = 240;
const MAX_FILE_ROWS: usize = 12;
const MAX_ERROR_GRAPHEMES: usize = 400;

pub(super) fn thread_to_agent_log_lines(
    thread: &Thread,
    raw_reasoning_visibility: RawReasoningVisibility,
) -> Result<TranscriptLines, String> {
    for turn in &thread.turns {
        match turn.items_view {
            TurnItemsView::Full => {}
            TurnItemsView::NotLoaded => {
                return Err(format!(
                    "Complete agent history is unavailable: turn {} was not loaded",
                    turn.id
                ));
            }
            TurnItemsView::Summary => {
                return Err(format!(
                    "Complete agent history is unavailable: turn {} contains only a summary",
                    turn.id
                ));
            }
        }
    }

    let rendered_turns = local_activity_turns(thread)
        .iter()
        .filter_map(|turn| render_turn(turn, thread, raw_reasoning_visibility))
        .collect::<Vec<_>>();
    if rendered_turns.is_empty() {
        return Ok(vec![
            "No subagent activity available"
                .italic()
                .fg(palette::muted())
                .into(),
        ]);
    }

    let mut lines = Vec::new();
    for (index, (turn, mut activity)) in rendered_turns.into_iter().enumerate() {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(turn_header(index + 1, turn));
        lines.append(&mut activity);
    }
    Ok(lines)
}

fn local_activity_turns(thread: &Thread) -> &[Turn] {
    if thread.forked_from_id.is_some()
        && let Some(local_start) = thread
            .turns
            .iter()
            .rposition(turn_contains_user_message)
            .map(|index| index + 1)
        && local_start < thread.turns.len()
    {
        return &thread.turns[local_start..];
    }
    &thread.turns
}

fn turn_contains_user_message(turn: &Turn) -> bool {
    turn.items
        .iter()
        .any(|item| matches!(item, ThreadItem::UserMessage { .. }))
}

fn render_turn<'a>(
    turn: &'a Turn,
    thread: &Thread,
    raw_reasoning_visibility: RawReasoningVisibility,
) -> Option<(&'a Turn, TranscriptLines)> {
    let mut lines = Vec::new();
    for item in &turn.items {
        let mut item_lines = agent_item_lines(item, thread, raw_reasoning_visibility);
        if item_lines.is_empty() {
            continue;
        }
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.append(&mut item_lines);
    }
    if let Some(error) = &turn.error {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push("Turn failed".bold().fg(palette::error()).into());
        lines.push(
            truncate_text(&error.message, MAX_ERROR_GRAPHEMES)
                .fg(palette::error())
                .into(),
        );
    }
    (!lines.is_empty()).then_some((turn, lines))
}

fn turn_header(index: usize, turn: &Turn) -> Line<'static> {
    let (status, color) = turn_status(&turn.status);
    let mut spans = vec![
        format!("Turn {index}").bold().fg(palette::purple()),
        "  ".into(),
        status.fg(color),
    ];
    if let Some(duration_ms) = turn.duration_ms {
        spans.push("  ".into());
        spans.push(format_duration(duration_ms).fg(palette::muted()));
    }
    spans.into()
}

fn agent_item_lines(
    item: &ThreadItem,
    thread: &Thread,
    raw_reasoning_visibility: RawReasoningVisibility,
) -> TranscriptLines {
    match item {
        ThreadItem::UserMessage { .. } | ThreadItem::HookPrompt { .. } => Vec::new(),
        ThreadItem::CommandExecution {
            command,
            status,
            aggregated_output,
            exit_code,
            duration_ms,
            ..
        } => command_lines(
            command,
            status,
            aggregated_output.as_deref(),
            *exit_code,
            *duration_ms,
        ),
        ThreadItem::FileChange {
            changes, status, ..
        } => file_change_lines(changes, status),
        ThreadItem::McpToolCall {
            server,
            tool,
            status,
            error,
            duration_ms,
            ..
        } => tool_lines(
            &format!("{server}/{tool}"),
            mcp_status(status),
            error.as_ref().map(|error| error.message.as_str()),
            *duration_ms,
        ),
        ThreadItem::DynamicToolCall {
            namespace,
            tool,
            status,
            duration_ms,
            ..
        } => {
            let name = namespace
                .as_ref()
                .map(|namespace| format!("{namespace}/{tool}"))
                .unwrap_or_else(|| tool.clone());
            tool_lines(
                &name,
                dynamic_tool_status(status),
                /*error*/ None,
                *duration_ms,
            )
        }
        ThreadItem::CollabAgentToolCall {
            tool,
            status,
            receiver_thread_ids,
            ..
        } => collaboration_lines(tool, status, receiver_thread_ids.len()),
        ThreadItem::SubAgentActivity {
            kind, agent_path, ..
        } => vec![
            vec![
                "Agent".bold().fg(palette::purple()),
                "  ".into(),
                format!("{kind:?}").fg(palette::muted()),
                "  ".into(),
                agent_path.clone().into(),
            ]
            .into(),
        ],
        ThreadItem::AgentMessage { .. }
        | ThreadItem::Plan { .. }
        | ThreadItem::Reasoning { .. }
        | ThreadItem::WebSearch(_)
        | ThreadItem::ImageView { .. }
        | ThreadItem::Sleep { .. }
        | ThreadItem::ImageGeneration(_)
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. }
        | ThreadItem::ContextCompaction { .. } => {
            thread_item_to_transcript_lines(item, thread, raw_reasoning_visibility)
        }
    }
}

fn command_lines(
    command: &str,
    status: &CommandExecutionStatus,
    output: Option<&str>,
    exit_code: Option<i32>,
    duration_ms: Option<i64>,
) -> TranscriptLines {
    let (status, color) = command_status(status);
    let mut header = vec![
        "Command".bold().fg(palette::purple()),
        "  ".into(),
        status.fg(color),
    ];
    if let Some(exit_code) = exit_code {
        header.push("  ".into());
        header.push(format!("exit {exit_code}").fg(palette::muted()));
    }
    if let Some(duration_ms) = duration_ms {
        header.push("  ".into());
        header.push(format_duration(duration_ms).fg(palette::muted()));
    }
    let mut lines = vec![
        header.into(),
        vec!["$ ".fg(palette::muted()), command.to_string().into()].into(),
    ];
    let Some(output) = output
        .map(str::trim_end)
        .filter(|output| !output.is_empty())
    else {
        return lines;
    };
    let output_lines = output.lines().collect::<Vec<_>>();
    let visible_start = output_lines.len().saturating_sub(MAX_COMMAND_OUTPUT_LINES);
    lines.push(Line::default());
    lines.push(
        if visible_start == 0 {
            "Output".bold().fg(palette::muted())
        } else {
            format!(
                "Output  last {} of {} lines",
                output_lines.len() - visible_start,
                output_lines.len()
            )
            .bold()
            .fg(palette::muted())
        }
        .into(),
    );
    lines.extend(output_lines[visible_start..].iter().map(|line| {
        truncate_text(line.trim_end(), MAX_OUTPUT_LINE_GRAPHEMES)
            .fg(palette::muted())
            .into()
    }));
    lines
}

fn file_change_lines(
    changes: &[codex_app_server_protocol::FileUpdateChange],
    status: &PatchApplyStatus,
) -> TranscriptLines {
    let (status, color) = patch_status(status);
    let mut lines = vec![
        vec![
            "Edits".bold().fg(palette::purple()),
            "  ".into(),
            status.fg(color),
            "  ".into(),
            format!(
                "{} {}",
                changes.len(),
                if changes.len() == 1 { "file" } else { "files" }
            )
            .fg(palette::muted()),
        ]
        .into(),
    ];
    lines.extend(changes.iter().take(MAX_FILE_ROWS).map(|change| {
        let (marker, color, destination) = match &change.kind {
            PatchChangeKind::Add => ("A", palette::success(), None),
            PatchChangeKind::Delete => ("D", palette::error(), None),
            PatchChangeKind::Update {
                move_path: Some(move_path),
            } => (
                "R",
                palette::warning(),
                Some(move_path.display().to_string()),
            ),
            PatchChangeKind::Update { move_path: None } => ("M", palette::warning(), None),
        };
        let mut spans = vec![
            "  ".into(),
            marker.bold().fg(color),
            "  ".into(),
            change.path.clone().into(),
        ];
        if let Some(destination) = destination {
            spans.push(" -> ".fg(palette::muted()));
            spans.push(destination.into());
        }
        spans.into()
    }));
    if changes.len() > MAX_FILE_ROWS {
        lines.push(
            format!("  ... {} more files", changes.len() - MAX_FILE_ROWS)
                .fg(palette::muted())
                .into(),
        );
    }
    lines
}

fn tool_lines(
    name: &str,
    (status, color): (&'static str, Color),
    error: Option<&str>,
    duration_ms: Option<i64>,
) -> TranscriptLines {
    let mut spans = vec![
        "Tool".bold().fg(palette::purple()),
        "  ".into(),
        name.to_string().into(),
        "  ".into(),
        status.fg(color),
    ];
    if let Some(duration_ms) = duration_ms {
        spans.push("  ".into());
        spans.push(format_duration(duration_ms).fg(palette::muted()));
    }
    let mut lines = vec![spans.into()];
    if let Some(error) = error {
        lines.push(
            truncate_text(error, MAX_ERROR_GRAPHEMES)
                .fg(palette::error())
                .into(),
        );
    }
    lines
}

fn collaboration_lines(
    tool: &CollabAgentTool,
    status: &CollabAgentToolCallStatus,
    receiver_count: usize,
) -> TranscriptLines {
    let action = match tool {
        CollabAgentTool::SpawnAgent => "Spawn agent",
        CollabAgentTool::SendInput => "Send input",
        CollabAgentTool::ResumeAgent => "Resume agent",
        CollabAgentTool::Wait => "Wait for agent",
        CollabAgentTool::CloseAgent => "Close agent",
    };
    let (status, color) = collaboration_status(status);
    let mut spans = vec![
        "Agent".bold().fg(palette::purple()),
        "  ".into(),
        action.into(),
        "  ".into(),
        status.fg(color),
    ];
    if receiver_count > 1 {
        spans.push("  ".into());
        spans.push(format!("{receiver_count} agents").fg(palette::muted()));
    }
    vec![spans.into()]
}

fn turn_status(status: &TurnStatus) -> (&'static str, Color) {
    match status {
        TurnStatus::Completed => ("Completed", palette::success()),
        TurnStatus::Interrupted => ("Interrupted", palette::warning()),
        TurnStatus::Failed => ("Failed", palette::error()),
        TurnStatus::InProgress => ("Running", palette::warning()),
    }
}

fn command_status(status: &CommandExecutionStatus) -> (&'static str, Color) {
    match status {
        CommandExecutionStatus::InProgress => ("Running", palette::warning()),
        CommandExecutionStatus::Completed => ("Completed", palette::success()),
        CommandExecutionStatus::Failed => ("Failed", palette::error()),
        CommandExecutionStatus::Declined => ("Declined", palette::warning()),
    }
}

fn patch_status(status: &PatchApplyStatus) -> (&'static str, Color) {
    match status {
        PatchApplyStatus::InProgress => ("Applying", palette::warning()),
        PatchApplyStatus::Completed => ("Completed", palette::success()),
        PatchApplyStatus::Failed => ("Failed", palette::error()),
        PatchApplyStatus::Declined => ("Declined", palette::warning()),
    }
}

fn mcp_status(status: &McpToolCallStatus) -> (&'static str, Color) {
    match status {
        McpToolCallStatus::InProgress => ("Running", palette::warning()),
        McpToolCallStatus::Completed => ("Completed", palette::success()),
        McpToolCallStatus::Failed => ("Failed", palette::error()),
    }
}

fn dynamic_tool_status(status: &DynamicToolCallStatus) -> (&'static str, Color) {
    match status {
        DynamicToolCallStatus::InProgress => ("Running", palette::warning()),
        DynamicToolCallStatus::Completed => ("Completed", palette::success()),
        DynamicToolCallStatus::Failed => ("Failed", palette::error()),
    }
}

fn collaboration_status(status: &CollabAgentToolCallStatus) -> (&'static str, Color) {
    match status {
        CollabAgentToolCallStatus::InProgress => ("Running", palette::warning()),
        CollabAgentToolCallStatus::Completed => ("Completed", palette::success()),
        CollabAgentToolCallStatus::Failed => ("Failed", palette::error()),
    }
}

fn format_duration(duration_ms: i64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else {
        let tenths = duration_ms.saturating_add(50) / 100;
        let seconds = tenths / 10;
        let tenths_digit = tenths % 10;
        format!("{seconds}.{tenths_digit}s")
    }
}

#[cfg(test)]
#[path = "agent_log_format_tests.rs"]
mod tests;
