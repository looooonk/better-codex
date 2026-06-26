use super::ShellState;
use super::ToolActivity;
use super::TranscriptKind;
use super::TranscriptLine;
use crate::markdown;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::mark_buffer_hyperlinks;
use crate::terminal_hyperlinks::prefix_hyperlink_lines;
use crate::terminal_hyperlinks::visible_lines;
use crate::text_formatting::truncate_text;
use crate::tui;
use codex_app_server_protocol::TurnPlanStepStatus;
use codex_protocol::models::ManagedFileSystemPermissions;
use codex_protocol::models::PermissionProfile;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

pub(super) fn draw_shell(tui: &mut tui::Tui, shell: &ShellState) -> std::io::Result<()> {
    let height = tui.terminal.size()?.height;
    tui.draw(height, |frame| {
        ShellView { shell }.render(frame.area(), frame.buffer);
    })
}

pub(super) struct ShellView<'a> {
    pub(super) shell: &'a ShellState,
}

impl ShellView<'_> {
    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(48), Constraint::Length(30)])
            .split(area);
        let main = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(4),
            ])
            .split(horizontal[0]);

        self.render_header(main[0], buf);
        self.render_transcript(main[1], buf);
        self.render_input(main[2], buf);
        self.render_dashboard(horizontal[1], buf);
    }

    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        let title = vec![
            "Better Codex".magenta().bold(),
            "  ".into(),
            self.shell.status.clone().cyan(),
        ];
        Paragraph::new(Line::from(title))
            .block(Block::default().borders(Borders::BOTTOM))
            .render(area, buf);
    }

    fn render_transcript(&self, area: Rect, buf: &mut Buffer) {
        let mut lines = Vec::new();
        let cwd = std::path::Path::new(&self.shell.cwd);
        for line in &self.shell.transcript {
            lines.extend(transcript_lines(line, area.width.saturating_sub(2), cwd));
        }
        if !self.shell.streaming_plan.is_empty() {
            lines.extend(transcript_lines(
                &TranscriptLine::new(TranscriptKind::Plan, self.shell.streaming_plan.clone()),
                area.width.saturating_sub(2),
                cwd,
            ));
        }
        if !self.shell.streaming_assistant.is_empty() {
            lines.extend(transcript_lines(
                &TranscriptLine::new(
                    TranscriptKind::Assistant,
                    self.shell.streaming_assistant.clone(),
                ),
                area.width.saturating_sub(2),
                cwd,
            ));
        }
        let visible_count = area.height.saturating_sub(2) as usize;
        let max_scroll = lines.len().saturating_sub(visible_count);
        self.shell.transcript_scroll_max.set(max_scroll);
        let scroll = self.shell.transcript_scroll.min(max_scroll);
        let visible_from = lines.len().saturating_sub(visible_count + scroll);
        let title = if scroll == 0 {
            "Conversation".to_string()
        } else {
            format!("Conversation +{scroll}")
        };
        let visible_hyperlink_lines = lines.into_iter().skip(visible_from).collect::<Vec<_>>();
        let visible_lines = visible_lines(visible_hyperlink_lines.clone());
        Paragraph::new(visible_lines)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
            .render(area, buf);
        mark_buffer_hyperlinks(
            buf,
            inner_rect(area),
            &visible_hyperlink_lines,
            /*scroll_rows*/ 0,
        );
    }

    fn render_input(&self, area: Rect, buf: &mut Buffer) {
        if let Some(pending) = &self.shell.pending_approval {
            Paragraph::new(approval_lines(pending))
                .block(Block::default().borders(Borders::TOP).title("Approval"))
                .wrap(Wrap { trim: false })
                .render(area, buf);
            return;
        }
        if let Some(pending) = &self.shell.pending_elicitation {
            Paragraph::new(elicitation_lines(pending))
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .title("MCP Elicitation"),
                )
                .wrap(Wrap { trim: false })
                .render(area, buf);
            return;
        }
        if let Some(pending) = &self.shell.pending_user_input {
            Paragraph::new(user_input_lines(
                pending,
                self.shell.composer.text(),
                self.shell.composer.is_empty(),
            ))
            .block(Block::default().borders(Borders::TOP).title("Tool Input"))
            .wrap(Wrap { trim: false })
            .render(area, buf);
            return;
        }

        let (line, column) = self.shell.composer.cursor_position();
        let title = if self.shell.active_turn_id.is_some() {
            format!("Composer busy {}:{}", line + 1, column + 1)
        } else {
            format!("Composer ready {}:{}", line + 1, column + 1)
        };
        Paragraph::new(composer_lines(
            self.shell.composer.text(),
            self.shell.composer.cursor(),
            self.shell.composer.is_empty(),
        ))
        .block(Block::default().borders(Borders::TOP).title(title))
        .wrap(Wrap { trim: false })
        .render(area, buf);
    }

    fn render_dashboard(&self, area: Rect, buf: &mut Buffer) {
        let context_window = self
            .shell
            .model_context_window
            .map(|window| window.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let mut lines = vec![Line::from("Status".bold()), status_line(&self.shell.status)];
        if let Some(active_turn_id) = &self.shell.active_turn_id {
            lines.push(Line::from(vec![
                "turn ".dim(),
                short_id(active_turn_id).cyan(),
            ]));
        }
        lines.extend([
            Line::from(""),
            Line::from("Model".bold()),
            Line::from(self.shell.model.clone()),
        ]);
        if let Some(reasoning_effort) = &self.shell.reasoning_effort {
            lines.push(Line::from(format!("reasoning {reasoning_effort}").dim()));
        }
        if let Some(service_tier) = self
            .shell
            .service_tier
            .as_deref()
            .filter(|service_tier| !service_tier.trim().is_empty())
        {
            lines.push(Line::from(vec![
                "tier ".dim(),
                compact_dashboard_text(service_tier).into(),
            ]));
        }
        lines.extend([
            Line::from(""),
            Line::from("Thread".bold()),
            Line::from(short_id(&self.shell.thread_id.to_string())),
            Line::from(""),
            Line::from("Tokens".bold()),
            Line::from(format!("total {}", self.shell.token_usage.total_tokens)),
            Line::from(format!("input {}", self.shell.token_usage.input_tokens)),
            Line::from(format!("output {}", self.shell.token_usage.output_tokens)),
            Line::from(format!("context {context_window}")),
        ]);
        if let Some(context_used_percent) =
            context_used_percent(&self.shell.token_usage, self.shell.model_context_window)
                .filter(|percent| *percent >= 50)
        {
            let line = format!("context {context_used_percent}% used");
            let line = if context_used_percent >= 90 {
                line.red()
            } else if context_used_percent >= 75 {
                line.magenta()
            } else {
                line.cyan()
            };
            lines.push(Line::from(line));
        }

        lines.push(Line::from(""));
        lines.push(Line::from("Diff".bold()));
        if let Some(diff) = &self.shell.latest_diff {
            lines.push(Line::from(format!(
                "{} files +{} -{}",
                diff.files, diff.additions, diff.removals
            )));
        } else {
            lines.push(Line::from("no changes".dim()));
        }

        lines.push(Line::from(""));
        lines.push(Line::from("Plan".bold()));
        if let Some(explanation) = &self.shell.plan_explanation {
            lines.push(Line::from(explanation.clone().dim()));
        }
        if self.shell.plan_steps.is_empty() {
            lines.push(Line::from("no active plan".dim()));
        } else {
            for step in self.shell.plan_steps.iter().take(5) {
                lines.push(plan_step_line(step.status, &step.step));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from("Tools".bold()));
        if self.shell.tool_activity.is_empty() {
            lines.push(Line::from("idle".dim()));
        } else {
            for activity in self.shell.tool_activity.iter().rev().take(4).rev() {
                lines.push(tool_activity_line(activity));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from("Workspace".bold()));
        lines.push(Line::from(vec![
            "cwd ".dim(),
            compact_dashboard_text(&self.shell.cwd).into(),
        ]));
        match &self.shell.permission_profile {
            PermissionProfile::Managed {
                file_system,
                network,
            } => {
                let file_system_label = match file_system {
                    ManagedFileSystemPermissions::Restricted { .. } => "restricted",
                    ManagedFileSystemPermissions::Unrestricted => "unrestricted",
                };
                lines.push(Line::from(vec!["profile ".dim(), "managed".into()]));
                lines.push(Line::from(format!(
                    "files {file_system_label}, net {network}"
                )));
            }
            PermissionProfile::Disabled => {
                lines.push(Line::from(vec!["profile ".dim(), "full access".into()]));
            }
            PermissionProfile::External { network } => {
                lines.push(Line::from(vec!["profile ".dim(), "external".into()]));
                lines.push(Line::from(format!("net {network}")));
            }
        }
        if self.shell.runtime_workspace_roots.is_empty() {
            lines.push(Line::from("roots none selected".dim()));
        } else {
            const WORKSPACE_ROOT_PREVIEW_LIMIT: usize = 3;
            let root_count = self.shell.runtime_workspace_roots.len();
            lines.push(Line::from(format!("roots {root_count} writable")));
            for root in self
                .shell
                .runtime_workspace_roots
                .iter()
                .take(WORKSPACE_ROOT_PREVIEW_LIMIT)
            {
                lines.push(Line::from(vec![
                    "  ".dim(),
                    compact_dashboard_text(&root.display().to_string()).dim(),
                ]));
            }
            let hidden = root_count.saturating_sub(WORKSPACE_ROOT_PREVIEW_LIMIT);
            if hidden > 0 {
                lines.push(Line::from(format!("  +{hidden} more").dim()));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from("Keys".bold()));
        if self.shell.active_turn_id.is_some() {
            lines.push(Line::from("Enter steer"));
            lines.push(Line::from("Ctrl+C interrupt"));
        } else {
            lines.push(Line::from("Enter send"));
            lines.push(Line::from("Ctrl+C exit"));
        }
        lines.push(Line::from("Esc exit"));
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::LEFT).title("Dashboard"))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

fn transcript_lines(
    line: &TranscriptLine,
    width: u16,
    cwd: &std::path::Path,
) -> Vec<HyperlinkLine> {
    let width = usize::from(width).max(12);
    let (label, style): (&str, LineStyle) = match line.kind {
        TranscriptKind::System => ("system", LineStyle::Dim),
        TranscriptKind::User => ("you", LineStyle::Cyan),
        TranscriptKind::Assistant => ("codex", LineStyle::Magenta),
        TranscriptKind::Plan => ("plan", LineStyle::Green),
        TranscriptKind::Tool => ("tool", LineStyle::Cyan),
        TranscriptKind::Diff => ("diff", LineStyle::Green),
        TranscriptKind::Output => ("output", LineStyle::Dim),
        TranscriptKind::Status => ("status", LineStyle::Dim),
        TranscriptKind::Audit => ("audit", LineStyle::Cyan),
        TranscriptKind::Error => ("error", LineStyle::Red),
    };

    let prefix_width = label.len() + 2;
    let body_width = width.saturating_sub(prefix_width).max(1);
    let initial_prefix = style.label_prefix(label);
    let subsequent_prefix = " ".repeat(prefix_width).into();

    if matches!(line.kind, TranscriptKind::Assistant | TranscriptKind::Plan) {
        let rendered: Vec<HyperlinkLine> = markdown::render_markdown_agent_with_links_and_cwd(
            &line.text,
            Some(body_width),
            Some(cwd),
        )
        .into_iter()
        .map(|line| line.style(style.line_style()))
        .collect();
        return prefix_hyperlink_lines(rendered, initial_prefix, subsequent_prefix);
    }

    let options = textwrap::Options::new(body_width);
    let wrapped_lines: Vec<HyperlinkLine> = textwrap::wrap(&line.text, options)
        .into_iter()
        .map(|wrapped| {
            HyperlinkLine::new(
                Line::from(style.text(wrapped.into_owned())).style(style.line_style()),
            )
        })
        .collect();
    prefix_hyperlink_lines(wrapped_lines, initial_prefix, subsequent_prefix)
}

fn inner_rect(area: Rect) -> Rect {
    let width = area.width.saturating_sub(2);
    let height = area.height.saturating_sub(2);
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        width,
        height,
    )
}

#[derive(Debug, Clone, Copy)]
enum LineStyle {
    Cyan,
    Dim,
    Green,
    Magenta,
    Red,
}

impl LineStyle {
    fn label_prefix(self, text: &str) -> Span<'static> {
        self.label(format!("{text}: "))
    }

    fn label(self, text: String) -> Span<'static> {
        match self {
            Self::Cyan => text.cyan().bold(),
            Self::Dim => text.dim().bold(),
            Self::Green => text.green().bold(),
            Self::Magenta => text.magenta().bold(),
            Self::Red => text.red().bold(),
        }
    }

    fn text(self, text: String) -> Span<'static> {
        match self {
            Self::Cyan => text.into(),
            Self::Dim => text.dim(),
            Self::Green => text.green(),
            Self::Magenta => text.into(),
            Self::Red => text.red(),
        }
    }

    fn line_style(self) -> Style {
        match self {
            Self::Cyan | Self::Magenta => Style::new(),
            Self::Dim => Style::new().dim(),
            Self::Green => Style::new().green(),
            Self::Red => Style::new().red(),
        }
    }
}

