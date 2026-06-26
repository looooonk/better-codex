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
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::RateLimitWindow;
use codex_app_server_protocol::TurnPlanStepStatus;
use codex_protocol::models::ManagedFileSystemPermissions;
use codex_protocol::models::PermissionProfile;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

const MOCHA_BASE: Color = Color::Black;
const MOCHA_MANTLE: Color = Color::DarkGray;
const MOCHA_SURFACE0: Color = Color::Gray;
const MOCHA_SURFACE1: Color = Color::DarkGray;
const MOCHA_TEXT: Color = Color::Reset;
const MOCHA_SUBTEXT0: Color = Color::Gray;
const MOCHA_OVERLAY0: Color = Color::DarkGray;
const DASHBOARD_COLLAPSE_WIDTH: u16 = 88;

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
        fill_rect(buf, area, MOCHA_BASE);
        let dashboard_collapsed = area.width < DASHBOARD_COLLAPSE_WIDTH;
        let horizontal = if dashboard_collapsed {
            vec![area].into()
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(area)
        };
        let header_height = if dashboard_collapsed { 3 } else { 2 };
        let main = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_height),
                Constraint::Min(5),
                Constraint::Length(4),
            ])
            .split(horizontal[0]);

        self.render_header(main[0], dashboard_collapsed, buf);
        self.render_transcript(main[1], buf);
        self.render_input(main[2], buf);
        if !dashboard_collapsed {
            self.render_dashboard(horizontal[1], buf);
        }
        self.render_command_palette(area, buf);
    }

    fn render_header(&self, area: Rect, dashboard_collapsed: bool, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_MANTLE);
        let mut lines = vec![Line::from("Better Codex".magenta().bold())];
        if dashboard_collapsed {
            lines.push(compact_dashboard_summary(self.shell));
        }
        Paragraph::new(lines)
            .style(pane_style(MOCHA_MANTLE))
            .render(area, buf);
    }

    fn render_transcript(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_BASE);
        let mut lines = Vec::new();
        let cwd = std::path::Path::new(&self.shell.cwd);
        for (index, line) in self.shell.transcript.iter().enumerate() {
            lines.extend(transcript_lines(
                line,
                area.width.saturating_sub(1),
                cwd,
                self.shell.transcript_selection == Some(index),
            ));
        }
        if !self.shell.streaming_plan.is_empty() {
            lines.extend(transcript_lines(
                &TranscriptLine::new(TranscriptKind::Plan, self.shell.streaming_plan.clone()),
                area.width.saturating_sub(1),
                cwd,
                /*selected*/ false,
            ));
        }
        if !self.shell.streaming_assistant.is_empty() {
            lines.extend(transcript_lines(
                &TranscriptLine::new(
                    TranscriptKind::Assistant,
                    self.shell.streaming_assistant.clone(),
                ),
                area.width.saturating_sub(1),
                cwd,
                /*selected*/ false,
            ));
        }
        let visible_count = area.height.saturating_sub(1) as usize;
        let max_scroll = lines.len().saturating_sub(visible_count);
        self.shell.transcript_scroll_max.set(max_scroll);
        let scroll = self.shell.transcript_scroll.min(max_scroll);
        let visible_from = lines.len().saturating_sub(visible_count + scroll);
        let title = if let Some(selected) = self.shell.transcript_selection {
            format!(
                "Conversation select {}/{}",
                selected.saturating_add(1),
                self.shell.transcript.len()
            )
        } else if scroll == 0 {
            "Conversation".to_string()
        } else {
            format!("Conversation +{scroll}")
        };
        let visible_hyperlink_lines = lines.into_iter().skip(visible_from).collect::<Vec<_>>();
        let visible_lines = visible_lines(visible_hyperlink_lines.clone());
        Paragraph::new(Line::from(title.bold()))
            .style(pane_style(MOCHA_BASE))
            .render(title_rect(area), buf);
        let body = body_rect_after_title(area);
        Paragraph::new(visible_lines)
            .style(pane_style(MOCHA_BASE))
            .wrap(Wrap { trim: false })
            .render(body, buf);
        mark_buffer_hyperlinks(buf, body, &visible_hyperlink_lines, /*scroll_rows*/ 0);
    }

    fn render_input(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_SURFACE0);
        if let Some(pending) = &self.shell.pending_approval {
            self.render_titled_panel(
                area,
                "Approval",
                approval_lines(pending),
                MOCHA_SURFACE0,
                buf,
            );
            return;
        }
        if let Some(pending) = &self.shell.pending_elicitation {
            self.render_titled_panel(
                area,
                "MCP Elicitation",
                elicitation_lines(pending),
                MOCHA_SURFACE0,
                buf,
            );
            return;
        }
        if let Some(pending) = &self.shell.pending_user_input {
            self.render_titled_panel(
                area,
                "Tool Input",
                user_input_lines(
                    pending,
                    self.shell.composer.text(),
                    self.shell.composer.is_empty(),
                ),
                MOCHA_SURFACE0,
                buf,
            );
            return;
        }

        let (line, column) = self.shell.composer.cursor_position();
        let title = if self.shell.active_turn_id.is_some() {
            format!("Composer busy {}:{}", line + 1, column + 1)
        } else {
            format!("Composer ready {}:{}", line + 1, column + 1)
        };
        self.render_titled_panel(
            area,
            &title,
            composer_lines(
                self.shell.composer.text(),
                self.shell.composer.cursor(),
                self.shell.composer.is_empty(),
            ),
            MOCHA_SURFACE0,
            buf,
        );
    }

    fn render_dashboard(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_MANTLE);
        let context_window = self
            .shell
            .model_context_window
            .map(|window| window.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let mut lines = vec![
            Line::from("Dashboard".bold()),
            Line::from(""),
            Line::from("Status".bold()),
            status_line(&self.shell.status),
        ];
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

        if !self.shell.rate_limits.is_empty() || self.shell.rate_limit_reset_credits.is_some() {
            lines.push(Line::from(""));
            lines.push(Line::from("Rate Limits".bold()));
            for limit in self.shell.rate_limits.iter().take(2) {
                lines.extend(rate_limit_lines(limit));
            }
            if self.shell.rate_limits.len() > 2 {
                lines.push(Line::from(
                    format!("+{} more", self.shell.rate_limits.len() - 2).dim(),
                ));
            }
            if let Some(credits) = self.shell.rate_limit_reset_credits {
                lines.push(Line::from(format!("reset credits {credits}").dim()));
            }
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
        if let Some(git_status) = &self.shell.workspace_git_status {
            if let Some(branch) = &git_status.branch {
                lines.push(Line::from(vec![
                    "branch ".dim(),
                    truncate_text(branch, /*max_chars*/ 16).cyan(),
                ]));
            }
            if git_status.is_dirty() {
                lines.push(Line::from(format!(
                    "changes {} files",
                    git_status.changes.total()
                )));
                lines.extend(workspace_change_lines(&git_status.changes));
            } else {
                lines.push(Line::from("tree clean".green()));
            }
        }
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
        if self.shell.transcript_selection.is_some() {
            lines.push(Line::from("Up/Down select"));
            lines.push(Line::from("Enter copy"));
            lines.push(Line::from("Esc composer"));
        } else if self.shell.active_turn_id.is_some() {
            lines.push(Line::from("Enter steer"));
            lines.push(Line::from("Ctrl+C interrupt, Esc exit"));
            lines.push(Line::from("Alt+Up select, Ctrl+O copy"));
        } else {
            lines.push(Line::from("Enter send"));
            lines.push(Line::from("Ctrl+C/Esc exit"));
            lines.push(Line::from("Alt+Up select, Ctrl+O copy"));
        }
        Paragraph::new(lines)
            .style(pane_style(MOCHA_MANTLE))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }

    fn render_titled_panel(
        &self,
        area: Rect,
        title: &str,
        lines: Vec<Line<'static>>,
        background: Color,
        buf: &mut Buffer,
    ) {
        Paragraph::new(Line::from(title.to_string().bold()))
            .style(pane_style(background))
            .render(title_rect(area), buf);
        Paragraph::new(lines)
            .style(pane_style(background))
            .wrap(Wrap { trim: false })
            .render(body_rect_after_title(area), buf);
    }

    fn render_command_palette(&self, area: Rect, buf: &mut Buffer) {
        let Some(palette) = &self.shell.command_palette else {
            return;
        };
        let entries = self.shell.command_palette_entries();
        let palette_area = centered_band_rect(area, /*height*/ 15);
        Clear.render(palette_area, buf);

        let mut lines = Vec::new();
        for (index, entry) in entries.iter().take(11).enumerate() {
            let selected = index == palette.selected();
            let marker = if selected {
                ">".cyan().bold()
            } else {
                " ".into()
            };
            let title = if entry.enabled {
                entry.title.to_string().into()
            } else {
                entry.title.to_string().dim()
            };
            let detail = if selected {
                format!(" - {}", truncate_text(entry.detail, /*max_chars*/ 28)).dim()
            } else {
                String::new().into()
            };
            let line = Line::from(vec![marker, " ".dim(), title, detail]);
            if selected {
                lines.push(line.style(Style::new().bg(MOCHA_SURFACE1)));
            } else {
                lines.push(line);
            }
        }

        fill_rect(buf, palette_area, MOCHA_SURFACE0);
        let mut palette_lines = vec![Line::from("Command Palette".bold()), Line::from("")];
        palette_lines.extend(lines);
        Paragraph::new(palette_lines)
            .style(pane_style(MOCHA_SURFACE0))
            .wrap(Wrap { trim: true })
            .render(palette_area, buf);
    }
}

