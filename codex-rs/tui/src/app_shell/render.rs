use super::ShellState;
use super::agent_activity_render::agent_activity_overview_lines;
use super::agent_activity_render::agent_activity_thread_at_line;
use super::composer_render::composer_cursor_position;
use super::composer_render::composer_visual_cursor_line;
use super::composer_render::wrapped_composer_lines;
use super::dashboard::DashboardPanel;
use super::dashboard::dashboard_panels;
use super::dashboard_help;
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
use super::input_request_view::request_panel_visual_line_count;
use super::input_request_view::user_input_lines;
use super::input_request_view::visible_request_panel_lines;
use super::modal_view::render_modal;
use super::navigation::DashboardRoute;
use super::navigation::DashboardTabs;
use super::settings::SettingsTabs;
use super::transcript_view::render_transcript;
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

const DASHBOARD_COLLAPSE_WIDTH: u16 = 100;
const DASHBOARD_MIN_WIDTH: u16 = 50;
const DASHBOARD_MAX_WIDTH: u16 = 64;
const DASHBOARD_PANEL_GAP: u16 = 1;
const DASHBOARD_WIDTH_PERCENT: u16 = 34;
const COMPACT_HEADER_HEIGHT: u16 = 2;
const PADDED_HEADER_HEIGHT: u16 = 3;
const PADDED_HEADER_MIN_SCREEN_HEIGHT: u16 = 17;
const INPUT_PANEL_MIN_HEIGHT: u16 = 6;
const INPUT_PANEL_MAX_HEIGHT: u16 = 12;
const INPUT_REQUEST_PANEL_MIN_HEIGHT: u16 = 8;
const PANE_CHROME_HEIGHT: u16 = 3;
const TRANSCRIPT_MIN_HEIGHT: u16 = 5;

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
        let layout = self.layout(area);
        self.render_header(layout.header, buf);
        if let Some(collapsed_dashboard) = layout.collapsed_dashboard {
            self.render_collapsed_dashboard(collapsed_dashboard, buf);
        }
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
    }

    pub(super) fn cursor_position(&self, area: Rect) -> Option<Position> {
        if self.shell.selector.is_some()
            || self.shell.command_palette.is_some()
            || self.shell.agent_log.is_some()
            || self.shell.tool_output.is_some()
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

    pub(super) fn transcript_output_at(&self, area: Rect, position: Position) -> Option<usize> {
        super::transcript_view::transcript_output_at(
            self.shell,
            self.layout(area).transcript,
            position,
        )
    }

    pub(super) fn pointer_pane_at(&self, area: Rect, position: Position) -> Option<PointerPane> {
        let layout = self.layout(area);
        if layout.header.contains(position) {
            Some(PointerPane::Header)
        } else if layout.transcript.contains(position) {
            Some(PointerPane::Transcript)
        } else if layout.input.contains(position) {
            Some(PointerPane::Input)
        } else if layout
            .dashboard
            .or(layout.collapsed_dashboard)
            .is_some_and(|dashboard| dashboard.contains(position))
        {
            Some(PointerPane::Dashboard)
        } else {
            None
        }
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
        let panel_gap = if layout.collapsed_dashboard.is_some() {
            0
        } else {
            DASHBOARD_PANEL_GAP
        };
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

    fn layout(&self, area: Rect) -> ShellLayout {
        let header_height = if area.height >= PADDED_HEADER_MIN_SCREEN_HEIGHT {
            PADDED_HEADER_HEIGHT
        } else {
            COMPACT_HEADER_HEIGHT
        };
        if !self.shell.dashboard_visible {
            let input_height =
                self.input_panel_height(area.height.saturating_sub(header_height), area.width);
            let main = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(header_height),
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
            let help_is_primary_content = self.shell.dashboard_route == DashboardRoute::Help;
            let dense_help = help_is_primary_content
                && dashboard_help::uses_dense_layout(usize::from(pane_content_rect(area).width));
            let dashboard_height = if help_is_primary_content {
                // Help is the active content, not incidental status. At short terminal heights,
                // give the shortcut reference priority while retaining a visible composer.
                area.height
                    .saturating_sub(header_height)
                    .min(if dense_help { 10 } else { 14 })
            } else {
                area.height
                    .saturating_sub(
                        header_height
                            .saturating_add(INPUT_PANEL_MIN_HEIGHT)
                            .saturating_add(TRANSCRIPT_MIN_HEIGHT),
                    )
                    .clamp(3, 14)
            };
            let input_height = self.input_panel_height(
                area.height
                    .saturating_sub(header_height)
                    .saturating_sub(dashboard_height),
                area.width,
            );
            let main = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(header_height),
                    Constraint::Length(dashboard_height),
                    Constraint::Min(if help_is_primary_content {
                        0
                    } else {
                        TRANSCRIPT_MIN_HEIGHT
                    }),
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
            .clamp(DASHBOARD_MIN_WIDTH, DASHBOARD_MAX_WIDTH)
            .min(area.width);
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(area.width.saturating_sub(dashboard_width)),
                Constraint::Length(dashboard_width),
            ])
            .split(area);
        let input_height = self.input_panel_height(
            area.height.saturating_sub(header_height),
            horizontal[0].width,
        );
        let main = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_height),
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
        let titles = if self.shell.active_turn_id.is_some() {
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

    fn input_panel_height(&self, available_height: u16, input_width: u16) -> u16 {
        let request_lines = if let Some(pending) = &self.shell.pending_approval {
            Some(approval_lines(pending))
        } else if let Some(pending) = &self.shell.pending_elicitation {
            Some(elicitation_lines(pending))
        } else {
            self.shell.pending_user_input.as_ref().map(|pending| {
                user_input_lines(
                    pending,
                    self.shell.composer.text(),
                    self.shell.composer.is_empty(),
                )
            })
        };
        if let Some(lines) = request_lines {
            let body_width = pane_content_rect(Rect::new(
                /*x*/ 0,
                /*y*/ 0,
                input_width,
                available_height,
            ))
            .width;
            let visual_line_count =
                u16::try_from(request_panel_visual_line_count(&lines, body_width))
                    .unwrap_or(u16::MAX);
            let desired_height = visual_line_count
                .saturating_add(PANE_CHROME_HEIGHT)
                .clamp(INPUT_REQUEST_PANEL_MIN_HEIGHT, INPUT_PANEL_MAX_HEIGHT);
            return desired_height.min(available_height);
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

        self.render_dashboard_panels(content, &panels, DASHBOARD_PANEL_GAP, buf);
    }

    fn render_collapsed_dashboard(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, palette::DARK);
        let content = pane_content_rect(area);
        let panels = dashboard_panels(self.shell, usize::from(content.width));
        self.render_dashboard_panels(content, &panels, /*panel_gap*/ 0, buf);
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
        let interactive = matches!(
            (self.shell.dashboard_route, panel.title.as_str()),
            (DashboardRoute::Sessions, "Sessions")
                | (DashboardRoute::Agents, "Agents")
                | (DashboardRoute::Status, "Settings")
        );
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
            || self.shell.pending_approval.is_some()
            || self.shell.pending_elicitation.is_some()
            || self.shell.pending_external_agent_import.is_some()
            || self.shell.pending_mcp_management.is_some()
            || self.shell.pending_plugin_management.is_some()
            || self.shell.pending_user_input.is_some()
            || self.shell.safety_buffering_modal_lines().is_some();
        (!blocked).then_some(self.shell.pointer_position).flatten()
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

#[derive(Debug, Clone, Copy)]
struct ShellLayout {
    header: Rect,
    collapsed_dashboard: Option<Rect>,
    transcript: Rect,
    input: Rect,
    dashboard: Option<Rect>,
}