fn status_line(status: &str) -> Line<'static> {
    match status {
        "ready" => Line::from(status.to_string().green()),
        "failed" | "error" | "disconnected" => Line::from(status.to_string().red()),
        "thinking" | "reasoning" | "retrying" => Line::from(status.to_string().cyan()),
        "interrupted" => Line::from(status.to_string().magenta()),
        _ => Line::from(status.to_string()),
    }
}

fn plan_step_line(status: TurnPlanStepStatus, step: &str) -> Line<'static> {
    let marker = match status {
        TurnPlanStepStatus::Pending => "-".dim(),
        TurnPlanStepStatus::InProgress => ">".cyan().bold(),
        TurnPlanStepStatus::Completed => "x".green(),
    };
    Line::from(vec![marker, " ".dim(), step.to_string().into()])
}

fn tool_activity_line(activity: &ToolActivity) -> Line<'static> {
    let status = match activity.status.as_str() {
        "completed" => activity.status.clone().green(),
        "failed" | "declined" => activity.status.clone().red(),
        "in progress" | "inprogress" => activity.status.clone().cyan(),
        _ => activity.status.clone().dim(),
    };
    Line::from(vec![
        status,
        " ".dim(),
        compact_dashboard_text(&activity.title).into(),
    ])
}

fn short_id(id: &str) -> String {
    id.get(..8)
        .map(|prefix| format!("{prefix}..."))
        .unwrap_or_else(|| id.to_string())
}

