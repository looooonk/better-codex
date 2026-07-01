use super::ShellState;
use super::ToolBlockStatus;
use super::TranscriptKind;
use super::TranscriptLine;
use super::dashboard::DashboardPanel;
use super::dashboard::dashboard_panels;
use super::dashboard::dashboard_value;
use super::dashboard::format_usize;
use super::design::MOCHA_BASE;
use super::design::MOCHA_MANTLE;
use super::design::MOCHA_SURFACE0;
use super::design::MOCHA_SURFACE1;
use super::design::body_rect_after_title;
use super::design::centered_band_rect;
use super::design::fill_rect;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::selection_style;
use super::design::title_rect;
use crate::line_truncation::line_width;
use crate::line_truncation::truncate_line_to_width;
use crate::markdown;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::mark_buffer_hyperlinks;
use crate::terminal_hyperlinks::prefix_hyperlink_lines;
use crate::terminal_hyperlinks::visible_lines;
use crate::text_formatting::truncate_text;
use crate::tui;
use crossterm::cursor::SetCursorStyle;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Position;
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
use unicode_width::UnicodeWidthStr;

const DASHBOARD_COLLAPSE_WIDTH: u16 = 88;
const DASHBOARD_PANEL_GAP: u16 = 1;
const HEADER_HEIGHT: u16 = 3;
const INPUT_PANEL_MIN_HEIGHT: u16 = 6;
const INPUT_PANEL_MAX_HEIGHT: u16 = 12;
const PANE_CHROME_HEIGHT: u16 = 3;
const TRANSCRIPT_MIN_HEIGHT: u16 = 5;
const TRANSCRIPT_SCROLLBAR_MIN_THUMB_HEIGHT: u16 = 2;
const OUTPUT_BLOCK_INDENT: usize = 2;
const OUTPUT_BLOCK_MAX_LINES: usize = 4;

pub(super) fn draw_shell(tui: &mut tui::Tui, shell: &ShellState) -> std::io::Result<()> {
    let height = tui.terminal.size()?.height;
    tui.draw(height, |frame| {
        let view = ShellView { shell };
        let area = frame.area();
        view.render(area, frame.buffer);
        if let Some(position) = view.cursor_position(area) {
            frame.set_cursor_style(SetCursorStyle::SteadyBar);
            frame.set_cursor_position(position);
        }
    })
}

pub(super) struct ShellView<'a> {
    pub(super) shell: &'a ShellState,
}

