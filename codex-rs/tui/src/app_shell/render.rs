use super::ShellState;
use super::composer_render::composer_cursor_position;
use super::composer_render::composer_visual_cursor_line;
use super::composer_render::wrapped_composer_lines_with_selection;
use super::dashboard_view;
use super::dashboard_view::DashboardPanelPosition;
use super::design::body_rect_after_title;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::title_rect;
use super::header::HeaderControl;
use super::header::HeaderView;
use super::input_request_layout::approval_panel_hit;
use super::input_request_layout::elicitation_panel_hit;
use super::input_request_layout::request_panel_hit;
use super::input_request_layout::visible_approval_panel_lines;
use super::input_request_layout::visible_elicitation_panel_lines;
use super::input_request_layout::visible_request_panel_lines;
use super::input_request_view::approval_lines;
use super::input_request_view::user_input_lines;
use super::modal_view::modal_body_width;
use super::modal_view::render_modal;
use super::navigation::DashboardRoute;
use super::shell_layout;
use super::shell_layout::MIN_TERMINAL_WIDTH;
use super::shell_layout::ShellLayout;
use super::transcript_view::render_transcript;
use super::transcript_view::render_transcript_text_selection;
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
pub(super) enum PointerPane {
    Header,
    Transcript,
    Input,
    Dashboard,
}

