use super::ShellState;
use super::ToolActivity;
use super::TranscriptKind;
use super::TranscriptLine;
use crate::goal_display::format_goal_elapsed_seconds;
use crate::goal_display::goal_status_label;
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

const MOCHA_BASE: Color = Color::Rgb(30, 30, 46);
const MOCHA_MANTLE: Color = Color::Rgb(24, 24, 37);
const MOCHA_SURFACE0: Color = Color::Rgb(49, 50, 68);
const MOCHA_SURFACE1: Color = Color::Rgb(69, 71, 90);
const MOCHA_TEXT: Color = Color::Rgb(205, 214, 244);
const MOCHA_SUBTEXT0: Color = Color::Rgb(166, 173, 200);
const MOCHA_OVERLAY0: Color = Color::Rgb(108, 112, 134);
const DASHBOARD_COLLAPSE_WIDTH: u16 = 88;
const PANE_PADDING: u16 = 1;
const DASHBOARD_PANEL_GAP: u16 = 1;

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
        let header_height = if dashboard_collapsed { 4 } else { 3 };
        let main = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_height),
                Constraint::Min(5),
                Constraint::Length(6),
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
        let content = pane_content_rect(area);
        let mut lines = vec![Line::from("Better Codex".magenta().bold())];
        if dashboard_collapsed {
            lines.push(compact_dashboard_summary(self.shell));
        }
        Paragraph::new(lines)
            .style(pane_style(MOCHA_MANTLE))
            .render(content, buf);
    }

    fn render_transcript(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_BASE);
        let content = pane_content_rect(area);
        let body = body_rect_after_title(content);
        let mut lines = Vec::new();
        let cwd = std::path::Path::new(&self.shell.cwd);
        let transcript_width = body.width;
        let mut previous_kind = None;
        for (index, line) in self.shell.transcript.iter().enumerate() {
            push_transcript_lines(
                &mut lines,
                &mut previous_kind,
                line,
                transcript_width,
                cwd,
                self.shell.transcript_selection == Some(index),
            );
        }
        if !self.shell.streaming_plan.is_empty() {
            push_transcript_lines(
                &mut lines,
                &mut previous_kind,
                &TranscriptLine::new(TranscriptKind::Plan, self.shell.streaming_plan.clone()),
                transcript_width,
                cwd,
                /*selected*/ false,
            );
        }
        if !self.shell.streaming_assistant.is_empty() {
            push_transcript_lines(
                &mut lines,
                &mut previous_kind,
                &TranscriptLine::new(
                    TranscriptKind::Assistant,
                    self.shell.streaming_assistant.clone(),
                ),
                transcript_width,
                cwd,
                /*selected*/ false,
            );
        }
        let visible_count = usize::from(body.height);
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
            .render(title_rect(content), buf);
        Paragraph::new(visible_lines)
            .style(pane_style(MOCHA_BASE))
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
        let content = pane_content_rect(area);
        let width = usize::from(content.width);
        let context_window = self
            .shell
            .model_context_window
            .map(format_i64)
            .unwrap_or_else(|| "unknown".to_string());
        let mut panels = vec![DashboardPanel::new(
            "Status",
            vec![status_line(&self.shell.status)],
        )];
        if let Some(active_turn_id) = &self.shell.active_turn_id {
            panels[0].lines.push(Line::from(vec![
                "turn ".dim(),
                short_id(active_turn_id).cyan(),
            ]));
        }
        let mut model_lines = vec![Line::from(dashboard_value(
            &self.shell.model,
            width,
            /*prefix_width*/ 0,
        ))];
        if let Some(reasoning_effort) = &self.shell.reasoning_effort {
            model_lines.push(Line::from(format!("reasoning {reasoning_effort}").dim()));
        }
        if let Some(service_tier) = self
            .shell
            .service_tier
            .as_deref()
            .filter(|service_tier| !service_tier.trim().is_empty())
        {
            model_lines.push(Line::from(vec![
                "tier ".dim(),
                dashboard_value(service_tier, width, /*prefix_width*/ 5).into(),
            ]));
        }
        panels.push(DashboardPanel::new("Model", model_lines));
        let mut token_lines = vec![
            Line::from(format!(
                "total {}",
                format_i64(self.shell.token_usage.total_tokens)
            )),
            Line::from(format!(
                "input {}",
                format_i64(self.shell.token_usage.input_tokens)
            )),
            Line::from(format!(
                "output {}",
                format_i64(self.shell.token_usage.output_tokens)
            )),
            Line::from(format!("context {context_window}")),
        ];
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
            token_lines.push(Line::from(line));
        }
        panels.push(DashboardPanel::new("Tokens", token_lines));

        if !self.shell.rate_limits.is_empty() || self.shell.rate_limit_reset_credits.is_some() {
            let mut limit_lines = Vec::new();
            for limit in self.shell.rate_limits.iter().take(2) {
                limit_lines.extend(rate_limit_lines(limit, width));
            }
            if self.shell.rate_limits.len() > 2 {
                limit_lines.push(Line::from(
                    format!("+{} more", format_usize(self.shell.rate_limits.len() - 2)).dim(),
                ));
            }
            if let Some(credits) = self.shell.rate_limit_reset_credits {
                limit_lines.push(Line::from(
                    format!("reset credits {}", format_i64(credits)).dim(),
                ));
            }
            panels.push(DashboardPanel::new("Rate Limits", limit_lines));
        }

        let diff_lines = if let Some(diff) = &self.shell.latest_diff {
            vec![Line::from(format!(
                "{} files +{} -{}",
                format_usize(diff.files),
                format_usize(diff.additions),
                format_usize(diff.removals)
            ))]
        } else {
            vec![Line::from("no changes".dim())]
        };
        panels.push(DashboardPanel::new("Diff", diff_lines));

        let mut plan_lines = Vec::new();
        if let Some(goal) = &self.shell.active_goal {
            plan_lines.push(Line::from(vec![
                "goal ".dim(),
                goal_status_span(goal.status),
            ]));
            plan_lines.push(Line::from(format!(
                "  {}",
                dashboard_value(&goal.objective, width, /*prefix_width*/ 2)
            )));
            let mut usage = Vec::new();
            if goal.time_used_seconds > 0 {
                usage.push(format_goal_elapsed_seconds(goal.time_used_seconds));
            }
            if let Some(token_budget) = goal.token_budget {
                usage.push(format!(
                    "{}/{} tokens",
                    format_i64(goal.tokens_used),
                    format_i64(token_budget)
                ));
            } else if goal.tokens_used > 0 {
                usage.push(format!("{} tokens", format_i64(goal.tokens_used)));
            }
            if !usage.is_empty() {
                plan_lines.push(Line::from(format!("  {}", usage.join(" | ")).dim()));
            }
        }
        if let Some(explanation) = &self.shell.plan_explanation {
            plan_lines.push(Line::from(explanation.clone().dim()));
        }
        if self.shell.plan_steps.is_empty() && self.shell.active_goal.is_none() {
            plan_lines.push(Line::from("no active plan".dim()));
        } else {
            for step in self.shell.plan_steps.iter().take(5) {
                plan_lines.push(plan_step_line(step.status, &step.step));
            }
        }
        panels.push(DashboardPanel::new("Plan", plan_lines));

        let tool_lines = if self.shell.tool_activity.is_empty() {
            vec![Line::from("idle".dim())]
        } else {
            self.shell
                .tool_activity
                .iter()
                .rev()
                .take(4)
                .rev()
                .map(|activity| tool_activity_line(activity, width))
                .collect()
        };
        panels.push(DashboardPanel::new("Tools", tool_lines));

        let mut workspace_lines = vec![Line::from(vec![
            "cwd ".dim(),
            dashboard_value(&self.shell.cwd, width, /*prefix_width*/ 4).into(),
        ])];
        if let Some(git_status) = &self.shell.workspace_git_status {
            if let Some(branch) = &git_status.branch {
                workspace_lines.push(Line::from(vec![
                    "branch ".dim(),
                    dashboard_value(branch, width, /*prefix_width*/ 7).cyan(),
                ]));
            }
            if git_status.is_dirty() {
                workspace_lines.push(Line::from(format!(
                    "changes {} files",
                    format_usize(git_status.changes.total())
                )));
                workspace_lines.extend(workspace_change_lines(&git_status.changes));
            } else {
                workspace_lines.push(Line::from("tree clean".green()));
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
                workspace_lines.push(Line::from(vec!["profile ".dim(), "managed".into()]));
                workspace_lines.push(Line::from(format!(
                    "files {file_system_label}, net {network}"
                )));
            }
            PermissionProfile::Disabled => {
                workspace_lines.push(Line::from(vec!["profile ".dim(), "full access".into()]));
            }
            PermissionProfile::External { network } => {
                workspace_lines.push(Line::from(vec!["profile ".dim(), "external".into()]));
                workspace_lines.push(Line::from(format!("net {network}")));
            }
        }
        if self.shell.runtime_workspace_roots.is_empty() {
            workspace_lines.push(Line::from("roots none selected".dim()));
        } else {
            const WORKSPACE_ROOT_PREVIEW_LIMIT: usize = 3;
            let root_count = self.shell.runtime_workspace_roots.len();
            workspace_lines.push(Line::from(format!(
                "roots {} writable",
                format_usize(root_count)
            )));
            for root in self
                .shell
                .runtime_workspace_roots
                .iter()
                .take(WORKSPACE_ROOT_PREVIEW_LIMIT)
            {
                workspace_lines.push(Line::from(vec![
                    "  ".dim(),
                    dashboard_value(&root.display().to_string(), width, /*prefix_width*/ 2).dim(),
                ]));
            }
            let hidden = root_count.saturating_sub(WORKSPACE_ROOT_PREVIEW_LIMIT);
            if hidden > 0 {
                workspace_lines.push(Line::from(
                    format!("  +{} more", format_usize(hidden)).dim(),
                ));
            }
        }
        panels.push(DashboardPanel::new("Workspace", workspace_lines));

        let key_lines = if self.shell.transcript_selection.is_some() {
            vec![
                Line::from("Up/Down select"),
                Line::from("Enter copy"),
                Line::from("Esc composer"),
            ]
        } else if self.shell.active_turn_id.is_some() {
            vec![
                Line::from("Enter steer"),
                Line::from("Ctrl+C interrupt, Esc exit"),
                Line::from("Alt+Up select, Ctrl+O copy"),
            ]
        } else {
            vec![
                Line::from("Enter send"),
                Line::from("Ctrl+C/Esc exit"),
                Line::from("Alt+Up select, Ctrl+O copy"),
            ]
        };
        panels.push(DashboardPanel::new("Keys", key_lines));

        self.render_dashboard_panels(content, &panels, buf);
    }

    fn render_dashboard_panels(&self, area: Rect, panels: &[DashboardPanel], buf: &mut Buffer) {
        let mut y = area.y;
        for (index, panel) in panels.iter().enumerate() {
            if y >= area.bottom() {
                break;
            }
            let desired_height = panel.height();
            let available_height = area.bottom().saturating_sub(y);
            let height = desired_height.min(available_height);
            if height == 0 {
                break;
            }
            let panel_area = Rect::new(area.x, y, area.width, height);
            fill_rect(buf, panel_area, panel.background(index));
            let mut lines = vec![Line::from(panel.title.clone().bold())];
            lines.extend(panel.lines.clone());
            Paragraph::new(lines)
                .style(pane_style(panel.background(index)))
                .wrap(Wrap { trim: false })
                .render(panel_area, buf);
            y = y.saturating_add(height).saturating_add(DASHBOARD_PANEL_GAP);
        }
    }

    fn render_titled_panel(
        &self,
        area: Rect,
        title: &str,
        lines: Vec<Line<'static>>,
        background: Color,
        buf: &mut Buffer,
    ) {
        let content = pane_content_rect(area);
        Paragraph::new(Line::from(title.to_string().bold()))
            .style(pane_style(background))
            .render(title_rect(content), buf);
        Paragraph::new(lines)
            .style(pane_style(background))
            .wrap(Wrap { trim: false })
            .render(body_rect_after_title(content), buf);
    }

    fn render_command_palette(&self, area: Rect, buf: &mut Buffer) {
        let Some(palette) = &self.shell.command_palette else {
            return;
        };
        let entries = self.shell.command_palette_entries();
        let palette_area = centered_band_rect(area, /*height*/ 17);
        let content = pane_content_rect(palette_area);
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
            .render(content, buf);
    }
}

