//! Line-based transcript rendering for the standalone session picker.

use crate::app_server_session::AppServerSession;
use crate::git_action_directives::parse_assistant_markdown;
use crate::markdown_render::render_markdown_text_with_width_and_cwd;
use crate::reasoning_summary::split_reasoning_summary_parts;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::ThreadId;
use codex_protocol::items::UserMessageItem;
use ratatui::style::Stylize as _;
use ratatui::text::Line;

pub(crate) type TranscriptLines = Vec<Line<'static>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RawReasoningVisibility {
    Hidden,
    Visible,
}

pub(crate) async fn load_session_transcript(
    app_server: &mut AppServerSession,
    thread_id: ThreadId,
    raw_reasoning_visibility: RawReasoningVisibility,
) -> std::io::Result<TranscriptLines> {
    let thread = app_server
        .thread_read(thread_id, /*include_turns*/ true)
        .await
        .map_err(std::io::Error::other)?;
    Ok(thread_to_transcript_lines(
        &thread,
        raw_reasoning_visibility,
    ))
}

pub(crate) fn thread_to_transcript_lines(
    thread: &Thread,
    raw_reasoning_visibility: RawReasoningVisibility,
) -> TranscriptLines {
    let mut lines = Vec::new();
    for item in thread.turns.iter().flat_map(|turn| &turn.items) {
        let mut item_lines = item_lines(item, thread, raw_reasoning_visibility);
        if item_lines.is_empty() {
            continue;
        }
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.append(&mut item_lines);
    }

    if lines.is_empty() {
        lines.push("No transcript content available".italic().dim().into());
    }
    lines
}

fn item_lines(
    item: &ThreadItem,
    thread: &Thread,
    raw_reasoning_visibility: RawReasoningVisibility,
) -> TranscriptLines {
    match item {
        ThreadItem::UserMessage {
            id,
            client_id,
            content,
        } => {
            let item = UserMessageItem {
                id: id.clone(),
                client_id: client_id.clone(),
                content: content
                    .iter()
                    .cloned()
                    .map(codex_app_server_protocol::UserInput::into_core)
                    .collect(),
            };
            prefixed_text("You", item.message(), ratatui::style::Color::Cyan)
        }
        ThreadItem::AgentMessage { text, .. } => {
            let parsed = parse_assistant_markdown(text, &thread.cwd);
            if parsed.visible_markdown.trim().is_empty() {
                Vec::new()
            } else {
                let mut lines = vec!["Assistant".magenta().bold().into()];
                lines.extend(
                    render_markdown_text_with_width_and_cwd(
                        &parsed.visible_markdown,
                        /*width*/ None,
                        Some(&thread.cwd),
                    )
                    .lines,
                );
                lines
            }
        }
        ThreadItem::Plan { text, .. } => {
            prefixed_text("Plan", text.clone(), ratatui::style::Color::Green)
        }
        ThreadItem::Reasoning {
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
            if text.trim().is_empty() {
                Vec::new()
            } else {
                prefixed_text(
                    if header.is_empty() {
                        "Reasoning"
                    } else {
                        &header
                    },
                    text,
                    ratatui::style::Color::DarkGray,
                )
            }
        }
        ThreadItem::HookPrompt { fragments, .. } => fragments
            .iter()
            .map(|fragment| {
                vec![
                    "hook prompt: ".dim(),
                    fragment.text.trim().to_string().into(),
                ]
                .into()
            })
            .collect(),
        ThreadItem::CommandExecution {
            command,
            status,
            aggregated_output,
            exit_code,
            ..
        } => {
            let mut lines = vec![vec!["$ ".dim(), command.clone().into()].into()];
            lines.push(
                format!(
                    "status: {status:?}{}",
                    exit_code
                        .map(|code| format!(" · exit {code}"))
                        .unwrap_or_default()
                )
                .dim()
                .into(),
            );
            if let Some(output) = aggregated_output.as_deref()
                && !output.trim().is_empty()
            {
                lines.extend(
                    output
                        .lines()
                        .map(|line| line.trim_end().to_string().dim().into()),
                );
            }
            lines
        }
        ThreadItem::FileChange {
            changes, status, ..
        } => vec![
            format!("file changes: {status:?} · {} changes", changes.len())
                .dim()
                .into(),
        ],
        ThreadItem::McpToolCall {
            server,
            tool,
            status,
            ..
        } => vec![
            format!("mcp tool: {server}/{tool} · {status:?}")
                .dim()
                .into(),
        ],
        ThreadItem::DynamicToolCall {
            namespace,
            tool,
            status,
            ..
        } => {
            let name = namespace
                .as_ref()
                .map(|namespace| format!("{namespace}/{tool}"))
                .unwrap_or_else(|| tool.clone());
            vec![format!("tool: {name} · {status:?}").dim().into()]
        }
        ThreadItem::CollabAgentToolCall { tool, status, .. } => {
            vec![format!("agent tool: {tool:?} · {status:?}").dim().into()]
        }
        ThreadItem::SubAgentActivity {
            kind, agent_path, ..
        } => vec![format!("agent {agent_path}: {kind:?}").dim().into()],
        ThreadItem::WebSearch(item) => {
            vec![vec!["web search: ".dim(), item.query.clone().into()].into()]
        }
        ThreadItem::ImageView { path, .. } => {
            vec![format!("image: {}", path.render_for_ui()).dim().into()]
        }
        ThreadItem::ImageGeneration(item) => {
            let saved = item
                .saved_path
                .as_ref()
                .map(|path| format!(" · {}", path.as_path().display()))
                .unwrap_or_default();
            vec![
                format!("image generation: {}{saved}", item.status)
                    .dim()
                    .into(),
            ]
        }
        ThreadItem::EnteredReviewMode { review, .. } => {
            vec![vec!["review started: ".dim(), review.clone().into()].into()]
        }
        ThreadItem::ExitedReviewMode { review, .. } => {
            vec![vec!["review finished: ".dim(), review.clone().into()].into()]
        }
        ThreadItem::ContextCompaction { .. } => vec!["context compacted".dim().into()],
        ThreadItem::Sleep { duration_ms, .. } => {
            vec![format!("sleep: {duration_ms} ms").dim().into()]
        }
    }
}

fn prefixed_text(label: &str, text: String, color: ratatui::style::Color) -> TranscriptLines {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let mut lines = vec![label.to_string().fg(color).bold().into()];
    lines.extend(text.lines().map(|line| line.to_string().into()));
    lines
}