impl ShellView<'_> {
    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        let _active_theme = crate::app_theme::activate(self.shell.app_theme);
        fill_rect(buf, area, palette::base());
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
        if let Some(selection) = self.shell.transcript_text_selection() {
            render_transcript_text_selection(self.shell, layout.transcript, selection, buf);
        }
        self.render_input(layout.input, buf);
        if let Some(dashboard) = layout.dashboard {
            dashboard_view::render_dashboard(
                self.shell,
                dashboard,
                self.base_hover_position(),
                buf,
            );
        }
        if let Some(pending) = &self.shell.pending_external_agent_import {
            render_modal(area, "Claude Code Import", pending.lines(), buf);
        }
        if let Some(pending) = &self.shell.pending_mcp_management {
            let width = usize::from(modal_body_width(area));
            render_modal(area, "MCP Servers", pending.lines_for_width(width), buf);
        }
        if let Some(pending) = &self.shell.pending_plugin_management {
            render_modal(area, "Plugins", pending.lines(), buf);
        }
        if let Some(lines) = self.shell.safety_buffering_modal_lines() {
            render_modal(area, "Safety review", lines, buf);
        }
        if let Some(pending) = &self.shell.pending_session_delete {
            render_modal(area, "Delete session", pending.lines(), buf);
        }
        super::command_palette_view::render(self.shell, area, buf);
        if let Some(state) = &self.shell.pending_account_auth {
            super::account_auth::render(state, area, buf);
        }
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
            || self.shell.pending_account_auth.is_some()
            || self.shell.agent_log.is_some()
            || self.shell.tool_output.is_some()
            || self.shell.diff_view.is_some()
            || self.shell.pending_approval.is_some()
            || self.shell.pending_session_delete.is_some()
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
        let composer = self.shell.composer.display();
        composer_cursor_position(input, composer.text(), composer.cursor())
    }

    pub(super) fn input_area(&self, area: Rect) -> Rect {
        self.layout(area)
            .map_or(Rect::default(), |layout| layout.input)
    }

    pub(super) fn transcript_area(&self, area: Rect) -> Rect {
        self.layout(area)
            .map_or(Rect::default(), |layout| layout.transcript)
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
        dashboard_view::route_at(self.shell, self.layout(area)?.dashboard?, position)
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
            turn_elapsed_seconds: self.shell.active_turn_elapsed_seconds(),
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
        dashboard_view::panel_position_at(
            self.shell,
            self.layout(area)?.dashboard?,
            position,
            title,
        )
    }

    pub(super) fn dashboard_scroll_max(&self, area: Rect) -> usize {
        self.layout(area)
            .and_then(|layout| layout.dashboard)
            .map_or(0, |dashboard| {
                dashboard_view::max_scroll(self.shell, dashboard)
            })
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
        let hit = approval_panel_hit(self.input_area(area), position, pending)?;
        let option_start = pending.details().len().saturating_add(1);
        if (option_start..option_start.saturating_add(pending.option_count())).contains(&hit.line) {
            return Some(super::ApprovalAction::Choose(hit.line - option_start));
        }
        if hit.line != lines.len().saturating_sub(1) {
            return None;
        }
        let text = lines[hit.line]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        [
            ("e Edit", Some(super::ApprovalAction::Edit)),
            ("? Explain", Some(super::ApprovalAction::Explain)),
        ]
        .into_iter()
        .find_map(|(label, action)| {
            let byte_start = text.find(label)?;
            let start = UnicodeWidthStr::width(&text[..byte_start]);
            (start..start + UnicodeWidthStr::width(label))
                .contains(&hit.column)
                .then_some(action)
                .flatten()
        })
    }

    pub(super) fn elicitation_choice_at(
        &self,
        area: Rect,
        position: Position,
    ) -> Option<super::ElicitationChoice> {
        let pending = self.shell.pending_elicitation.as_ref()?;
        let hit = elicitation_panel_hit(
            self.input_area(area),
            position,
            pending,
            &self.shell.composer,
        )?;
        pending.choice_at(hit.line, hit.column)
    }

    pub(super) fn user_input_option_at(&self, area: Rect, position: Position) -> Option<usize> {
        let pending = self.shell.pending_user_input.as_ref()?;
        let input_area = self.input_area(area);
        let width = body_rect_after_title(pane_content_rect(input_area)).width;
        let lines = user_input_lines(pending, &self.shell.composer, width);
        let hit = request_panel_hit(input_area, position, &lines)?;
        let option_index = hit.line.checked_sub(2)?;
        let option = pending
            .current_question()?
            .options
            .as_ref()?
            .get(option_index)?;
        let text = lines[hit.line]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        let label = format!("{} {}", option_index + 1, option.label);
        let byte_start = text.find(&label)?;
        let start = UnicodeWidthStr::width(&text[..byte_start]);
        (start..start + UnicodeWidthStr::width(label.as_str()))
            .contains(&hit.column)
            .then_some(option_index)
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
            turn_elapsed_seconds: self.shell.active_turn_elapsed_seconds(),
            dashboard_visible: self.shell.dashboard_visible,
        };
        let hovered = self
            .base_hover_position()
            .and_then(|position| view.control_at(area, position));
        view.render(area, hovered, buf);
    }

    fn render_input(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, palette::surface());
        let border_color = if self.shell.dashboard_focused() {
            palette::border()
        } else {
            palette::focus()
        };
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(border_color))
            .style(pane_style(palette::surface()))
            .render(area, buf);
        if let Some(pending) = &self.shell.pending_approval {
            let body = body_rect_after_title(pane_content_rect(area));
            let lines = visible_approval_panel_lines(pending, body.width, body.height);
            self.render_titled_panel(area, "APPROVAL", lines, palette::surface(), buf);
            return;
        }
        if let Some(pending) = &self.shell.pending_elicitation {
            let body = body_rect_after_title(pane_content_rect(area));
            let lines = visible_elicitation_panel_lines(
                pending,
                &self.shell.composer,
                body.width,
                body.height,
            );
            self.render_titled_panel(area, "MCP ELICITATION", lines, palette::surface(), buf);
            return;
        }
        if let Some(pending) = &self.shell.pending_user_input {
            let width = body_rect_after_title(pane_content_rect(area)).width;
            self.render_request_panel(
                area,
                "TOOL INPUT",
                user_input_lines(pending, &self.shell.composer, width),
                palette::surface(),
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
        let composer = self.shell.composer.display();
        let mut lines = wrapped_composer_lines_with_selection(
            composer.text(),
            self.shell.composer.is_empty(),
            composer.cursor(),
            usize::from(body.width).max(1),
            composer.selection_range(),
        );
        if visible_height > 0 && lines.len() > visible_height {
            let max_start = lines.len().saturating_sub(visible_height);
            let cursor_line = composer_visual_cursor_line(
                composer.text(),
                composer.cursor(),
                usize::from(body.width).max(1),
            )
            .unwrap_or(line);
            let start = cursor_line
                .saturating_add(1)
                .saturating_sub(visible_height)
                .min(max_start);
            lines = lines.into_iter().skip(start).take(visible_height).collect();
        }
        self.render_titled_panel(area, &title, lines, palette::surface(), buf);
    }

    fn base_hover_position(&self) -> Option<Position> {
        let blocked = self.shell.selector.is_some()
            || self.shell.command_palette.is_some()
            || self.shell.pending_account_auth.is_some()
            || self.shell.agent_log.is_some()
            || self.shell.tool_output.is_some()
            || self.shell.diff_view.is_some()
            || self.shell.pending_approval.is_some()
            || self.shell.pending_session_delete.is_some()
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
            "◆ ".set_style(Style::new().fg(palette::focus())),
            title
                .to_string()
                .set_style(Style::new().fg(palette::text()).bold()),
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