fn fill_rect(buf: &mut Buffer, area: Rect, color: Color) {
    let style = pane_style(color);
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

fn pane_style(color: Color) -> Style {
    Style::new().fg(MOCHA_TEXT).bg(color)
}

fn title_rect(area: Rect) -> Rect {
    Rect::new(area.x, area.y, area.width, area.height.min(1))
}

fn body_rect_after_title(area: Rect) -> Rect {
    Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    )
}

fn compact_dashboard_summary(shell: &ShellState) -> Line<'static> {
    let token_total = shell.token_usage.total_tokens;
    Line::from(vec![
        "Dashboard ".cyan().bold(),
        status_span(&shell.status),
        " · ".fg(MOCHA_OVERLAY0),
        compact_dashboard_text(&shell.model).into(),
        " · ".fg(MOCHA_OVERLAY0),
        format!("{token_total} tokens").fg(MOCHA_SUBTEXT0),
    ])
}

fn centered_band_rect(area: Rect, height: u16) -> Rect {
    let available_height = area.height.saturating_sub(4);
    let height = height.min(available_height).max(available_height.min(5));
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(area.x, y, area.width, height)
}

fn transcript_lines(
    line: &TranscriptLine,
    width: u16,
    cwd: &std::path::Path,
    selected: bool,
) -> Vec<HyperlinkLine> {
    let width = usize::from(width).max(12);
    let label = line.kind.label();
    let style = match line.kind {
        TranscriptKind::System => LineStyle::Dim,
        TranscriptKind::User => LineStyle::Cyan,
        TranscriptKind::Assistant => LineStyle::Magenta,
        TranscriptKind::Plan => LineStyle::Green,
        TranscriptKind::Tool => LineStyle::Cyan,
        TranscriptKind::Diff => LineStyle::Green,
        TranscriptKind::Output => LineStyle::Dim,
        TranscriptKind::Status => LineStyle::Dim,
        TranscriptKind::Audit => LineStyle::Cyan,
        TranscriptKind::Error => LineStyle::Red,
    };

    let prefix_width = label.len() + if selected { 4 } else { 2 };
    let body_width = width.saturating_sub(prefix_width).max(1);
    let initial_prefix = style.label_prefix(label, selected);
    let subsequent_prefix = " ".repeat(prefix_width).into();

    let mut rendered_lines =
        if matches!(line.kind, TranscriptKind::Assistant | TranscriptKind::Plan) {
            let rendered = markdown::render_markdown_agent_with_links_and_cwd(
                &line.text,
                Some(body_width),
                Some(cwd),
            )
            .into_iter()
            .map(|line| line.style(style.line_style()))
            .collect();
            prefix_hyperlink_lines(rendered, initial_prefix, subsequent_prefix)
        } else {
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
        };

    if selected {
        let selection_style = Style::new().bg(Color::DarkGray);
        rendered_lines = rendered_lines
            .into_iter()
            .map(|line| line.style(selection_style))
            .collect();
    }
    rendered_lines
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
    fn label_prefix(self, text: &str, selected: bool) -> Span<'static> {
        if selected {
            self.label(format!("> {text}: "))
        } else {
            self.label(format!("{text}: "))
        }
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
    Line::from(status_span(status))
}

