use super::ShellState;
use super::ToolBlockStatus;
use super::TranscriptKind;
use super::composer_render::composer_cursor_position;
use super::composer_render::composer_visual_cursor_line;
use super::composer_render::wrapped_composer_lines;
use super::dashboard::DashboardPanel;
use super::dashboard::dashboard_panels;
use super::design::body_rect_after_title;
use super::design::centered_band_rect;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::selection_style;
use super::design::title_rect;
use super::header::HeaderControl;
use super::header::HeaderView;
use super::navigation::DashboardRoute;
use super::navigation::DashboardTabs;
use crate::line_truncation::line_width;
use crate::line_truncation::truncate_line_to_width;
use crate::markdown;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::mark_buffer_hyperlinks;
use crate::terminal_hyperlinks::prefix_hyperlink_lines;
use crate::terminal_hyperlinks::visible_lines;
use crate::text_formatting::truncate_text;
use crate::tui;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use crossterm::cursor::SetCursorStyle;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;
use unicode_width::UnicodeWidthStr;

const DASHBOARD_COLLAPSE_WIDTH: u16 = 88;
const DASHBOARD_MIN_WIDTH: u16 = 42;
const DASHBOARD_MAX_WIDTH: u16 = 64;
const DASHBOARD_PANEL_GAP: u16 = 1;
const DASHBOARD_WIDTH_PERCENT: u16 = 34;
const HEADER_HEIGHT: u16 = 2;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DashboardPanelPosition {
    pub(super) line: usize,
    pub(super) column: usize,
}

