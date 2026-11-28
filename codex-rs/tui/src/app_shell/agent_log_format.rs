use super::agent_text::markdown;
use super::agent_trace::Message;
use super::agent_trace::Trace;
use super::design::palette;
use crate::git_action_directives::parse_assistant_markdown;
use crate::reasoning_summary::split_reasoning_summary_parts;
use crate::session_transcript::RawReasoningVisibility;
use crate::session_transcript::TranscriptLines;
use crate::session_transcript::thread_item_to_transcript_lines;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::plain_hyperlink_lines;
use crate::text_formatting::truncate_text;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::CommandAction;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::McpToolCallStatus;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use ratatui::style::Color;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::path::Path;

const MAX_PREVIEW: usize = 88;
const MAX_FILE_ROWS: usize = 3;

pub(super) fn thread_to_agent_log_lines(
    thread: &Thread,
    raw_reasoning_visibility: RawReasoningVisibility,
    trace: &Trace,
) -> Result<Vec<HyperlinkLine>, String> {
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

    let lane = trace
        .lanes
        .iter()
        .position(|lane| lane.target.thread_id == thread.id);
    let mut mail = trace
        .messages
        .iter()
        .filter_map(|msg| {
            if Some(msg.from) == lane {
                Some((msg.at, msg))
            } else if Some(msg.to) == lane {
                Some((msg.received_at.unwrap_or(msg.at), msg))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    mail.sort_by_key(|(at, _)| *at);
    let sent_ids = mail
        .iter()
        .filter(|(_, msg)| Some(msg.from) == lane)
        .filter_map(|(_, msg)| msg.call_id.as_deref())
        .collect::<std::collections::HashSet<_>>();
    let mut mail = mail.into_iter().peekable();
    let turns = local_activity_turns(thread);
    let mut lines = Vec::new();
    for (index, turn) in turns.iter().enumerate() {
        let mut activity = Vec::new();
        for item in &turn.items {
            if sent_ids.contains(item.id()) {
                continue;
            }
            let at = item.completed_at_ms.or(item.started_at_ms);
            while let Some((_, msg)) = mail.next_if(|(time, _)| at.is_some_and(|at| *time <= at)) {
                append_entry(&mut activity, message_lines(trace, msg, &thread.cwd));
            }
            append_entry(
                &mut activity,
                agent_item_lines(item, thread, raw_reasoning_visibility),
            );
        }
        let next_turn = turns
            .get(index + 1)
            .and_then(|t| t.started_at)
            .map(|s| s.saturating_mul(1000));
        while let Some((_, msg)) = mail
            .next_if(|(time, _)| next_turn.is_some_and(|t| *time < t) || index + 1 == turns.len())
        {
            append_entry(&mut activity, message_lines(trace, msg, &thread.cwd));
        }
        if let Some(error) = &turn.error {
            append_entry(
                &mut activity,
                plain_hyperlink_lines(vec![
                    "Turn failed".bold().fg(palette::error()).into(),
                    one_line(&error.message).fg(palette::error()).into(),
                ]),
            );
        }
        if !activity.is_empty() {
            if !lines.is_empty() {
                lines.push(HyperlinkLine::default());
            }
            lines.push(turn_header(index + 1, turn).into());
            lines.extend(activity);
        }
    }
    for (_, msg) in mail {
        append_entry(&mut lines, message_lines(trace, msg, &thread.cwd));
    }
    if lines.is_empty() {
        lines.push(Line::from("No activity".fg(palette::muted())).into());
    }
    Ok(lines)
}

fn local_activity_turns(thread: &Thread) -> &[Turn] {
    if thread.forked_from_id.is_some() && thread.parent_thread_id.is_some() {
        // Older copied forks include the parent's turns. Never discard an
        // agent's earlier run just because a later turn has user input.
        if let Some(start) = thread
            .turns
            .iter()
            .position(|t| t.started_at.is_some_and(|at| at >= thread.created_at))
        {
            return &thread.turns[start..];
        }
    }
    &thread.turns
}

fn append_entry(lines: &mut Vec<HyperlinkLine>, entry: Vec<HyperlinkLine>) {
    if entry.is_empty() {
        return;
    }
    if !lines.is_empty() {
        lines.push(HyperlinkLine::default());
    }
    lines.extend(entry);
}

fn message_lines(trace: &Trace, msg: &Message, cwd: &Path) -> Vec<HyperlinkLine> {
    let mut lines = vec![HyperlinkLine::new(Line::from(vec![
        format!(
            "{} → {}",
            trace.lanes[msg.from].target.display_name, trace.lanes[msg.to].target.display_name
        )
        .bold()
        .fg(palette::cyan()),
        format!("  {}", msg.kind).fg(palette::muted()),
    ]))];
    if msg.readable() {
        lines.extend(markdown(msg.body.text(), cwd, None));
    } else {
        lines.push(Line::from(msg.body.text().to_owned().fg(palette::muted())).into());
    }
    lines
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
) -> Vec<HyperlinkLine> {
    let lines = match &item.payload {
        ThreadItemPayload::HookPrompt { .. } => Vec::new(),
        ThreadItemPayload::UserMessage {
            id,
            client_id,
            content,
        } => {
            let text = crate::session_transcript::user_text(id, client_id, content);
            return themed_prefixed_text("You", &text, palette::cyan(), &thread.cwd);
        }
        ThreadItemPayload::CommandExecution {
            command,
            command_actions,
            status,
            aggregated_output,
            exit_code,
            duration_ms,
            ..
        } => command_lines(
            command,
            command_actions,
            status,
            aggregated_output.as_deref(),
            *exit_code,
            *duration_ms,
        ),
        ThreadItemPayload::FileChange {
            changes, status, ..
        } => file_change_lines(changes, status),
        ThreadItemPayload::McpToolCall {
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
        ThreadItemPayload::DynamicToolCall {
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
        ThreadItemPayload::CollabAgentToolCall {
            tool,
            status,
            receiver_thread_ids,
            ..
        } => collaboration_lines(tool, status, receiver_thread_ids.len()),
        ThreadItemPayload::SubAgentActivity {
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
        ThreadItemPayload::AgentMessage {
            text,
            phase,
            questions,
            ..
        } => {
            let text = crate::assistant_message::with_questions(text, questions.as_deref());
            let parsed = parse_assistant_markdown(&text, &thread.cwd);
            let label = match phase {
                Some(codex_protocol::models::MessagePhase::Commentary) => "Update",
                Some(codex_protocol::models::MessagePhase::FinalAnswer) => "Final answer",
                _ => "Assistant",
            };
            return themed_prefixed_text(
                label,
                &parsed.visible_markdown,
                palette::purple(),
                &thread.cwd,
            );
        }
        ThreadItemPayload::Plan { text, .. } => {
            return themed_prefixed_text("Plan", text, palette::success(), &thread.cwd);
        }
        ThreadItemPayload::Reasoning {
            summary, content, ..
        } => {
            let (header, text) =
                if matches!(raw_reasoning_visibility, RawReasoningVisibility::Visible)
                    && !content.is_empty()
                {
                    ("Reasoning".to_string(), content.join("\n\n"))
                } else {
                    split_reasoning_summary_parts(summary)
                };
            let label = header
                .strip_prefix("**")
                .and_then(|s| s.strip_suffix("**"))
                .unwrap_or(&header);
            return themed_prefixed_text(
                if label.is_empty() { "Reasoning" } else { label },
                &text,
                palette::muted(),
                &thread.cwd,
            );
        }
        ThreadItemPayload::FunctionCallOutput { name, output, .. } => {
            let mut lines = vec![Line::from(vec![
                "Tool  ".bold().fg(palette::purple()),
                one_line(name).into(),
            ])];
            if let codex_protocol::models::FunctionCallOutputBody::Text(text) = output {
                append_output(&mut lines, text);
            }
            lines
        }
        ThreadItemPayload::WebSearch(_)
        | ThreadItemPayload::ImageView { .. }
        | ThreadItemPayload::Sleep(_)
        | ThreadItemPayload::ImageGeneration(_)
        | ThreadItemPayload::EnteredReviewMode { .. }
        | ThreadItemPayload::ExitedReviewMode { .. }
        | ThreadItemPayload::ContextCompaction { .. } => {
            thread_item_to_transcript_lines(item, thread, raw_reasoning_visibility)
        }
    };
    plain_hyperlink_lines(lines)
}

fn themed_prefixed_text(label: &str, text: &str, color: Color, cwd: &Path) -> Vec<HyperlinkLine> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let mut lines = vec![HyperlinkLine::new(Line::from(
        label.to_owned().fg(color).bold(),
    ))];
    lines.extend(markdown(text, cwd, None));
    lines
}

fn one_line(text: &str) -> String {
    let mut lines = text.trim().lines();
    let first = lines.next().unwrap_or_default();
    let mut result = truncate_text(first, MAX_PREVIEW);
    if lines.next().is_some() {
        result.push('…');
    }
    result
}

fn append_output(lines: &mut TranscriptLines, text: &str) {
    let mut nonempty = text.lines().rev().map(str::trim).filter(|s| !s.is_empty());
    if let Some(last) = nonempty.next() {
        let start = text.trim_start();
        if start.starts_with('{') || start.starts_with("[{") {
            lines.push("Structured output".fg(palette::muted()).into());
        } else {
            let prefix = if nonempty.next().is_some() {
                "… "
            } else {
                ""
            };
            lines.push(
                format!("{prefix}{}", one_line(last))
                    .fg(palette::muted())
                    .into(),
            );
        }
    }
}

fn command_action(command: &str, actions: &[CommandAction]) -> String {
    match actions.first() {
        Some(CommandAction::Read { name, .. }) => format!("Read {}", one_line(name)),
        Some(CommandAction::ListFiles { path, .. }) => path
            .as_deref()
            .map_or_else(|| "List files".into(), |p| format!("List {}", one_line(p))),
        Some(CommandAction::Search { query, .. }) => query.as_deref().map_or_else(
            || "Search files".into(),
            |q| format!("Search {}", one_line(q)),
        ),
        _ => {
            let args = shlex::split(command).unwrap_or_default();
            let program = |s: &str| s.rsplit('/').next().unwrap_or_default().to_owned();
            let inner;
            let args = match args.as_slice() {
                [shell, flag, script, ..]
                    if matches!(program(shell).as_str(), "sh" | "bash" | "zsh" | "dash")
                        && matches!(flag.as_str(), "-lc" | "-c") =>
                {
                    inner =
                        shlex::split(script.lines().next().unwrap_or_default()).unwrap_or_default();
                    &inner
                }
                _ => &args,
            };
            let Some(name) = args.first().map(|s| program(s)) else {
                return "Run command".into();
            };
            if matches!(
                name.as_str(),
                "cargo" | "git" | "just" | "npm" | "pnpm" | "uv"
            ) && let Some(action) = args.get(1)
            {
                format!("Run {name} {}", one_line(action))
            } else {
                format!("Run {}", one_line(&name))
            }
        }
    }
}

fn command_lines(
    command: &str,
    actions: &[CommandAction],
    status: &CommandExecutionStatus,
    output: Option<&str>,
    exit_code: Option<i32>,
    duration_ms: Option<i64>,
) -> TranscriptLines {
    let (status_label, color) = command_status(status);
    let mut spans = vec![command_action(command, actions).bold()];
    if actions.len() > 1 {
        spans.push(format!("  +{}", actions.len() - 1).fg(palette::muted()));
    }
    if !matches!(status, CommandExecutionStatus::Completed) {
        spans.push(format!("  {status_label}").fg(color));
    }
    if let Some(code) = exit_code
        && code != 0
    {
        spans.push(format!("  exit {code}").fg(palette::error()));
    }
    if let Some(ms) = duration_ms
        && ms >= 1000
    {
        spans.push(format!("  {}", format_duration(ms)).fg(palette::muted()));
    }
    let mut lines = vec![spans.into()];
    let successful_inspection = !actions.is_empty()
        && matches!(status, CommandExecutionStatus::Completed)
        && exit_code.unwrap_or(0) == 0
        && actions
            .iter()
            .all(|action| !matches!(action, CommandAction::Unknown { .. }));
    if !successful_inspection && let Some(text) = output {
        append_output(&mut lines, text);
    }
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
        lines.push(one_line(error).fg(palette::error()).into());
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
        CollabAgentTool::SendMessage => "Send message",
        CollabAgentTool::FollowupTask => "Follow-up task",
        CollabAgentTool::InterruptAgent => "Interrupt agent",
        CollabAgentTool::ListAgents => "List agents",
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
        CollabAgentToolCallStatus::Interrupted => ("Interrupted", palette::warning()),
    }
}

fn format_duration(duration_ms: i64) -> String {
    if duration_ms >= 60_000 {
        codex_utils_elapsed::format_duration(std::time::Duration::from_millis(duration_ms as u64))
    } else if duration_ms < 1_000 {
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