impl ShellView<'_> {
    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_BASE);
        let layout = self.layout(area);
        self.render_header(layout.header, buf);
        if let Some(collapsed_dashboard) = layout.collapsed_dashboard {
            self.render_collapsed_dashboard(collapsed_dashboard, buf);
        }
        self.render_transcript(layout.transcript, buf);
        self.render_input(layout.input, buf);
        if let Some(dashboard) = layout.dashboard {
            self.render_dashboard(dashboard, buf);
        }
        if let Some(pending) = &self.shell.pending_external_agent_import {
            let lines = pending.lines();
            let line_count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            let panel_height = line_count.saturating_add(4).min(area.height);
            let panel_area = centered_band_rect(area, panel_height);
            Clear.render(panel_area, buf);
            fill_rect(buf, panel_area, MOCHA_SURFACE0);
            self.render_titled_panel(panel_area, "Claude Code Import", lines, MOCHA_SURFACE0, buf);
        }
        if let Some(pending) = &self.shell.pending_mcp_management {
            let lines = pending.lines();
            let line_count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            let panel_height = line_count.saturating_add(4).min(area.height);
            let panel_area = centered_band_rect(area, panel_height);
            Clear.render(panel_area, buf);
            fill_rect(buf, panel_area, MOCHA_SURFACE0);
            self.render_titled_panel(panel_area, "MCP Servers", lines, MOCHA_SURFACE0, buf);
        }
        if let Some(pending) = &self.shell.pending_plugin_management {
            let lines = pending.lines();
            let line_count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            let panel_height = line_count.saturating_add(4).min(area.height);
            let panel_area = centered_band_rect(area, panel_height);
            Clear.render(panel_area, buf);
            fill_rect(buf, panel_area, MOCHA_SURFACE0);
            self.render_titled_panel(panel_area, "Plugins", lines, MOCHA_SURFACE0, buf);
        }
        self.render_command_palette(area, buf);
    }

    pub(super) fn cursor_position(&self, area: Rect) -> Option<Position> {
        if self.shell.command_palette.is_some()
            || self.shell.pending_approval.is_some()
            || self.shell.pending_elicitation.is_some()
            || self.shell.pending_external_agent_import.is_some()
            || self.shell.pending_mcp_management.is_some()
            || self.shell.pending_plugin_management.is_some()
            || self.shell.pending_user_input.is_some()
        {
            return None;
        }

        composer_cursor_position(
            self.input_area(area),
            self.shell.composer.text(),
            self.shell.composer.cursor(),
            self.shell.composer.is_empty(),
        )
    }

    pub(super) fn input_area(&self, area: Rect) -> Rect {
        self.layout(area).input
    }

    fn layout(&self, area: Rect) -> ShellLayout {
        if area.width < DASHBOARD_COLLAPSE_WIDTH {
            let dashboard_height = if area.height >= 30 {
                9
            } else if area.height >= 24 {
                7
            } else if area.height >= 18 {
                5
            } else {
                3
            };
            let input_height = self.input_panel_height(
                area.height
                    .saturating_sub(HEADER_HEIGHT)
                    .saturating_sub(dashboard_height),
            );
            let main = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(HEADER_HEIGHT),
                    Constraint::Length(dashboard_height),
                    Constraint::Min(TRANSCRIPT_MIN_HEIGHT),
                    Constraint::Length(input_height),
                ])
                .split(area);
            return ShellLayout {
                header: main[0],
                collapsed_dashboard: Some(main[1]),
                transcript: main[2],
                input: main[3],
                dashboard: None,
            };
        }

        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);
        let input_height = self.input_panel_height(area.height.saturating_sub(HEADER_HEIGHT));
        let main = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(HEADER_HEIGHT),
                Constraint::Min(TRANSCRIPT_MIN_HEIGHT),
                Constraint::Length(input_height),
            ])
            .split(horizontal[0]);
        ShellLayout {
            header: main[0],
            collapsed_dashboard: None,
            transcript: main[1],
            input: main[2],
            dashboard: Some(horizontal[1]),
        }
    }

    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_MANTLE);
        let content = pane_content_rect(area);
        Paragraph::new(Line::from("Better Codex".magenta().bold()))
            .style(pane_style(MOCHA_MANTLE))
            .render(content, buf);
    }

    fn render_transcript(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_BASE);
        let content = pane_content_rect(area);
        let body = body_rect_after_title(content);
        let cwd = std::path::Path::new(&self.shell.cwd);
        let mut text_body = body;
        let mut lines = collect_transcript_lines(self.shell, body.width, cwd);
        let visible_count = usize::from(body.height);
        let mut max_scroll = lines.len().saturating_sub(visible_count);
        if max_scroll > 0 && body.width > 2 {
            text_body.width = text_body.width.saturating_sub(2);
            lines = collect_transcript_lines(self.shell, text_body.width, cwd);
            max_scroll = lines.len().saturating_sub(visible_count);
        }
        self.shell.transcript_scroll_max.set(max_scroll);
        let scroll = self.shell.transcript_scroll.min(max_scroll);
        let visible_from = lines.len().saturating_sub(visible_count + scroll);
        let title = if let Some(selected) = self.shell.transcript_selection {
            format!(
                "Conversation select {}/{}",
                selected.saturating_add(1),
                self.shell.transcript.len()
            )
        } else {
            "Conversation".to_string()
        };
        let scrollbar = transcript_scrollbar_metrics(
            lines.len(),
            body.height,
            visible_from,
            TRANSCRIPT_SCROLLBAR_MIN_THUMB_HEIGHT,
        );
        let visible_hyperlink_lines = lines.into_iter().skip(visible_from).collect::<Vec<_>>();
        let visible_lines = visible_lines(visible_hyperlink_lines.clone());
        Paragraph::new(Line::from(title.bold()))
            .style(pane_style(MOCHA_BASE))
            .render(title_rect(content), buf);
        Paragraph::new(visible_lines)
            .style(pane_style(MOCHA_BASE))
            .render(text_body, buf);
        mark_buffer_hyperlinks(
            buf,
            text_body,
            &visible_hyperlink_lines,
            /*scroll_rows*/ 0,
        );
        if let Some(scrollbar) = scrollbar {
            render_transcript_scrollbar(buf, body, scrollbar);
        }
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
        let visible_height = usize::from(body_rect_after_title(pane_content_rect(area)).height);
        let mut lines = composer_lines(self.shell.composer.text(), self.shell.composer.is_empty());
        if visible_height > 0 && lines.len() > visible_height {
            let max_start = lines.len().saturating_sub(visible_height);
            let start = line
                .saturating_add(1)
                .saturating_sub(visible_height)
                .min(max_start);
            lines = lines.into_iter().skip(start).take(visible_height).collect();
        }
        self.render_titled_panel(area, &title, lines, MOCHA_SURFACE0, buf);
    }

    fn input_panel_height(&self, available_height: u16) -> u16 {
        if self.shell.pending_approval.is_some()
            || self.shell.pending_user_input.is_some()
            || self.shell.pending_elicitation.is_some()
        {
            return available_height.min(INPUT_PANEL_MIN_HEIGHT);
        }

        let composer_line_count =
            u16::try_from(self.shell.composer.text().split('\n').count()).unwrap_or(u16::MAX);
        let desired_height = composer_line_count
            .saturating_add(PANE_CHROME_HEIGHT)
            .clamp(INPUT_PANEL_MIN_HEIGHT, INPUT_PANEL_MAX_HEIGHT);
        let max_height = available_height
            .saturating_sub(TRANSCRIPT_MIN_HEIGHT)
            .max(available_height.min(INPUT_PANEL_MIN_HEIGHT));
        desired_height.min(max_height)
    }

    fn render_dashboard(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_MANTLE);
        let content = pane_content_rect(area);
        let width = usize::from(content.width);
        let panels = dashboard_panels(self.shell, width);

        self.render_dashboard_panels(content, &panels, buf);
    }

    fn render_collapsed_dashboard(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_SURFACE0);
        let content = pane_content_rect(area);
        let width = usize::from(content.width);
        let panels = dashboard_panels(self.shell, width);
        let body = body_rect_after_title(content);
        let visible_count = panels.len().min(6);
        let mut labels = panels
            .iter()
            .take(visible_count)
            .map(|panel| panel.title.as_str())
            .collect::<Vec<_>>()
            .join("  ");
        let hidden_count = panels.len().saturating_sub(visible_count);
        if hidden_count > 0 {
            labels.push_str(&format!("  +{} more", format_usize(hidden_count)));
        }
        let mut lines = vec![Line::from(
            dashboard_value(&labels, width, /*prefix_width*/ 0).dim(),
        )];
        let available_panel_lines = usize::from(body.height.saturating_sub(1));
        for title in [
            "Navigation",
            "Approvals",
            "Background",
            "Tools",
            "Subagents",
            "Sessions",
            "Settings",
            "Thread",
            "Status",
            "Model",
            "Tokens",
            "Plan",
            "Workspace",
            "Edits",
            "Rate Limits",
            "Keys",
        ] {
            if lines.len() > available_panel_lines {
                break;
            }
            if let Some(panel) = panels.iter().find(|panel| panel.title == title) {
                let summary = panel
                    .lines
                    .first()
                    .map(|line| {
                        line.spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect::<String>()
                    })
                    .unwrap_or_else(|| "empty".to_string());
                let title = panel.title.clone();
                let prefix_width = title.chars().count() + 1;
                lines.push(Line::from(vec![
                    title.cyan().bold(),
                    " ".dim(),
                    dashboard_value(&summary, width, prefix_width).into(),
                ]));
            }
        }

        Paragraph::new(Line::from("Dashboard".bold()))
            .style(pane_style(MOCHA_SURFACE0))
            .render(title_rect(content), buf);
        Paragraph::new(lines)
            .style(pane_style(MOCHA_SURFACE0))
            .wrap(Wrap { trim: false })
            .render(body, buf);
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
        let palette_height = u16::try_from(entries.len())
            .unwrap_or(u16::MAX)
            .saturating_add(6)
            .min(area.height);
        let palette_area = centered_band_rect(area, palette_height);
        let content = pane_content_rect(palette_area);
        Clear.render(palette_area, buf);

        let mut lines = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
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
                lines.push(line.style(selection_style()));
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