fn compact_dashboard_text(text: &str) -> String {
    const MAX_CHARS: usize = 24;
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    let mut compact = text.chars().take(MAX_CHARS).collect::<String>();
    compact.push_str("...");
    compact
}

pub(super) fn context_used_percent(
    usage: &crate::token_usage::TokenUsage,
    model_context_window: Option<i64>,
) -> Option<i64> {
    let context_window = model_context_window.filter(|window| *window > 0)?;
    Some(100 - usage.percent_of_context_window_remaining(context_window))
}

fn composer_lines(text: &str, cursor: usize, is_empty: bool) -> Vec<Line<'static>> {
    if is_empty {
        return vec![Line::from(vec![
            "> ".cyan(),
            "Type a message, Shift+Enter for newline".dim(),
        ])];
    }

    let mut lines = Vec::new();
    let mut offset = 0usize;
    for (index, logical_line) in text.split('\n').enumerate() {
        let end = offset + logical_line.len();
        let prefix = if index == 0 { "> ".cyan() } else { "  ".dim() };
        lines.push(Line::from(composer_line_spans(
            prefix,
            logical_line,
            cursor,
            offset,
            end,
        )));
        offset = end + 1;
    }
    lines
}

fn approval_lines(pending: &super::PendingApproval) -> Vec<Line<'static>> {
    vec![
        Line::from(vec!["? ".cyan().bold(), pending.title().to_string().bold()]),
        Line::from(vec!["  ".into(), pending.detail().to_string().dim()]),
        Line::from(vec![
            "  ".into(),
            "a".green().bold(),
            " approve  ".dim(),
            "d".red().bold(),
            " deny".dim(),
        ]),
    ]
}