struct DashboardPanel {
    title: String,
    lines: Vec<Line<'static>>,
}

impl DashboardPanel {
    fn new(title: impl Into<String>, lines: Vec<Line<'static>>) -> Self {
        Self {
            title: title.into(),
            lines,
        }
    }

    fn height(&self) -> u16 {
        u16::try_from(self.lines.len().saturating_add(1)).unwrap_or(u16::MAX)
    }

    fn background(&self, index: usize) -> Color {
        if index.is_multiple_of(2) {
            MOCHA_SURFACE0
        } else {
            MOCHA_MANTLE
        }
    }
}

fn push_transcript_lines(
    lines: &mut Vec<HyperlinkLine>,
    previous_kind: &mut Option<TranscriptKind>,
    line: &TranscriptLine,
    width: u16,
    cwd: &std::path::Path,
    selected: bool,
) {
    if should_separate_transcript_item(*previous_kind, line.kind) {
        lines.push(HyperlinkLine::new(Line::default()));
    }
    lines.extend(transcript_lines(line, width, cwd, selected));
    *previous_kind = Some(line.kind);
}

fn should_separate_transcript_item(
    previous_kind: Option<TranscriptKind>,
    current_kind: TranscriptKind,
) -> bool {
    previous_kind.is_some_and(|previous_kind| {
        previous_kind != TranscriptKind::System
            && matches!(
                current_kind,
                TranscriptKind::User | TranscriptKind::Assistant
            )
    })
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

fn pane_content_rect(area: Rect) -> Rect {
    let horizontal_padding = inset_for(area.width, PANE_PADDING);
    let vertical_padding = inset_for(area.height, PANE_PADDING);
    Rect::new(
        area.x.saturating_add(horizontal_padding),
        area.y.saturating_add(vertical_padding),
        area.width
            .saturating_sub(horizontal_padding.saturating_mul(2)),
        area.height
            .saturating_sub(vertical_padding.saturating_mul(2)),
    )
}

fn inset_for(size: u16, padding: u16) -> u16 {
    padding.min(size.saturating_sub(1) / 2)
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
    Line::from(vec![
        "Dashboard ".cyan().bold(),
        status_span(&shell.status),
        " · ".fg(MOCHA_OVERLAY0),
        dashboard_value(
            &shell.model,
            /*line_width*/ 24,
            /*prefix_width*/ 0,
        )
        .into(),
        " · ".fg(MOCHA_OVERLAY0),
        format!("{} tokens", format_i64(shell.token_usage.total_tokens)).fg(MOCHA_SUBTEXT0),
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

fn goal_status_span(status: codex_app_server_protocol::ThreadGoalStatus) -> Span<'static> {
    let label = goal_status_label(status);
    match status {
        codex_app_server_protocol::ThreadGoalStatus::Active => label.cyan(),
        codex_app_server_protocol::ThreadGoalStatus::Complete => label.green(),
        codex_app_server_protocol::ThreadGoalStatus::Blocked
        | codex_app_server_protocol::ThreadGoalStatus::UsageLimited
        | codex_app_server_protocol::ThreadGoalStatus::BudgetLimited => label.red(),
        codex_app_server_protocol::ThreadGoalStatus::Paused => label.magenta(),
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

fn tool_activity_line(activity: &ToolActivity, width: usize) -> Line<'static> {
    let status = match activity.status.as_str() {
        "completed" => activity.status.clone().green(),
        "failed" | "declined" => activity.status.clone().red(),
        "in progress" | "inprogress" => activity.status.clone().cyan(),
        _ => activity.status.clone().dim(),
    };
    let prefix_width = activity.status.chars().count() + 1;
    Line::from(vec![
        status,
        " ".dim(),
        dashboard_value(&activity.title, width, prefix_width).into(),
    ])
}

fn rate_limit_lines(limit: &RateLimitSnapshot, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let label = rate_limit_label(limit, width);
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

fn rate_limit_label(limit: &RateLimitSnapshot, width: usize) -> String {
    limit
        .limit_name
        .as_deref()
        .or(limit.limit_id.as_deref())
        .map(|label| dashboard_value(label, width, /*prefix_width*/ 10))
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
        spans.extend([" ".dim(), format!("{}m", format_i64(duration)).dim()]);
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
    .map(|(label, count)| Line::from(format!("  {label} {}", format_usize(count)).dim()))
    .collect()
}

fn short_id(id: &str) -> String {
    id.get(..8)
        .map(|prefix| format!("{prefix}..."))
        .unwrap_or_else(|| id.to_string())
}

fn dashboard_value(text: &str, line_width: usize, prefix_width: usize) -> String {
    let max_chars = line_width.saturating_sub(prefix_width).max(1);
    truncate_text(text, max_chars)
}

fn format_i64(value: i64) -> String {
    if value < 0 {
        format!("-{}", format_u64(value.unsigned_abs()))
    } else {
        format_u64(value as u64)
    }
}

fn format_usize(value: usize) -> String {
    format_u64(value as u64)
}

fn format_u64(value: u64) -> String {
    let text = value.to_string();
    let mut grouped = String::with_capacity(text.len() + text.len() / 3);
    for (index, ch) in text.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    grouped.chars().rev().collect()
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
            " deny  ".dim(),
            "e".cyan().bold(),
            " edit  ".dim(),
            "?".bold(),
            " explain".dim(),
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