fn status_span(status: &str) -> Span<'static> {
    match status {
        "ready" => status.to_string().green(),
        "failed" | "error" | "disconnected" => status.to_string().red(),
        "thinking" | "reasoning" | "retrying" => status.to_string().cyan(),
        "interrupted" => status.to_string().magenta(),
        _ => status.to_string().into(),
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

fn rate_limit_lines(limit: &RateLimitSnapshot) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let label = rate_limit_label(limit);
    if let Some(primary) = &limit.primary {
        lines.push(rate_limit_window_line(&label, primary));
    } else {
        lines.push(Line::from(label));
    }
    if let Some(secondary) = &limit.secondary {
        lines.push(rate_limit_window_line("  secondary", secondary));
    }
    if let Some(reached) = limit.rate_limit_reached_type {
        lines.push(Line::from(format!("limited {reached:?}").red()));
    }
    if let Some(credits) = &limit.credits {
        if credits.unlimited {
            lines.push(Line::from("credits unlimited".green()));
        } else if let Some(balance) = &credits.balance {
            lines.push(Line::from(format!("credits {balance}").dim()));
        } else if !credits.has_credits {
            lines.push(Line::from("credits depleted".red()));
        }
    }
    if let Some(individual_limit) = &limit.individual_limit {
        lines.push(Line::from(format!(
            "spend {}% left",
            individual_limit.remaining_percent
        )));
    }
    lines
}