#[derive(Debug, Clone, Copy)]
struct ShellLayout {
    header: Rect,
    collapsed_dashboard: Option<Rect>,
    transcript: Rect,
    input: Rect,
    dashboard: Option<Rect>,
}

fn collect_transcript_lines(
    shell: &ShellState,
    transcript_width: u16,
    cwd: &std::path::Path,
) -> Vec<HyperlinkLine> {
    let mut lines = Vec::new();
    let mut previous_kind = None;
    for (index, line) in shell.transcript.iter().enumerate() {
        push_transcript_lines(
            &mut lines,
            &mut previous_kind,
            line,
            transcript_width,
            cwd,
            shell.transcript_selection == Some(index),
        );
    }
    if !shell.streaming_plan.is_empty() {
        push_transcript_lines(
            &mut lines,
            &mut previous_kind,
            &TranscriptLine::new(TranscriptKind::Plan, shell.streaming_plan.clone()),
            transcript_width,
            cwd,
            /*selected*/ false,
        );
    }
    if !shell.streaming_assistant.is_empty() {
        push_transcript_lines(
            &mut lines,
            &mut previous_kind,
            &TranscriptLine::new(TranscriptKind::Assistant, shell.streaming_assistant.clone()),
            transcript_width,
            cwd,
            /*selected*/ false,
        );
    }
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TranscriptScrollbarMetrics {
    pub(super) thumb_top: u16,
    pub(super) thumb_height: u16,
}

pub(super) fn transcript_scrollbar_metrics(
    total_lines: usize,
    visible_count: u16,
    visible_from: usize,
    min_thumb_height: u16,
) -> Option<TranscriptScrollbarMetrics> {
    let visible_count_usize = usize::from(visible_count);
    if visible_count == 0 || total_lines <= visible_count_usize {
        return None;
    }

    let track_height = visible_count;
    let min_thumb_height = min_thumb_height.min(track_height).max(1);
    let raw_thumb_height = visible_count_usize
        .saturating_mul(visible_count_usize)
        .div_ceil(total_lines)
        .try_into()
        .unwrap_or(u16::MAX);
    let thumb_height = raw_thumb_height.clamp(min_thumb_height, track_height);
    let thumb_travel = track_height.saturating_sub(thumb_height);
    let max_visible_from = total_lines.saturating_sub(visible_count_usize);
    let thumb_top = if thumb_travel == 0 || max_visible_from == 0 {
        0
    } else {
        let rounded_offset = visible_from
            .min(max_visible_from)
            .saturating_mul(usize::from(thumb_travel))
            .saturating_add(max_visible_from / 2)
            / max_visible_from;
        rounded_offset.try_into().unwrap_or(thumb_travel)
    };

    Some(TranscriptScrollbarMetrics {
        thumb_top,
        thumb_height,
    })
}

fn render_transcript_scrollbar(
    buf: &mut Buffer,
    body: Rect,
    scrollbar: TranscriptScrollbarMetrics,
) {
    let x = body.right().saturating_sub(1);
    let thumb_start = body.y.saturating_add(scrollbar.thumb_top);
    let thumb_end = thumb_start.saturating_add(scrollbar.thumb_height);
    for y in body.y..body.bottom() {
        let cell = buf.cell_mut((x, y)).expect("scrollbar cell should exist");
        if (thumb_start..thumb_end).contains(&y) {
            cell.set_symbol("┃").set_style(Style::new().cyan());
        } else {
            cell.set_symbol("│").set_style(Style::new().dim());
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
    let Some(previous_kind) = previous_kind else {
        return false;
    };
    if previous_kind == TranscriptKind::System {
        return false;
    }
    matches!(
        current_kind,
        TranscriptKind::User
            | TranscriptKind::Assistant
            | TranscriptKind::Tool
            | TranscriptKind::Diff
            | TranscriptKind::Output
    )
}

fn transcript_lines(
    line: &TranscriptLine,
    width: u16,
    cwd: &std::path::Path,
    selected: bool,
) -> Vec<HyperlinkLine> {
    if let Some(status) = line.tool_status
        && matches!(
            line.kind,
            TranscriptKind::Tool | TranscriptKind::Diff | TranscriptKind::Output
        )
    {
        return tool_block_lines(line, width, status, selected);
    }

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
        rendered_lines = rendered_lines
            .into_iter()
            .map(|line| line.style(selection_style()))
            .collect();
    }
    rendered_lines
}

fn tool_block_lines(
    line: &TranscriptLine,
    width: u16,
    status: ToolBlockStatus,
    selected: bool,
) -> Vec<HyperlinkLine> {
    let width = usize::from(width).max(12);
    let block_indent = if line.kind == TranscriptKind::Output {
        OUTPUT_BLOCK_INDENT.min(width.saturating_sub(1))
    } else {
        0
    };
    let block_width = width.saturating_sub(block_indent).max(1);
    let block_background = match line.kind {
        TranscriptKind::Output => MOCHA_MANTLE,
        TranscriptKind::Tool | TranscriptKind::Diff => MOCHA_SURFACE0,
        TranscriptKind::System
        | TranscriptKind::User
        | TranscriptKind::Assistant
        | TranscriptKind::Plan
        | TranscriptKind::Status
        | TranscriptKind::Audit
        | TranscriptKind::Error => MOCHA_SURFACE0,
    };
    let label = line.kind.label();
    let label_prefix_width = label.len() + 3;
    let content_width = block_width.saturating_sub(label_prefix_width).max(1);
    let normalized_text = line.text.replace('\r', "\n").replace('\t', "    ");
    let visible_text = codex_ansi_escape::ansi_escape(&normalized_text);
    let visible_text_lines = if visible_text.lines.is_empty() {
        vec![String::new()]
    } else {
        visible_text
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    let mut wrapped = Vec::new();
    for text in visible_text_lines {
        let line_wrapped = textwrap::wrap(&text, textwrap::Options::new(content_width));
        if line_wrapped.is_empty() {
            wrapped.push(String::new());
        } else {
            wrapped.extend(line_wrapped.into_iter().map(std::borrow::Cow::into_owned));
        }
    }
    if line.kind == TranscriptKind::Output && wrapped.len() > OUTPUT_BLOCK_MAX_LINES {
        let hidden_lines = wrapped.len().saturating_sub(OUTPUT_BLOCK_MAX_LINES - 1);
        wrapped.truncate(OUTPUT_BLOCK_MAX_LINES);
        if let Some(last) = wrapped.last_mut() {
            let noun = if hidden_lines == 1 { "line" } else { "lines" };
            *last = format!("... {hidden_lines} more output {noun}");
        }
    }
    let mut rendered_lines = wrapped
        .into_iter()
        .enumerate()
        .map(|(index, wrapped)| {
            let label_span = if index == 0 {
                format!("{label} ").bold()
            } else {
                " ".repeat(label.len() + 1).dim()
            };
            let mut spans = Vec::new();
            if block_indent > 0 {
                spans.push(" ".repeat(block_indent).into());
            }
            let accent_style = if line.kind == TranscriptKind::Output {
                Style::new().fg(MOCHA_SURFACE1).bg(block_background)
            } else {
                status.accent_style()
            };
            spans.extend([
                Span::styled("▌", accent_style),
                " ".into(),
                label_span,
                wrapped.into(),
            ]);
            let content_span_index = usize::from(block_indent > 0) + 3;
            let occupied_width =
                block_indent + label_prefix_width + spans[content_span_index].content.width();
            if occupied_width < width {
                spans.push(Span::styled(
                    " ".repeat(width - occupied_width),
                    Style::new().bg(block_background),
                ));
            }
            let mut line = Line::from(spans);
            for span in line.spans.iter_mut().skip(usize::from(block_indent > 0)) {
                span.style = span.style.patch(Style::new().bg(block_background));
            }
            if line_width(&line) > width {
                line = truncate_line_to_width(line, width);
            }
            let rendered_width = line_width(&line);
            if rendered_width < width {
                line.spans.push(Span::styled(
                    " ".repeat(width - rendered_width),
                    Style::new().bg(block_background),
                ));
            }
            HyperlinkLine::new(line)
        })
        .collect::<Vec<_>>();

    if selected {
        rendered_lines = rendered_lines
            .into_iter()
            .map(|line| line.style(selection_style()))
            .collect();
    }
    rendered_lines
}

impl ToolBlockStatus {
    fn accent_style(self) -> Style {
        match self {
            Self::Running => Style::new().cyan().bg(MOCHA_SURFACE0),
            Self::Success => Style::new().green().bg(MOCHA_SURFACE0),
            Self::Fail => Style::new().red().bg(MOCHA_SURFACE0),
        }
    }
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

fn composer_lines(text: &str, is_empty: bool) -> Vec<Line<'static>> {
    if is_empty {
        return vec![Line::from(vec![
            "> ".cyan(),
            "Type a message, Shift+Enter for newline".dim(),
        ])];
    }

    let mut lines = Vec::new();
    for (index, logical_line) in text.split('\n').enumerate() {
        if logical_line.is_empty() {
            lines.push(Line::default());
            continue;
        }

        let prefix = if index == 0 { "> ".cyan() } else { "  ".dim() };
        lines.push(Line::from(vec![prefix, logical_line.to_string().into()]));
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

fn composer_cursor_position(
    input_area: Rect,
    text: &str,
    cursor: usize,
    is_empty: bool,
) -> Option<Position> {
    let body = body_rect_after_title(pane_content_rect(input_area));
    if body.width == 0 || body.height == 0 {
        return None;
    }

    let cursor = cursor.min(text.len());
    let line_start = text[..cursor]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let logical_line = text[..line_start].chars().filter(|ch| *ch == '\n').count();
    let line_count = if is_empty {
        1
    } else {
        text.split('\n').count()
    };
    let visible_height = usize::from(body.height);
    let visible_start = if line_count > visible_height {
        logical_line
            .saturating_add(1)
            .saturating_sub(visible_height)
            .min(line_count.saturating_sub(visible_height))
    } else {
        0
    };
    let y = logical_line.checked_sub(visible_start)?;
    if y >= visible_height {
        return None;
    }

    let before_cursor = &text[line_start..cursor];
    let unwrapped_column = 2usize.saturating_add(UnicodeWidthStr::width(before_cursor));
    let body_width = usize::from(body.width).max(1);
    let wrapped_y = y.saturating_add(unwrapped_column / body_width);
    if wrapped_y >= visible_height {
        return None;
    }
    let x = unwrapped_column % body_width;

    Some(Position {
        x: body.x.saturating_add(u16::try_from(x).unwrap_or(u16::MAX)),
        y: body
            .y
            .saturating_add(u16::try_from(wrapped_y).unwrap_or(u16::MAX)),
    })
}
