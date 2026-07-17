use super::ShellState;
use super::agent_activity_render::agent_activity_overview_lines;
use super::agent_activity_render::agent_activity_thread_at_line;
use super::composer_render::composer_cursor_position;
use super::composer_render::composer_visual_cursor_line;
use super::composer_render::wrapped_composer_lines;
use super::dashboard::DashboardPanel;
use super::dashboard::dashboard_panels;
use super::design::body_rect_after_title;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::title_rect;
use super::header::HeaderControl;
use super::header::HeaderView;
use super::input_request_view::approval_lines;
use super::input_request_view::elicitation_lines;
use super::input_request_view::request_panel_hit;
use super::input_request_view::user_input_lines;
use super::input_request_view::visible_request_panel_lines;
use super::modal_view::render_modal;
use super::navigation::DashboardRoute;
use super::navigation::DashboardTabs;
use super::settings::SettingsTabs;
use super::shell_layout;
use super::shell_layout::DashboardPlacement;
use super::shell_layout::MIN_TERMINAL_WIDTH;
use super::shell_layout::ShellLayout;
use super::transcript_view::render_transcript;
use crate::tui;
use crossterm::cursor::SetCursorStyle;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;
use unicode_width::UnicodeWidthStr;

const DASHBOARD_PANEL_GAP: u16 = 1;

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
    pub(super) width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PointerPane {
    Header,
    Transcript,
    Input,
    Dashboard,
}

