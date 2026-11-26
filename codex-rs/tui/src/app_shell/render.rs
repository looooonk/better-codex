use super::ShellState;
use super::composer_layout::composer_body;
use super::composer_layout::composer_viewport;
use super::composer_render::composer_lines;
use super::composer_render::viewport_cursor_position;
use super::composer_toolbar::ComposerControl;
use super::composer_toolbar::ComposerToolbar;
use super::dashboard_view;
use super::dashboard_view::DashboardPanelPosition;
use super::design::body_rect_after_title;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::title_rect;
use super::input_request_layout::elicitation_panel_hit;
use super::input_request_layout::request_panel_hit;
use super::input_request_layout::visible_elicitation_panel_lines;
use super::input_request_layout::visible_request_panel_lines;
use super::input_request_view::user_input_lines;
use super::modal_view::render_modal;
use super::navigation::DashboardRoute;
use super::shell_layout;
use super::shell_layout::MIN_TERMINAL_HEIGHT;
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
    shell.pets.clear()?;
    shell.refresh_terminal_title();
    if shell.terminal_clear_requested.get() {
        shell.graphics.borrow_mut().viewport_cleared();
        tui.terminal.clear()?;
        shell.terminal_clear_requested.set(false);
    }
    let height = tui.terminal.size()?.height;
    shell.graphics.borrow_mut().refresh_cell();
    tui.draw_with_graphics(
        height,
        |frame| {
            shell.graphics.borrow_mut().begin(frame.area());
            let view = ShellView { shell };
            let area = frame.area();
            view.render(area, frame.buffer);
            shell.graphics.borrow_mut().compose(frame.buffer);
            if let Some(position) = view.cursor_position(area) {
                frame.set_cursor_style(SetCursorStyle::SteadyBar);
                frame.set_cursor_position(position);
            }
        },
        || shell.graphics.borrow_mut().present(&mut std::io::stdout()),
    )?;
    let size = tui.terminal.size()?;
    shell.draw_pet_image(Rect::new(
        /*x*/ 0,
        /*y*/ 0,
        size.width,
        size.height,
    ))
}

pub(super) struct ShellView<'a> {
    pub(super) shell: &'a ShellState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PointerPane {
    Transcript,
    Input,
    Dashboard,
}