impl ShellView<'_> {
    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, palette::BASE);
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
            fill_rect(buf, panel_area, palette::ELEVATED);
            self.render_titled_panel(
                panel_area,
                "Claude Code Import",
                lines,
                palette::ELEVATED,
                buf,
            );
        }
        if let Some(pending) = &self.shell.pending_mcp_management {
            let lines = pending.lines();
            let line_count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            let panel_height = line_count.saturating_add(4).min(area.height);
            let panel_area = centered_band_rect(area, panel_height);
            Clear.render(panel_area, buf);
            fill_rect(buf, panel_area, palette::ELEVATED);
            self.render_titled_panel(panel_area, "MCP Servers", lines, palette::ELEVATED, buf);
        }
        if let Some(pending) = &self.shell.pending_plugin_management {
            let lines = pending.lines();
            let line_count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            let panel_height = line_count.saturating_add(4).min(area.height);
            let panel_area = centered_band_rect(area, panel_height);
            Clear.render(panel_area, buf);
            fill_rect(buf, panel_area, palette::ELEVATED);
            self.render_titled_panel(panel_area, "Plugins", lines, palette::ELEVATED, buf);
        }
        if let Some(lines) = self.shell.safety_buffering_modal_lines() {
            let lines = word_wrap_lines(
                lines,
                RtOptions::new(usize::from(pane_content_rect(area).width.max(1))),
            );
            let line_count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            let panel_height = line_count.saturating_add(4).min(area.height);
            let panel_area = centered_band_rect(area, panel_height);
            Clear.render(panel_area, buf);
            fill_rect(buf, panel_area, palette::ELEVATED);
            self.render_titled_panel(panel_area, "Safety review", lines, palette::ELEVATED, buf);
        }
        self.render_command_palette(area, buf);
        if let Some(selector) = &self.shell.selector {
            selector.render(area, buf);
        }
    }

    pub(super) fn cursor_position(&self, area: Rect) -> Option<Position> {
        if self.shell.selector.is_some()
            || self.shell.command_palette.is_some()
            || self.shell.pending_approval.is_some()
            || self.shell.pending_elicitation.is_some()
            || self.shell.pending_external_agent_import.is_some()
            || self.shell.pending_mcp_management.is_some()
            || self.shell.pending_plugin_management.is_some()
            || self.shell.pending_user_input.is_some()
            || self.shell.safety_buffering_modal_lines().is_some()
            || self.shell.dashboard_focused()
        {
            return None;
        }

        composer_cursor_position(
            self.input_area(area),
            self.shell.composer.text(),
            self.shell.composer.cursor(),
        )
    }

    pub(super) fn input_area(&self, area: Rect) -> Rect {
        self.layout(area).input
    }

    pub(super) fn dashboard_route_at(
        &self,
        area: Rect,
        position: Position,
    ) -> Option<DashboardRoute> {
        let layout = self.layout(area);
        let dashboard = layout.dashboard.or(layout.collapsed_dashboard)?;
        let content = pane_content_rect(dashboard);
        if content.height == 0 {
            return None;
        }
        let tabs = Rect::new(content.x, content.y, content.width, content.height.min(2));
        if !tabs.contains(position) {
            return None;
        }
        DashboardTabs::new(tabs.width).route_at(position.x.saturating_sub(tabs.x))
    }

    pub(super) fn header_control_at(
        &self,
        area: Rect,
        position: Position,
    ) -> Option<HeaderControl> {
        let effort = self
            .shell
            .reasoning_effort
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "default".to_string());
        HeaderView {
            cwd: &self.shell.cwd,
            model: &self.shell.model,
            reasoning_effort: &effort,
            status: &self.shell.status,
            dashboard_visible: self.shell.dashboard_visible,
        }
        .control_at(self.layout(area).header, position)
    }

    pub(super) fn dashboard_panel_position_at(
        &self,
        area: Rect,
        position: Position,
        title: &str,
    ) -> Option<DashboardPanelPosition> {
        let layout = self.layout(area);
        let dashboard = layout.dashboard.or(layout.collapsed_dashboard)?;
        let content = pane_content_rect(dashboard);
        if !content.contains(position) {
            return None;
        }
        let panels = dashboard_panels(self.shell, usize::from(content.width));
        let mut y = content.y;
        for panel in panels {
            let height = panel.height().min(content.bottom().saturating_sub(y));
            let panel_area = Rect::new(content.x, y, content.width, height);
            if panel.title == title && panel_area.contains(position) {
                let text_x = panel_area.x.saturating_add(u16::from(panel.show_title));
                let body_y = panel_area.y.saturating_add(u16::from(panel.show_title));
                if position.y < body_y || position.x < text_x {
                    return None;
                }
                return Some(DashboardPanelPosition {
                    line: usize::from(position.y.saturating_sub(body_y)),
                    column: usize::from(position.x.saturating_sub(text_x)),
                });
            }
            y = y.saturating_add(height).saturating_add(DASHBOARD_PANEL_GAP);
            if y >= content.bottom() {
                break;
            }
        }
        None
    }

    pub(super) fn command_palette_entry_at(&self, area: Rect, position: Position) -> Option<usize> {
        self.shell.command_palette.as_ref()?;
        let entries = self.shell.command_palette_entries();
        let palette_height = u16::try_from(entries.len())
            .unwrap_or(u16::MAX)
            .saturating_add(5)
            .min(area.height);
        let content = pane_content_rect(centered_band_rect(area, palette_height));
        if !content.contains(position) {
            return None;
        }
        let index = usize::from(position.y.saturating_sub(content.y)).checked_sub(2)?;
        (index < entries.len()).then_some(index)
    }

    pub(super) fn approval_action_at(
        &self,
        area: Rect,
        position: Position,
    ) -> Option<super::ApprovalAction> {
        self.shell.pending_approval.as_ref()?;
        let body = body_rect_after_title(pane_content_rect(self.input_area(area)));
        if position.y != body.y.saturating_add(2) || !body.contains(position) {
            return None;
        }
        match position.x.saturating_sub(body.x) {
            2..=12 => Some(super::ApprovalAction::Choose(
                super::ApprovalChoice::Approve,
            )),
            14..=21 => Some(super::ApprovalAction::Choose(super::ApprovalChoice::Deny)),
            23..=30 => Some(super::ApprovalAction::Edit),
            32..=42 => Some(super::ApprovalAction::Explain),
            _ => None,
        }
    }

    fn layout(&self, area: Rect) -> ShellLayout {
        if !self.shell.dashboard_visible {
            let input_height =
                self.input_panel_height(area.height.saturating_sub(HEADER_HEIGHT), area.width);
            let main = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(HEADER_HEIGHT),
                    Constraint::Min(TRANSCRIPT_MIN_HEIGHT),
                    Constraint::Length(input_height),
                ])
                .split(area);
            return ShellLayout {
                header: main[0],
                collapsed_dashboard: None,
                transcript: main[1],
                input: main[2],
                dashboard: None,
            };
        }
        if area.width < DASHBOARD_COLLAPSE_WIDTH {
            let dashboard_height = area
                .height
                .saturating_sub(
                    HEADER_HEIGHT
                        .saturating_add(INPUT_PANEL_MIN_HEIGHT)
                        .saturating_add(TRANSCRIPT_MIN_HEIGHT),
                )
                .min(14)
                .max(3);
            let input_height = self.input_panel_height(
                area.height
                    .saturating_sub(HEADER_HEIGHT)
                    .saturating_sub(dashboard_height),
                area.width,
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

        let dashboard_width = u32::from(area.width)
            .saturating_mul(u32::from(DASHBOARD_WIDTH_PERCENT))
            .div_ceil(100)
            .try_into()
            .unwrap_or(u16::MAX)
            .max(DASHBOARD_MIN_WIDTH)
            .min(DASHBOARD_MAX_WIDTH)
            .min(area.width);
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(area.width.saturating_sub(dashboard_width)),
                Constraint::Length(dashboard_width),
            ])
            .split(area);
        let input_height = self.input_panel_height(
            area.height.saturating_sub(HEADER_HEIGHT),
            horizontal[0].width,
        );
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
        let effort = self
            .shell
            .reasoning_effort
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "default".to_string());
        HeaderView {
            cwd: &self.shell.cwd,
            model: &self.shell.model,
            reasoning_effort: &effort,
            status: &self.shell.status,
            dashboard_visible: self.shell.dashboard_visible,
        }
        .render(area, buf);
    }

    fn render_transcript(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, palette::BASE);
        let content = pane_content_rect(area);
        let body = body_rect_after_title(content);
        let cwd = std::path::Path::new(&self.shell.cwd);
        let mut text_body = body;
        let visible_count = usize::from(body.height);
        let mut layout = self.shell.transcript_render_cache.borrow_mut().layout(
            self.shell,
            text_body.width,
            cwd,
        );
        let mut max_scroll = layout.total_lines.saturating_sub(visible_count);
        if max_scroll > 0 && body.width > 2 {
            text_body.width = text_body.width.saturating_sub(2);
            layout = self.shell.transcript_render_cache.borrow_mut().layout(
                self.shell,
                text_body.width,
                cwd,
            );
            max_scroll = layout.total_lines.saturating_sub(visible_count);
        }
        self.shell.transcript_scroll_max.set(max_scroll);
        let scroll = self.shell.transcript_scroll.min(max_scroll);
        let visible_from = layout
            .total_lines
            .saturating_sub(visible_count.saturating_add(scroll));
        let title = if let Some(selected) = self.shell.transcript_selection {
            format!(
                "CONVERSATION  SELECT {}/{}",
                selected.saturating_add(1),
                self.shell.transcript.len()
            )
        } else {
            "CONVERSATION".to_string()
        };
        let scrollbar = transcript_scrollbar_metrics(
            layout.total_lines,
            body.height,
            visible_from,
            TRANSCRIPT_SCROLLBAR_MIN_THUMB_HEIGHT,
        );
        let visible_hyperlink_lines = layout.visible_hyperlink_lines(visible_from, visible_count);
        let visible_lines = visible_lines(visible_hyperlink_lines.clone());
        Paragraph::new(Line::from(vec![
            "◆ ".set_style(Style::new().fg(palette::FOCUS)),
            title.set_style(Style::new().fg(palette::MUTED).bold()),
        ]))
        .style(pane_style(palette::BASE))
        .render(title_rect(content), buf);
        Paragraph::new(visible_lines)
            .style(pane_style(palette::BASE))
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
        fill_rect(buf, area, palette::SURFACE);
        let border_color = if self.shell.dashboard_focused() {
            palette::BORDER
        } else {
            palette::FOCUS
        };
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(border_color))
            .style(pane_style(palette::SURFACE))
            .render(area, buf);
        if let Some(pending) = &self.shell.pending_approval {
            self.render_titled_panel(
                area,
                "APPROVAL",
                approval_lines(pending),
                palette::SURFACE,
                buf,
            );
            return;
        }
        if let Some(pending) = &self.shell.pending_elicitation {
            self.render_titled_panel(
                area,
                "MCP ELICITATION",
                elicitation_lines(pending),
                palette::SURFACE,
                buf,
            );
            return;
        }
        if let Some(pending) = &self.shell.pending_user_input {
            self.render_titled_panel(
                area,
                "TOOL INPUT",
                user_input_lines(
                    pending,
                    self.shell.composer.text(),
                    self.shell.composer.is_empty(),
                ),
                palette::SURFACE,
                buf,
            );
            return;
        }

        let (line, column) = self.shell.composer.cursor_position();
        let position = format!("{}:{}", line + 1, column + 1);
        let title_width = usize::from(pane_content_rect(area).width).saturating_sub(2);
        let titles = if self.shell.active_turn_id.is_some() {
            [
                format!("MESSAGE  ● RUNNING  {position}"),
                format!("MESSAGE  ●  {position}"),
                position.clone(),
            ]
        } else {
            [
                format!("MESSAGE  ENTER SEND  SHIFT+ENTER NEWLINE  {position}"),
                format!("MESSAGE  ENTER SEND  {position}"),
                format!("MESSAGE  {position}"),
            ]
        };
        let title = titles
            .into_iter()
            .find(|title| UnicodeWidthStr::width(title.as_str()) <= title_width)
            .unwrap_or(position);
        let body = body_rect_after_title(pane_content_rect(area));
        let visible_height = usize::from(body.height);
        let mut lines = wrapped_composer_lines(
            self.shell.composer.text(),
            self.shell.composer.is_empty(),
            usize::from(body.width).max(1),
        );
        if visible_height > 0 && lines.len() > visible_height {
            let max_start = lines.len().saturating_sub(visible_height);
            let cursor_line = composer_visual_cursor_line(
                self.shell.composer.text(),
                self.shell.composer.cursor(),
                usize::from(body.width).max(1),
            )
            .unwrap_or(line);
            let start = cursor_line
                .saturating_add(1)
                .saturating_sub(visible_height)
                .min(max_start);
            lines = lines.into_iter().skip(start).take(visible_height).collect();
        }
        self.render_titled_panel(area, &title, lines, palette::SURFACE, buf);
    }

    fn input_panel_height(&self, available_height: u16, input_width: u16) -> u16 {
        if self.shell.pending_approval.is_some()
            || self.shell.pending_user_input.is_some()
            || self.shell.pending_elicitation.is_some()
        {
            return available_height.min(INPUT_PANEL_MIN_HEIGHT);
        }

        let body_width = pane_content_rect(Rect::new(
            /*x*/ 0,
            /*y*/ 0,
            input_width,
            available_height,
        ))
        .width;
        let composer_line_count = u16::try_from(
            wrapped_composer_lines(
                self.shell.composer.text(),
                self.shell.composer.is_empty(),
                usize::from(body_width).max(1),
            )
            .len(),
        )
        .unwrap_or(u16::MAX);
        let desired_height = composer_line_count
            .saturating_add(PANE_CHROME_HEIGHT)
            .clamp(INPUT_PANEL_MIN_HEIGHT, INPUT_PANEL_MAX_HEIGHT);
        let max_height = available_height
            .saturating_sub(TRANSCRIPT_MIN_HEIGHT)
            .max(available_height.min(INPUT_PANEL_MIN_HEIGHT));
        desired_height.min(max_height)
    }

    fn render_dashboard(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, palette::DARK);
        for y in area.y..area.bottom() {
            if let Some(cell) = buf.cell_mut((area.x, y)) {
                cell.set_symbol("│").set_style(Style::new().fg(
                    if self.shell.dashboard_focused() {
                        palette::FOCUS
                    } else {
                        palette::BORDER
                    },
                ));
            }
        }
        let content = pane_content_rect(area);
        let width = usize::from(content.width);
        let panels = dashboard_panels(self.shell, width);

        self.render_dashboard_panels(content, &panels, buf);
    }

    fn render_collapsed_dashboard(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, palette::DARK);
        let content = pane_content_rect(area);
        let panels = dashboard_panels(self.shell, usize::from(content.width));
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
            let text_area = if panel.show_title {
                for rail_y in panel_area.y..panel_area.bottom() {
                    if let Some(cell) = buf.cell_mut((panel_area.x, rail_y)) {
                        cell.set_symbol("▎")
                            .set_style(Style::new().fg(palette::BORDER));
                    }
                }
                Rect::new(
                    panel_area.x.saturating_add(1),
                    panel_area.y,
                    panel_area.width.saturating_sub(1),
                    panel_area.height,
                )
            } else {
                panel_area
            };
            let mut lines = Vec::new();
            if panel.show_title {
                lines.push(panel.title_line());
            }
            lines.extend(panel.lines.clone());
            Paragraph::new(lines)
                .style(pane_style(panel.background(index)))
                .wrap(Wrap { trim: false })
                .render(text_area, buf);
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
        Paragraph::new(Line::from(vec![
            "◆ ".set_style(Style::new().fg(palette::FOCUS)),
            title
                .to_string()
                .set_style(Style::new().fg(palette::TEXT).bold()),
        ]))
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
            .saturating_add(5)
            .min(area.height);
        let palette_area = centered_band_rect(area, palette_height);
        let content = pane_content_rect(palette_area);
        buf.set_style(area, Style::new().fg(palette::MUTED).bg(palette::DARK));
        let shadow = Rect::new(
            palette_area.x.saturating_add(1),
            palette_area.y.saturating_add(1),
            palette_area
                .width
                .min(area.right().saturating_sub(palette_area.x + 1)),
            palette_area
                .height
                .min(area.bottom().saturating_sub(palette_area.y + 1)),
        );
        fill_rect(buf, shadow, palette::DARK);
        Clear.render(palette_area, buf);

        let mut lines = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let selected = index == palette.selected();
            let marker = if selected {
                "▌".set_style(Style::new().fg(palette::FOCUS))
            } else {
                " ".into()
            };
            let title = if entry.enabled {
                entry
                    .title
                    .to_string()
                    .set_style(Style::new().fg(palette::TEXT))
            } else {
                entry
                    .title
                    .to_string()
                    .set_style(Style::new().fg(palette::MUTED))
            };
            let detail = if selected {
                format!("  {}", truncate_text(entry.detail, /*max_graphemes*/ 34))
                    .set_style(Style::new().fg(palette::MUTED))
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

        fill_rect(buf, palette_area, palette::ELEVATED);
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(palette::FOCUS))
            .style(pane_style(palette::ELEVATED))
            .render(palette_area, buf);
        let mut palette_lines = vec![
            Line::from(vec![
                "◆ ".set_style(Style::new().fg(palette::PURPLE)),
                "ACTIONS".set_style(Style::new().fg(palette::TEXT).bold()),
                "  Ctrl+P".set_style(Style::new().fg(palette::MUTED)),
            ]),
            Line::from(""),
        ];
        palette_lines.extend(lines);
        palette_lines.push(Line::from(
            "  ↑↓ / j k navigate   Enter select   Esc close"
                .set_style(Style::new().fg(palette::MUTED)),
        ));
        Paragraph::new(palette_lines)
            .style(pane_style(palette::ELEVATED))
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
            cell.set_symbol("┃")
                .set_style(Style::new().fg(palette::FOCUS));
        } else {
            cell.set_symbol("│")
                .set_style(Style::new().fg(palette::BORDER));
        }
    }
}