impl ShellView<'_> {
    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, palette::BASE);
        let Some(layout) = self.layout(area) else {
            self.render_terminal_too_narrow(area, buf);
            return;
        };
        self.render_header(layout.header, buf);
        render_transcript(
            self.shell,
            layout.transcript,
            self.base_hover_position(),
            buf,
        );
        self.render_input(layout.input, buf);
        if let Some(dashboard) = layout.dashboard {
            self.render_dashboard(dashboard, buf);
        }
        if let Some(pending) = &self.shell.pending_external_agent_import {
            render_modal(area, "Claude Code Import", pending.lines(), buf);
        }
        if let Some(pending) = &self.shell.pending_mcp_management {
            render_modal(area, "MCP Servers", pending.lines(), buf);
        }
        if let Some(pending) = &self.shell.pending_plugin_management {
            render_modal(area, "Plugins", pending.lines(), buf);
        }
        if let Some(lines) = self.shell.safety_buffering_modal_lines() {
            render_modal(area, "Safety review", lines, buf);
        }
        super::command_palette_view::render(self.shell, area, buf);
        if let Some(selector) = &self.shell.selector {
            selector.render(area, self.shell.pointer_position, buf);
        }
        if let Some(log) = &self.shell.agent_log {
            super::agent_log_view::render_agent_log(log, area, buf);
        }
        if let Some(output) = &self.shell.tool_output {
            super::tool_output_view::render_tool_output(output, area, buf);
        }
        if let Some(diff) = &self.shell.diff_view {
            super::diff_view_view::render_diff_view(diff, area, buf);
        }
    }

    pub(super) fn cursor_position(&self, area: Rect) -> Option<Position> {
        if self.shell.selector.is_some()
            || self.shell.command_palette.is_some()
            || self.shell.agent_log.is_some()
            || self.shell.tool_output.is_some()
            || self.shell.diff_view.is_some()
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

        let input = self.layout(area)?.input;
        composer_cursor_position(
            input,
            self.shell.composer.text(),
            self.shell.composer.cursor(),
        )
    }

    pub(super) fn input_area(&self, area: Rect) -> Rect {
        self.layout(area)
            .map_or(Rect::default(), |layout| layout.input)
    }

    pub(super) fn transcript_card_at(
        &self,
        area: Rect,
        position: Position,
    ) -> Option<super::transcript_view::TranscriptCardHit> {
        let layout = self.layout(area)?;
        if layout
            .dashboard
            .is_some_and(|dashboard| dashboard.area().contains(position))
        {
            return None;
        }
        super::transcript_view::transcript_card_at(self.shell, layout.transcript, position)
    }

    pub(super) fn pointer_pane_at(&self, area: Rect, position: Position) -> Option<PointerPane> {
        let layout = self.layout(area)?;
        if layout.header.contains(position) {
            Some(PointerPane::Header)
        } else if layout
            .dashboard
            .is_some_and(|dashboard| dashboard.area().contains(position))
        {
            Some(PointerPane::Dashboard)
        } else if layout.transcript.contains(position) {
            Some(PointerPane::Transcript)
        } else if layout.input.contains(position) {
            Some(PointerPane::Input)
        } else {
            None
        }
    }

    pub(super) fn dashboard_route_at(
        &self,
        area: Rect,
        position: Position,
    ) -> Option<DashboardRoute> {
        let dashboard = self.layout(area)?.dashboard?.area();
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
        let header = self.layout(area)?.header;
        let effort = self
            .shell
            .reasoning_effort
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "default".to_string());
        let service_tier = self
            .shell
            .service_tier
            .as_deref()
            .filter(|service_tier| !service_tier.trim().is_empty())
            .unwrap_or("default");
        HeaderView {
            cwd: &self.shell.cwd,
            model: &self.shell.model,
            reasoning_effort: &effort,
            service_tier,
            status: &self.shell.status,
            status_spinner_frame: self
                .shell
                .status_spinner_active()
                .then_some(self.shell.status_spinner_frame),
            dashboard_visible: self.shell.dashboard_visible,
        }
        .control_at(header, position)
    }

    pub(super) fn dashboard_panel_position_at(
        &self,
        area: Rect,
        position: Position,
        title: &str,
    ) -> Option<DashboardPanelPosition> {
        let dashboard = self.layout(area)?.dashboard?;
        let panel_gap = match dashboard {
            DashboardPlacement::Sidebar(_) => DASHBOARD_PANEL_GAP,
            DashboardPlacement::Overlay(_) => 0,
        };
        let dashboard = dashboard.area();
        let content = pane_content_rect(dashboard);
        if !content.contains(position) {
            return None;
        }
        let panels = dashboard_panels(self.shell, usize::from(content.width));
        let mut y = content.y;
        for panel in panels {
            let available_height = content.bottom().saturating_sub(y);
            if panel.show_title && available_height < 2 {
                break;
            }
            let height = panel.height().min(available_height);
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
                    width: usize::from(panel_area.width.saturating_sub(1)),
                });
            }
            y = y.saturating_add(height).saturating_add(panel_gap);
            if y >= content.bottom() {
                break;
            }
        }
        None
    }

    pub(super) fn command_palette_entry_at(&self, area: Rect, position: Position) -> Option<usize> {
        super::command_palette_view::entry_at(self.shell, area, position)
    }

    pub(super) fn approval_action_at(
        &self,
        area: Rect,
        position: Position,
    ) -> Option<super::ApprovalAction> {
        let pending = self.shell.pending_approval.as_ref()?;
        let lines = approval_lines(pending);
        let hit = request_panel_hit(self.input_area(area), position, &lines)?;
        if hit.line != 2 {
            return None;
        }
        match hit.column {
            2..=12 => Some(super::ApprovalAction::Choose(
                super::ApprovalChoice::Approve,
            )),
            14..=21 => Some(super::ApprovalAction::Choose(super::ApprovalChoice::Deny)),
            23..=30 => Some(super::ApprovalAction::Edit),
            32..=42 => Some(super::ApprovalAction::Explain),
            _ => None,
        }
    }

    pub(super) fn elicitation_choice_at(
        &self,
        area: Rect,
        position: Position,
    ) -> Option<super::ElicitationChoice> {
        let pending = self.shell.pending_elicitation.as_ref()?;
        let lines = elicitation_lines(pending);
        let hit = request_panel_hit(self.input_area(area), position, &lines)?;
        pending.choice_at(hit.line, hit.column)
    }

    pub(super) fn user_input_option_at(&self, area: Rect, position: Position) -> Option<usize> {
        let pending = self.shell.pending_user_input.as_ref()?;
        let lines = user_input_lines(
            pending,
            self.shell.composer.text(),
            self.shell.composer.is_empty(),
        );
        let hit = request_panel_hit(self.input_area(area), position, &lines)?;
        if hit.line != 2 {
            return None;
        }
        let text = lines[2]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        pending
            .current_question()?
            .options
            .as_ref()?
            .iter()
            .take(3)
            .enumerate()
            .find_map(|(index, option)| {
                let label = format!("{} {}", index + 1, option.label);
                let byte_start = text.find(&label)?;
                let start = UnicodeWidthStr::width(&text[..byte_start]);
                (start..start + UnicodeWidthStr::width(label.as_str()))
                    .contains(&hit.column)
                    .then_some(index)
            })
    }

    fn layout(&self, area: Rect) -> Option<ShellLayout> {
        shell_layout::calculate(self.shell, area)
    }

    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        let effort = self
            .shell
            .reasoning_effort
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "default".to_string());
        let service_tier = self
            .shell
            .service_tier
            .as_deref()
            .filter(|service_tier| !service_tier.trim().is_empty())
            .unwrap_or("default");
        let view = HeaderView {
            cwd: &self.shell.cwd,
            model: &self.shell.model,
            reasoning_effort: &effort,
            service_tier,
            status: &self.shell.status,
            status_spinner_frame: self
                .shell
                .status_spinner_active()
                .then_some(self.shell.status_spinner_frame),
            dashboard_visible: self.shell.dashboard_visible,
        };
        let hovered = self
            .base_hover_position()
            .and_then(|position| view.control_at(area, position));
        view.render(area, hovered, buf);
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
            self.render_request_panel(
                area,
                "APPROVAL",
                approval_lines(pending),
                palette::SURFACE,
                buf,
            );
            return;
        }
        if let Some(pending) = &self.shell.pending_elicitation {
            self.render_request_panel(
                area,
                "MCP ELICITATION",
                elicitation_lines(pending),
                palette::SURFACE,
                buf,
            );
            return;
        }
        if let Some(pending) = &self.shell.pending_user_input {
            self.render_request_panel(
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
        let queue_label = self
            .shell
            .composer
            .queued_edit_position()
            .map(|(index, count)| format!("EDIT {index}/{count}"))
            .or_else(|| {
                let count = self.shell.composer.queued_count();
                (count > 0).then(|| format!("QUEUED {count}"))
            });
        let titles = if let Some(queue_label) = queue_label {
            if self.shell.active_turn_id.is_some() {
                vec![
                    format!("MESSAGE  ● RUNNING  {queue_label}  {position}"),
                    format!("MESSAGE  ●  {queue_label}  {position}"),
                    format!("MESSAGE  {queue_label}  {position}"),
                    format!("{queue_label}  {position}"),
                    position.clone(),
                ]
            } else {
                vec![
                    format!("MESSAGE  {queue_label}  {position}"),
                    format!("{queue_label}  {position}"),
                    position.clone(),
                ]
            }
        } else if self.shell.active_turn_id.is_some() {
            vec![
                format!("MESSAGE  ● RUNNING  {position}"),
                format!("MESSAGE  ●  {position}"),
                position.clone(),
            ]
        } else {
            vec![format!("MESSAGE  {position}"), position.clone()]
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

    fn render_dashboard(&self, placement: DashboardPlacement, buf: &mut Buffer) {
        let area = placement.area();
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
        let panel_gap = match placement {
            DashboardPlacement::Sidebar(_) => DASHBOARD_PANEL_GAP,
            DashboardPlacement::Overlay(_) => 0,
        };
        self.render_dashboard_panels(content, &panels, panel_gap, buf);
    }

    fn render_dashboard_panels(
        &self,
        area: Rect,
        panels: &[DashboardPanel],
        panel_gap: u16,
        buf: &mut Buffer,
    ) {
        let mut y = area.y;
        for panel in panels {
            if y >= area.bottom() {
                break;
            }
            let desired_height = panel.height();
            let available_height = area.bottom().saturating_sub(y);
            if panel.show_title && available_height < 2 {
                break;
            }
            let height = desired_height.min(available_height);
            if height == 0 {
                break;
            }
            let panel_area = Rect::new(area.x, y, area.width, height);
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
            Paragraph::new(panel.render_lines(usize::from(text_area.width)))
                .style(Style::new().fg(palette::TEXT))
                .render(text_area, buf);
            self.render_dashboard_hover(panel, panel_area, text_area, buf);
            y = y.saturating_add(height).saturating_add(panel_gap);
        }
    }

    fn render_dashboard_hover(
        &self,
        panel: &DashboardPanel,
        panel_area: Rect,
        text_area: Rect,
        buf: &mut Buffer,
    ) {
        let Some(pointer) = self
            .base_hover_position()
            .filter(|pointer| panel_area.contains(*pointer))
        else {
            return;
        };
        if panel.title == "Navigation" {
            let tabs = DashboardTabs::new(panel_area.width);
            let Some(route) = tabs.route_at(pointer.x.saturating_sub(panel_area.x)) else {
                return;
            };
            let Some(columns) = tabs.column_range(route) else {
                return;
            };
            let x = panel_area.x.saturating_add(columns.start);
            let width = columns.end.saturating_sub(columns.start);
            buf.set_style(
                Rect::new(x, panel_area.y, width, 1),
                Style::new().bg(palette::BORDER),
            );
            if panel_area.height > 1 {
                buf.set_style(
                    Rect::new(x, panel_area.y.saturating_add(1), width, 1),
                    Style::new().fg(palette::FOCUS),
                );
            }
            return;
        }
        if panel.title == "Settings" && matches!(pointer.y.saturating_sub(text_area.y), 1 | 2) {
            let tabs = SettingsTabs::new(usize::from(text_area.width));
            let column = usize::from(pointer.x.saturating_sub(text_area.x));
            let Some(page) = tabs.page_at(column) else {
                return;
            };
            let Some(columns) = tabs.column_range(page) else {
                return;
            };
            let x = text_area
                .x
                .saturating_add(u16::try_from(columns.start).unwrap_or(u16::MAX));
            let width =
                u16::try_from(columns.end.saturating_sub(columns.start)).unwrap_or(u16::MAX);
            buf.set_style(
                Rect::new(x, text_area.y.saturating_add(1), width, 1),
                Style::new().bg(palette::BORDER),
            );
            if text_area.y.saturating_add(2) < panel_area.bottom() {
                buf.set_style(
                    Rect::new(x, text_area.y.saturating_add(2), width, 1),
                    Style::new().fg(palette::FOCUS),
                );
            }
            return;
        }
        let interactive = match (self.shell.dashboard_route, panel.title.as_str()) {
            (DashboardRoute::Sessions, "Sessions")
            | (DashboardRoute::Agents, "Agents")
            | (DashboardRoute::Status, "Settings") => true,
            (DashboardRoute::Status, "Edits") => self.shell.diff_store.has_session_edits(),
            (DashboardRoute::Sessions, _)
            | (DashboardRoute::Agents, _)
            | (DashboardRoute::Status, _)
            | (DashboardRoute::Help, _) => false,
        };
        if panel.title == "Agents" && pointer.y > text_area.y {
            let line = usize::from(pointer.y.saturating_sub(text_area.y.saturating_add(1)));
            let overview_height = agent_activity_overview_lines(
                &self.shell.agent_activity,
                usize::from(text_area.width),
            )
            .len();
            let agent_line = line.checked_sub(overview_height).and_then(|line| {
                agent_activity_thread_at_line(
                    &self.shell.agent_activity,
                    line,
                    /*line_budget*/ 24,
                )
            });
            if agent_line.is_none() {
                return;
            }
        }
        if interactive && pointer.y > text_area.y {
            buf.set_style(
                Rect::new(text_area.x, pointer.y, text_area.width, 1),
                Style::new().bg(palette::BORDER),
            );
        }
    }

    fn base_hover_position(&self) -> Option<Position> {
        let blocked = self.shell.selector.is_some()
            || self.shell.command_palette.is_some()
            || self.shell.agent_log.is_some()
            || self.shell.tool_output.is_some()
            || self.shell.diff_view.is_some()
            || self.shell.pending_approval.is_some()
            || self.shell.pending_elicitation.is_some()
            || self.shell.pending_external_agent_import.is_some()
            || self.shell.pending_mcp_management.is_some()
            || self.shell.pending_plugin_management.is_some()
            || self.shell.pending_user_input.is_some()
            || self.shell.safety_buffering_modal_lines().is_some();
        (!blocked).then_some(self.shell.pointer_position).flatten()
    }

    fn render_terminal_too_narrow(&self, area: Rect, buf: &mut Buffer) {
        render_modal(
            area,
            "Terminal too narrow",
            vec![
                "Use a larger terminal window.".into(),
                Line::from(format!("Minimum width: {MIN_TERMINAL_WIDTH} columns.").dim()),
            ],
            buf,
        );
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

    fn render_request_panel(
        &self,
        area: Rect,
        title: &str,
        lines: Vec<Line<'static>>,
        background: Color,
        buf: &mut Buffer,
    ) {
        let body = body_rect_after_title(pane_content_rect(area));
        let visible_lines = visible_request_panel_lines(&lines, body.width, body.height);
        self.render_titled_panel(area, title, visible_lines, background, buf);
    }
}