fn rate_limit_label(limit: &RateLimitSnapshot) -> String {
    limit
        .limit_name
        .as_deref()
        .or(limit.limit_id.as_deref())
        .map(|label| truncate_text(label, /*max_chars*/ 12))
        .unwrap_or_else(|| "account".to_string())
}

fn rate_limit_window_line(label: &str, window: &RateLimitWindow) -> Line<'static> {
    let percent = format!("{}%", window.used_percent);
    let percent = if window.used_percent >= 90 {
        percent.red()
    } else if window.used_percent >= 75 {
        percent.magenta()
    } else if window.used_percent >= 50 {
        percent.cyan()
    } else {
        percent.green()
    };
    let mut spans = vec![label.to_string().into(), " ".dim(), percent];
    if let Some(duration) = window.window_duration_mins {
        spans.extend([" ".dim(), format!("{duration}m").dim()]);
    }
    Line::from(spans)
}

fn workspace_change_lines(
    changes: &super::workspace::WorkspaceChangeSummary,
) -> Vec<Line<'static>> {
    [
        ("added", changes.added),
        ("modified", changes.modified),
        ("deleted", changes.deleted),
        ("renamed", changes.renamed),
        ("conflicted", changes.conflicted),
        ("untracked", changes.untracked),
    ]
    .into_iter()
    .filter(|(_label, count)| *count > 0)
    .map(|(label, count)| Line::from(format!("  {label} {count}").dim()))
    .collect()
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