fn user_input_lines(
    pending: &super::PendingUserInput,
    composer_text: &str,
    is_empty: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let (current, total) = pending.question_position();
    lines.push(Line::from(vec![
        "? ".cyan().bold(),
        format!("{} ({current}/{total})", pending.title()).bold(),
    ]));

    if let Some(question) = pending.current_question() {
        lines.push(Line::from(vec![
            "  ".into(),
            question.header.clone().bold(),
            ": ".dim(),
            question.question.clone().into(),
        ]));
    }

    let secret = pending
        .current_question()
        .is_some_and(|question| question.is_secret);
    let answer = if is_empty {
        "answer".dim()
    } else if secret {
        "[hidden]".dim()
    } else {
        composer_text.to_string().into()
    };
    let mut answer_line = vec!["> ".cyan().bold(), answer];
    if let Some(question) = pending.current_question()
        && let Some(options) = question.options.as_ref()
    {
        answer_line.push("  ".dim());
        answer_line.extend(
            options
                .iter()
                .take(3)
                .enumerate()
                .flat_map(|(index, option)| {
                    vec![
                        format!("{} ", index + 1).green().bold(),
                        option.label.clone().dim(),
                        "  ".dim(),
                    ]
                }),
        );
    }
    lines.push(Line::from(answer_line));
    lines
}

