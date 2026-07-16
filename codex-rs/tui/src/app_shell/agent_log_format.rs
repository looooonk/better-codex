use crate::git_action_directives::parse_assistant_markdown;
use crate::session_transcript::RawReasoningVisibility;
use crate::session_transcript::TranscriptLines;
use crate::session_transcript::thread_item_to_transcript_lines;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnItemsView;
use ratatui::style::Stylize;
use ratatui::text::Line;
use serde_json::Map;
use serde_json::Value;

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

    let mut lines = vec![
        vec!["Thread  ".dim(), thread.id.clone().into()].into(),
        vec![
            "Working directory  ".dim(),
            thread.cwd.as_path().display().to_string().into(),
        ]
        .into(),
        vec![
            "Persisted state  ".dim(),
            format!("{:?}", thread.status).into(),
            "  Source  ".dim(),
            format!("{:?}", thread.source).into(),
        ]
        .into(),
    ];

    for (index, turn) in thread.turns.iter().enumerate() {
        lines.push(Line::default());
        lines.push(
            vec![
                format!("Turn {}", index + 1).magenta().bold(),
                "  ".into(),
                format!("{:?}", turn.status).bold(),
                "  ".into(),
                turn.id.clone().dim(),
            ]
            .into(),
        );
        if let Some(timing) = turn_timing(turn) {
            lines.push(timing.dim().into());
        }

        for item in &turn.items {
            lines.push(Line::default());
            let mut readable =
                thread_item_to_transcript_lines(item, thread, raw_reasoning_visibility);
            if readable.is_empty()
                && matches!(
                    (item, raw_reasoning_visibility),
                    (
                        ThreadItem::Reasoning { content, .. },
                        RawReasoningVisibility::Hidden
                    ) if !content.is_empty()
                )
            {
                readable.push(
                    "Reasoning content hidden by configuration"
                        .italic()
                        .dim()
                        .into(),
                );
            }
            lines.append(&mut readable);
            lines.extend(item_detail_lines(item, thread, raw_reasoning_visibility));
        }

        if let Some(error) = &turn.error {
            lines.push(Line::default());
            lines.extend(turn_error_lines(error));
        }
    }

    if thread.turns.is_empty() {
        lines.push(Line::default());
        lines.push("No persisted agent turns available".italic().dim().into());
    }
    Ok(lines)
}

fn turn_timing(turn: &codex_app_server_protocol::Turn) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(started_at) = turn.started_at {
        parts.push(format!("started {started_at}"));
    }
    if let Some(completed_at) = turn.completed_at {
        parts.push(format!("completed {completed_at}"));
    }
    if let Some(duration_ms) = turn.duration_ms {
        parts.push(format!("{duration_ms}ms"));
    }
    (!parts.is_empty()).then(|| parts.join("  ·  "))
}

fn item_detail_lines(
    item: &ThreadItem,
    thread: &Thread,
    raw_reasoning_visibility: RawReasoningVisibility,
) -> TranscriptLines {
    let Ok(Value::Object(mut fields)) = serde_json::to_value(item) else {
        return vec!["Persisted item details could not be rendered".red().into()];
    };
    fields.remove("type");
    fields.remove("id");
    match item {
        ThreadItem::UserMessage { .. } | ThreadItem::HookPrompt { .. } => {}
        ThreadItem::AgentMessage { text, .. } => {
            let parsed = parse_assistant_markdown(text, &thread.cwd);
            if parsed.visible_markdown == text.trim_end() {
                fields.remove("text");
            }
        }
        ThreadItem::Plan { .. } => {
            fields.remove("text");
        }
        ThreadItem::Reasoning { .. } => {
            if matches!(raw_reasoning_visibility, RawReasoningVisibility::Hidden) {
                fields.remove("summary");
            }
            fields.remove("content");
        }
        ThreadItem::CommandExecution { .. } => {
            remove_fields(
                &mut fields,
                &["command", "status", "aggregatedOutput", "exitCode"],
            );
        }
        ThreadItem::FileChange { .. } => {
            fields.remove("status");
        }
        ThreadItem::McpToolCall { .. } => {
            remove_fields(&mut fields, &["server", "tool", "status"]);
        }
        ThreadItem::DynamicToolCall { .. } => {
            remove_fields(&mut fields, &["namespace", "tool", "status"]);
        }
        ThreadItem::CollabAgentToolCall { .. } => {
            remove_fields(&mut fields, &["tool", "status"]);
        }
        ThreadItem::SubAgentActivity { .. } => {
            remove_fields(&mut fields, &["kind", "agentPath"]);
        }
        ThreadItem::WebSearch(_) => {
            fields.remove("query");
        }
        ThreadItem::ImageView { .. } => {
            fields.remove("path");
        }
        ThreadItem::Sleep { .. } => {
            fields.remove("durationMs");
        }
        ThreadItem::ImageGeneration(_) => {
            remove_fields(&mut fields, &["status", "savedPath"]);
        }
        ThreadItem::EnteredReviewMode { .. } | ThreadItem::ExitedReviewMode { .. } => {
            fields.remove("review");
        }
        ThreadItem::ContextCompaction { .. } => {}
    }
    render_detail_fields("Persisted details", fields)
}

fn turn_error_lines(error: &TurnError) -> TranscriptLines {
    let mut lines = vec![
        "Turn failed".red().bold().into(),
        error.message.clone().red().into(),
    ];
    let Ok(Value::Object(mut fields)) = serde_json::to_value(error) else {
        return lines;
    };
    fields.remove("message");
    lines.extend(render_detail_fields("Failure details", fields));
    lines
}

fn remove_fields(fields: &mut Map<String, Value>, keys: &[&str]) {
    for key in keys {
        fields.remove(*key);
    }
}

fn render_detail_fields(label: &str, mut fields: Map<String, Value>) -> TranscriptLines {
    fields.retain(|_, value| detail_value_is_present(value));
    if fields.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![label.to_string().dim().bold().into()];
    match serde_json::to_string_pretty(&Value::Object(fields)) {
        Ok(details) => lines.extend(details.lines().map(|line| format!("  {line}").dim().into())),
        Err(err) => lines.push(format!("  could not render details: {err}").red().into()),
    }
    lines
}

fn detail_value_is_present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !value.is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        Value::Bool(_) | Value::Number(_) => true,
    }
}