pub(super) fn render_transcript_line(
    kind: TranscriptKind,
    text: &str,
    tool_status: Option<ToolBlockStatus>,
    width: u16,
    cwd: &std::path::Path,
    selected: bool,
) -> Vec<HyperlinkLine> {
    if kind == TranscriptKind::Separator {
        return vec![HyperlinkLine::new(
            Line::from("─".repeat(usize::from(width))).style(Style::new().fg(palette::BORDER)),
        )];
    }
    if let Some(status) = tool_status
        && matches!(
            kind,
            TranscriptKind::Tool | TranscriptKind::Diff | TranscriptKind::Output
        )
    {
        return tool_block_lines(kind, text, width, status, selected);
    }

    let width = usize::from(width).max(12);
    let label = kind.label();
    let style = match kind {
        TranscriptKind::System => LineStyle::Dim,
        TranscriptKind::User => LineStyle::Cyan,
        TranscriptKind::Assistant => LineStyle::Magenta,
        TranscriptKind::Plan => LineStyle::Green,
        TranscriptKind::Tool => LineStyle::Cyan,
        TranscriptKind::Diff => LineStyle::Green,
        TranscriptKind::Output => LineStyle::Dim,
        TranscriptKind::Separator => LineStyle::Dim,
        TranscriptKind::Status => LineStyle::Dim,
        TranscriptKind::Audit => LineStyle::Cyan,
        TranscriptKind::Error => LineStyle::Red,
    };

    let prefix_width = label.len() + 4;
    let body_width = width.saturating_sub(prefix_width).max(1);
    let initial_prefix = style.label_prefix(label, selected);
    let subsequent_prefix = " ".repeat(prefix_width).into();

    let mut rendered_lines = if matches!(kind, TranscriptKind::Assistant | TranscriptKind::Plan) {
        let rendered =
            markdown::render_markdown_agent_with_links_and_cwd(text, Some(body_width), Some(cwd))
                .into_iter()
                .map(|line| line.style(style.line_style()))
                .collect();
        prefix_hyperlink_lines(rendered, initial_prefix, subsequent_prefix)
    } else {
        let options = textwrap::Options::new(body_width);
        let wrapped_lines: Vec<HyperlinkLine> = textwrap::wrap(text, options)
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
    kind: TranscriptKind,
    text: &str,
    width: u16,
    status: ToolBlockStatus,
    selected: bool,
) -> Vec<HyperlinkLine> {
    let width = usize::from(width).max(12);
    let block_indent = if kind == TranscriptKind::Output {
        OUTPUT_BLOCK_INDENT.min(width.saturating_sub(1))
    } else {
        0
    };
    let block_width = width.saturating_sub(block_indent).max(1);
    let block_background = match kind {
        TranscriptKind::Output => palette::DARK,
        TranscriptKind::Tool | TranscriptKind::Diff => palette::SURFACE,
        TranscriptKind::System
        | TranscriptKind::User
        | TranscriptKind::Assistant
        | TranscriptKind::Plan
        | TranscriptKind::Separator
        | TranscriptKind::Status
        | TranscriptKind::Audit
        | TranscriptKind::Error => palette::SURFACE,
    };
    let label = kind.label();
    let label_prefix_width = label.len() + 3;
    let content_width = block_width.saturating_sub(label_prefix_width).max(1);
    let normalized_text = text.replace('\r', "\n").replace('\t', "    ");
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
    if kind == TranscriptKind::Output && wrapped.len() > OUTPUT_BLOCK_MAX_LINES {
        let hidden_lines = wrapped.len().saturating_sub(OUTPUT_BLOCK_MAX_LINES - 1);
        let mut tail = wrapped.split_off(hidden_lines);
        let noun = if hidden_lines == 1 { "line" } else { "lines" };
        wrapped = vec![format!("... {hidden_lines} earlier output {noun}")];
        wrapped.append(&mut tail);
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
            let accent_style = if kind == TranscriptKind::Output {
                Style::new().fg(palette::BORDER).bg(block_background)
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
            Self::Running => Style::new().fg(palette::CYAN).bg(palette::SURFACE),
            Self::Success => Style::new().fg(palette::SUCCESS).bg(palette::SURFACE),
            Self::Fail => Style::new().fg(palette::ERROR).bg(palette::SURFACE),
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
            self.label(format!("▶ {}  ", text.to_uppercase()))
        } else {
            self.label(format!("▎ {}  ", text.to_uppercase()))
        }
    }

    fn label(self, text: String) -> Span<'static> {
        Span::styled(text, Style::new().fg(self.color()).bold())
    }

    fn text(self, text: String) -> Span<'static> {
        Span::styled(text, Style::new().fg(self.text_color()))
    }

    fn line_style(self) -> Style {
        Style::new().fg(self.text_color())
    }

    fn color(self) -> Color {
        match self {
            Self::Cyan => palette::CYAN,
            Self::Dim => palette::MUTED,
            Self::Green => palette::SUCCESS,
            Self::Magenta => palette::PURPLE,
            Self::Red => palette::ERROR,
        }
    }

    fn text_color(self) -> Color {
        match self {
            Self::Dim => palette::MUTED,
            Self::Green => palette::SUCCESS,
            Self::Red => palette::ERROR,
            Self::Cyan | Self::Magenta => palette::TEXT,
        }
    }
}

fn approval_lines(pending: &super::PendingApproval) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            "? ".fg(palette::WARNING).bold(),
            pending.title().to_string().fg(palette::TEXT).bold(),
        ]),
        Line::from(vec![
            "  ".into(),
            pending.detail().to_string().fg(palette::MUTED),
        ]),
        Line::from(vec![
            "  ".into(),
            " Approve ↵ ".fg(palette::DARK).bg(palette::SUCCESS).bold(),
            " ".into(),
            " Deny n ".fg(palette::TEXT).bg(palette::ERROR).bold(),
            " ".into(),
            " Edit e ".fg(palette::TEXT).bg(palette::ELEVATED).bold(),
            " ".into(),
            " Explain ? ".fg(palette::TEXT).bg(palette::ELEVATED).bold(),
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
        Line::from(vec![
            "  ".into(),
            truncate_text(pending.detail(), /*max_graphemes*/ 62).dim(),
        ]),
        Line::from(action_line),
    ]
}