fn elicitation_lines(pending: &super::PendingElicitation) -> Vec<Line<'static>> {
    let mut action_line = vec!["  ".into()];
    if pending.can_accept() {
        action_line.extend(["a".green().bold(), " accept  ".dim()]);
    }
    action_line.extend([
        "d".red().bold(),
        " decline  ".dim(),
        "c".bold(),
        " cancel".dim(),
    ]);

    vec![
        Line::from(vec!["? ".cyan().bold(), pending.title().to_string().bold()]),
        Line::from(vec!["  ".into(), truncate_text(pending.detail(), 62).dim()]),
        Line::from(action_line),
    ]
}

fn composer_line_spans(
    prefix: Span<'static>,
    text: &str,
    cursor: usize,
    start: usize,
    end: usize,
) -> Vec<Span<'static>> {
    let mut spans = vec![prefix];
    if !(start..=end).contains(&cursor) {
        spans.push(text.to_string().into());
        return spans;
    }

    let cursor_offset = cursor.saturating_sub(start);
    let before = &text[..cursor_offset.min(text.len())];
    let after = &text[cursor_offset.min(text.len())..];
    if !before.is_empty() {
        spans.push(before.to_string().into());
    }
    if let Some(ch) = after.chars().next() {
        spans.push(ch.to_string().reversed());
        let rest_start = ch.len_utf8();
        if rest_start < after.len() {
            spans.push(after[rest_start..].to_string().into());
        }
    } else {
        spans.push(" ".to_string().reversed());
    }
    spans
}