impl ShellView<'_> {
    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        let _active_theme = crate::app_theme::activate(self.shell.app_theme);
        fill_rect(buf, area, palette::base());
        let Some(layout) = self.layout(area) else {
            self.render_terminal_too_small(area, buf);
            return;
        };
        if let Some(area) = layout.status {
            self.shell.render_status_surface(area, buf);
        }
        if self.shell.shows_start_logo()
            || self
                .shell
                .has_pending_backend_action(super::backend_actions::ActionGroup::SessionSwitch)
        {
            super::start_logo::render(self.shell, layout, self.base_hover_position(), buf);
        } else {
            render_transcript(
                self.shell,
                layout.transcript,
                self.base_hover_position(),
                buf,
            );
            if let Some(selection) = self.shell.transcript_text_selection() {
                render_transcript_text_selection(self.shell, layout.transcript, selection, buf);
            }
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
        super::queued_message_popup_view::render(self.shell, layout.transcript, layout.input, buf);
        super::slash_command_popup_view::render(self.shell, layout.transcript, layout.input, buf);
        if let Some(pending) = &self.shell.pending_approval {
            super::approval_popup::render(pending, layout.transcript, buf);
        }
        if let Some(pending) = &self.shell.pending_external_agent_import {
            render_modal(area, "Claude Code Import", pending.lines(), buf);
        }
        if let Some((panel, title, lines)) = self.management_menu(area) {
            super::modal_view::render_settings_menu(
                panel,
                title,
                lines,
                self.shell.pointer_position,
                buf,
            );
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
            selector.render(self.selector_area(area), self.shell.pointer_position, buf);
        }
        if let Some(log) = &self.shell.agent_log {
            super::agent_log_view::render_agent_log(log, area, buf);
        }
        if let Some(output) = &self.shell.tool_output {
            super::tool_output_view::render_tool_output(output, area, buf);
        }
        if let Some(review) = &self.shell.tool_review {
            review.render(area, buf);
        }
        if let Some(preview) = &self.shell.source_preview {
            preview.render(area, buf);
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
            || self.shell.tool_detail_open()
            || self.shell.source_preview.is_some()
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
        let body = composer_body(input);
        let mut cache = self.shell.composer_layout.borrow_mut();
        let layout = cache.for_draft(composer.text(), usize::from(body.width));
        viewport_cursor_position(&composer_viewport(body, layout.cursor(composer.cursor()))?)
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

    pub(super) fn transcript_hyperlink_at(
        &self,
        area: Rect,
        position: Position,
    ) -> Option<crate::terminal_hyperlinks::ClickTarget> {
        let layout = self.layout(area)?;
        if layout
            .dashboard
            .is_some_and(|dashboard| dashboard.area().contains(position))
        {
            return None;
        }
        super::transcript_view::transcript_hyperlink_at(self.shell, layout.transcript, position)
    }

    pub(super) fn pointer_pane_at(&self, area: Rect, position: Position) -> Option<PointerPane> {
        let layout = self.layout(area)?;
        if layout
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

    fn settings_menu_area(&self, screen: Rect, width: u16, height: u16) -> Rect {
        let anchor = self
            .layout(screen)
            .and_then(|layout| layout.dashboard)
            .and_then(|placement| dashboard_view::selected_setting_area(self.shell, placement))
            .unwrap_or_else(|| Rect::new(screen.x, screen.y, screen.width, 1));
        let width = width.min(screen.width);
        let x = anchor.x.min(screen.right().saturating_sub(width));
        let below = screen.bottom().saturating_sub(anchor.bottom());
        let above = anchor.y.saturating_sub(screen.y);
        if below >= height || below >= above {
            Rect::new(x, anchor.bottom(), width, height.min(below))
        } else {
            let height = height.min(above);
            Rect::new(x, anchor.y - height, width, height)
        }
    }

    pub(super) fn management_menu(
        &self,
        screen: Rect,
    ) -> Option<(Rect, &'static str, Vec<Line<'static>>)> {
        let width = 54.min(screen.width);
        let (title, lines) = if let Some(mcp) = &self.shell.pending_mcp_management {
            (
                "MCP servers",
                mcp.lines_for_width(usize::from(width.saturating_sub(2))),
            )
        } else {
            (
                "Plugins",
                self.shell.pending_plugin_management.as_ref()?.lines(),
            )
        };
        let height = super::modal_view::settings_menu_height(&lines, width);
        Some((self.settings_menu_area(screen, width, height), title, lines))
    }

    pub(super) fn selector_area(&self, screen: Rect) -> Rect {
        let Some(selector) = &self.shell.selector else {
            return screen;
        };
        if let Some(height) = selector.settings_menu_height() {
            return self.settings_menu_area(screen, 40, height);
        }
        let Some(control) = selector.composer_control() else {
            return screen;
        };
        let input = self.input_area(screen);
        let toolbar = ComposerToolbar::new(self.shell, input);
        let anchor = toolbar.button_area(control);
        let top = toolbar.button_area(ComposerControl::Permissions).y;
        let (width, height) = selector.popup_size(control, input.width);
        let height = height.min(top.saturating_sub(screen.y));
        let x = match control {
            ComposerControl::Permissions => anchor.x,
            ComposerControl::Model | ComposerControl::ReasoningEffort => {
                (anchor.x + anchor.width / 2).saturating_sub(width / 2)
            }
            ComposerControl::ServiceTier => anchor.right().saturating_sub(width),
        }
        .clamp(input.x, input.right().saturating_sub(width));
        Rect::new(x, top.saturating_sub(height), width, height)
    }

    pub(super) fn composer_control_at(
        &self,
        area: Rect,
        position: Position,
    ) -> Option<ComposerControl> {
        let input = self.layout(area)?.input;
        ComposerToolbar::new(self.shell, input).control_at(position)
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

    pub(super) fn approval_area(&self, area: Rect) -> Option<Rect> {
        let pending = self.shell.pending_approval.as_ref()?;
        Some(super::approval_popup::area(
            pending,
            self.layout(area)?.transcript,
        ))
    }

    pub(super) fn approval_action_at(
        &self,
        area: Rect,
        position: Position,
    ) -> Option<super::ApprovalAction> {
        let pending = self.shell.pending_approval.as_ref()?;
        super::approval_popup::action_at(pending, self.approval_area(area)?, position)
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

    fn render_input(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, palette::surface());
        let border_color = if self.shell.rewind.is_active() {
            palette::warning()
        } else if self.shell.dashboard_focused() {
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

        // Keep transient draft information on the border so the input always starts on row one.
        let mut hints = Vec::new();
        if let Some(hint) = self.shell.rewind_input_hint() {
            hints.push(hint.to_owned());
        }
        if let Some((index, count)) = self.shell.composer.queued_edit_position() {
            hints.push(format!("EDIT {index}/{count}"));
        }
        if self.shell.composer.has_images() {
            hints.push(self.shell.composer.image_summary());
        }
        if self.shell.pending_approval.is_none()
            && let Some(hint) = &self.shell.input_hint
        {
            hints.insert(0, hint.clone());
        }
        if !hints.is_empty() {
            Paragraph::new(format!(" {} ", hints.join("  ")))
                .fg(border_color)
                .render(
                    Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), 1),
                    buf,
                );
        }
        let body = composer_body(area);
        let composer = self.shell.composer.display();
        let mut cache = self.shell.composer_layout.borrow_mut();
        let wrap = cache.for_draft(composer.text(), usize::from(body.width));
        if let Some(viewport) = composer_viewport(body, wrap.cursor(composer.cursor())) {
            let end = viewport
                .cursor
                .line_count
                .min(viewport.visible_start + viewport.visible_height);
            let lines = composer_lines(
                composer.text(),
                self.shell.composer.is_empty(),
                wrap,
                composer.selection_range(),
                viewport.visible_start..end,
            );
            Paragraph::new(lines).render(body, buf);
        }
        drop(cache);
        ComposerToolbar::new(self.shell, area).render(self.base_hover_position(), buf);
    }

    fn base_hover_position(&self) -> Option<Position> {
        let blocked = self.shell.selector.is_some()
            || self.shell.command_palette.is_some()
            || self.shell.pending_account_auth.is_some()
            || self.shell.agent_log.is_some()
            || self.shell.tool_detail_open()
            || self.shell.source_preview.is_some()
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

    fn render_terminal_too_small(&self, area: Rect, buf: &mut Buffer) {
        render_modal(
            area,
            "Terminal too small",
            vec![
                "Use a larger terminal window.".into(),
                Line::from(
                    format!("Minimum: {MIN_TERMINAL_WIDTH} columns x {MIN_TERMINAL_HEIGHT} rows.")
                        .dim(),
                ),
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
