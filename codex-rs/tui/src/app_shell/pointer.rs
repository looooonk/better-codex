use super::ShellState;
use super::agent_activity_render;
use super::backend::AppShellBackend;
use super::command_palette_view;
use super::external_agent_import::ExternalAgentImportState;
use super::modal_view;
use super::navigation::DashboardRoute;
use super::render::PointerPane;
use super::render::ShellView;
use super::sessions::SessionListHit;
use super::transcript_view::TranscriptCardHit;
use crate::legacy_core::config::Config;
use crate::tui::MouseScrollDirection;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::layout::Position;
use ratatui::layout::Rect;

const TRANSCRIPT_WHEEL_SCROLL_STEP: usize = 3;
const DASHBOARD_WHEEL_SCROLL_STEP: usize = 3;

fn modal_click_key(
    area: Rect,
    position: Position,
    lines: &[ratatui::text::Line<'static>],
    click_key_at: impl FnOnce(modal_view::ModalHit) -> Option<KeyCode>,
) -> Option<KeyCode> {
    match modal_view::modal_hit(area, position, lines) {
        Some(hit) => click_key_at(hit),
        None if !modal_view::modal_panel_area(area, lines).contains(position) => Some(KeyCode::Esc),
        None => None,
    }
}

impl ShellState {
    pub(super) async fn handle_mouse_click<S>(
        &mut self,
        area: Rect,
        position: Position,
        config: &Config,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        self.input_hint = None;
        self.exit_confirmation_pending = false;
        self.set_pointer_position(position);
        if self.diff_view.is_some() {
            let selected = self
                .diff_view
                .as_ref()
                .and_then(|view| super::diff_view_view::diff_view_file_at(view, area, position));
            if let Some(index) = selected {
                self.select_diff_file(index);
            } else if !super::diff_view_view::diff_view_panel_area(area).contains(position) {
                self.close_diff_view();
            }
            return Ok(());
        }
        if let Some(preview) = &self.preview {
            if !preview.panel(area).contains(position) {
                self.preview = None;
            }
            return Ok(());
        }
        if let Some(review) = &self.tool_review {
            if !review.panel(area).contains(position) {
                self.tool_review = None;
            }
            return Ok(());
        }
        if self.tool_output.is_some() {
            if !super::tool_output_view::tool_output_panel_area(area).contains(position) {
                self.close_tool_output();
            }
            return Ok(());
        }
        if let Some(log) = &self.agent_log {
            if let Some(target) = log.links.at(position) {
                self.open_transcript_link(target, app_server);
                return Ok(());
            }
            if !super::agent_log_view::agent_log_panel_area(area).contains(position) {
                self.close_agent_log();
            }
            return Ok(());
        }
        if let Some(panel) = &self.agent_panel {
            if let Some(target) = panel.link_at(position) {
                self.open_transcript_link(target, app_server);
                return Ok(());
            }
            self.agent_panel_click(area, position, config, app_server);
            return Ok(());
        }
        if self
            .handle_selector_click(area, position, app_server)
            .await?
        {
            return Ok(());
        }
        if self.command_palette.is_some() {
            let entry = (ShellView { shell: self }).command_palette_entry_at(area, position);
            if let Some(index) = entry {
                let entries = self.command_palette_entries();
                if let Some(palette) = &mut self.command_palette {
                    palette.select(index, &entries);
                }
                self.execute_selected_command_palette_action(config, app_server)
                    .await?;
            } else if !command_palette_view::palette_area(
                area,
                self.command_palette_entries().len(),
            )
            .contains(position)
            {
                self.close_command_palette();
            }
            return Ok(());
        }
        if self.pending_account_auth.is_some() {
            let width = usize::from(modal_view::modal_body_width(area));
            let lines = self
                .pending_account_auth
                .as_ref()
                .map(|state| state.lines(width))
                .unwrap_or_default();
            let key = modal_click_key(area, position, &lines, |hit| {
                self.pending_account_auth.as_mut()?.click_key_at(hit.line)
            });
            if let Some(key) = key {
                let _exit_requested = self
                    .handle_account_auth_key(KeyEvent::new(key, KeyModifiers::NONE), app_server)
                    .await?;
            }
            return Ok(());
        }
        if let Some(lines) = self
            .pending_session_delete
            .as_ref()
            .map(super::PendingSessionDelete::lines)
        {
            let key = modal_click_key(area, position, &lines, |hit| {
                self.pending_session_delete.as_ref()?.click_key_at(hit.line)
            });
            if let Some(key) = key {
                self.handle_session_delete_key(KeyEvent::new(key, KeyModifiers::NONE), app_server)
                    .await?;
            }
            return Ok(());
        }
        if self.pending_approval.is_some() {
            if let Some(action) = (ShellView { shell: self }).approval_action_at(area, position) {
                self.handle_pending_approval_action(app_server, action)
                    .await?;
            }
            return Ok(());
        }
        if let Some(lines) = self.safety_buffering_modal_lines() {
            let key = modal_click_key(area, position, &lines, |hit| {
                self.safety_buffering_click_key(hit.line)
            });
            if let Some(key) = key {
                self.handle_safety_buffering_key(
                    KeyEvent::new(key, KeyModifiers::NONE),
                    config,
                    app_server,
                )
                .await;
            }
            return Ok(());
        }
        if let Some(lines) = self
            .pending_external_agent_import
            .as_ref()
            .map(ExternalAgentImportState::lines)
        {
            let key = modal_click_key(area, position, &lines, |hit| {
                self.pending_external_agent_import
                    .as_mut()?
                    .click_key_at(hit.line, hit.column)
            });
            if let Some(key) = key {
                self.handle_external_agent_import_key(
                    KeyEvent::new(key, KeyModifiers::NONE),
                    app_server,
                )
                .await?;
            }
            return Ok(());
        }
        if let Some((panel, _, lines)) = (ShellView { shell: self }).management_menu(area) {
            let hit = modal_view::menu_hit(panel, position, &lines);
            let outside = (!panel.contains(position)).then_some(KeyCode::Esc);
            if let Some(state) = &mut self.pending_mcp_management {
                if let Some(key) =
                    outside.or_else(|| hit.and_then(|h| state.click_key_at(h.line, h.column)))
                {
                    self.handle_mcp_management_key(
                        KeyEvent::new(key, KeyModifiers::NONE),
                        app_server,
                    )
                    .await?;
                }
            } else if let Some(state) = &mut self.pending_plugin_management
                && let Some(key) =
                    outside.or_else(|| hit.and_then(|h| state.click_key_at(h.line, h.column)))
            {
                self.handle_plugin_management_key(
                    KeyEvent::new(key, KeyModifiers::NONE),
                    app_server,
                )
                .await?;
            }
            return Ok(());
        }
        if self.pending_elicitation.is_some() {
            if let Some(choice) = (ShellView { shell: self }).elicitation_choice_at(area, position)
            {
                self.resolve_pending_elicitation(app_server, choice).await?;
            }
            return Ok(());
        }
        if self.pending_user_input.is_some() {
            if let Some(hit) = (ShellView { shell: self })
                .question_popup(area)
                .and_then(|popup| popup.hit(position))
            {
                self.handle_question_hit(hit, app_server).await?;
            }
            return Ok(());
        }
        if self.rewind.is_active() {
            return Ok(());
        }
        let queued_message_popup_hit = {
            let view = ShellView { shell: self };
            super::queued_message_popup_view::hit_at(
                self,
                view.transcript_area(area),
                view.input_area(area),
                position,
            )
        };
        match queued_message_popup_hit {
            Some(super::queued_message_popup_view::QueuedMessagePopupHit::Message(index)) => {
                self.session_list.focused = false;
                self.settings.focused = false;
                self.agents_focused = false;
                self.clear_transcript_selection();
                if !self.composer.edit_queued_message(index) {
                    self.show_hint("structured queued messages cannot be edited");
                }
                self.sync_composer_queue_edits(app_server);
                return Ok(());
            }
            Some(super::queued_message_popup_view::QueuedMessagePopupHit::Chrome) => return Ok(()),
            None => {}
        }
        if let Some(control) = (ShellView { shell: self }).composer_control_at(area, position) {
            self.open_composer_picker(control);
            return Ok(());
        }
        if let Some(destination) =
            (ShellView { shell: self }).transcript_hyperlink_at(area, position)
        {
            self.open_transcript_link(destination, app_server);
            return Ok(());
        }
        if let Some(card) = (ShellView { shell: self }).transcript_card_at(area, position) {
            match card {
                TranscriptCardHit::Review {
                    transcript_index,
                    review_index,
                } => self.open_tool_review(transcript_index, review_index),
                TranscriptCardHit::ToolGroup { transcript_index } => {
                    let pane = (ShellView { shell: self }).transcript_area(area);
                    super::transcript_view::toggle_tool_group_at(self, pane, transcript_index);
                }
                TranscriptCardHit::ToolOutput { transcript_index } => {
                    self.open_tool_output_at(transcript_index);
                }
                TranscriptCardHit::Diff { transcript_index } => {
                    self.open_diff_view_at(transcript_index);
                }
            }
            return Ok(());
        }
        if (ShellView { shell: self })
            .input_area(area)
            .contains(position)
        {
            self.session_list.focused = false;
            self.settings.focused = false;
            self.agents_focused = false;
            self.clear_transcript_selection();
            return Ok(());
        }
        if let Some(route) = (ShellView { shell: self }).dashboard_route_at(area, position) {
            self.set_dashboard_route(route);
            self.session_list.focused = false;
            self.settings.focused = false;
            self.agents_focused = false;
            if route == DashboardRoute::Sessions {
                self.start_session_list_refresh(app_server);
            }
            return Ok(());
        }

        let view = ShellView { shell: self };
        if self.dashboard_route == DashboardRoute::Settings
            && let Some(target) = view.dashboard_panel_position_at(area, position, "Settings")
        {
            if self.settings.editing() && self.settings.selected_line_range().contains(&target.line)
            {
                self.settings.focused = true;
            } else if let Some(action) = self.settings.action_at_line(target.line) {
                self.settings.focused = true;
                self.settings.focus_action(action);
                self.activate_selected_setting(app_server).await?;
            }
            return Ok(());
        }
        if self.dashboard_route == DashboardRoute::Status
            && view
                .dashboard_panel_position_at(area, position, "Edits")
                .is_some()
        {
            self.open_session_diff_view();
            return Ok(());
        }
        if self.dashboard_route == DashboardRoute::Sessions
            && let Some(target) = view.dashboard_panel_position_at(area, position, "Projects")
        {
            let rows = self.projects.rows(target.width);
            if let Some(row) = rows.get(target.line) {
                let action = if target.on_more_button() {
                    row.more.as_ref().or(row.action.as_ref())
                } else {
                    row.action.as_ref()
                };
                if let Some(action) = action.cloned() {
                    self.session_list.focused = true;
                    self.projects.selected = Some(target.line);
                    self.activate_project(action, config, app_server).await;
                }
            }
            return Ok(());
        }
        if self.dashboard_route == DashboardRoute::Sessions
            && let Some(target) = view.dashboard_panel_position_at(area, position, "Sessions")
        {
            self.projects.selected = None;
            match self.session_list.hit_at_line(target.line) {
                Some(SessionListHit::NewSession) => {
                    self.start_new_session(config, app_server).await?;
                }
                Some(SessionListHit::Thread(thread_id)) => {
                    if target.on_more_button() && !self.session_list.show_archived() {
                        self.project_menu(super::project_hub::HubView::Move(thread_id));
                    } else {
                        self.resume_session(config, app_server, thread_id).await;
                    }
                }
                None => {}
            }
            return Ok(());
        }
        if self.dashboard_route == DashboardRoute::Agents
            && let Some(target) = view.dashboard_panel_position_at(area, position, "Agents")
        {
            self.agents_focused = true;
            if target.line == 0 {
                self.open_agent_panel(app_server);
                return Ok(());
            }
            let overview_height = agent_activity_render::agent_activity_overview_lines(
                &self.agent_activity,
                target.width,
            )
            .len();
            let thread_id = target
                .line
                .checked_sub(overview_height + 1)
                .and_then(|line| {
                    agent_activity_render::agent_activity_thread_at_line(
                        &self.agent_activity,
                        line,
                        /*line_budget*/ 23,
                    )
                    .map(ToString::to_string)
                });
            if let Some(thread_id) = thread_id {
                self.agent_activity.select_thread(&thread_id);
                self.open_selected_agent_log(config, app_server);
            }
        }
        Ok(())
    }

    fn open_transcript_link(
        &mut self,
        destination: crate::terminal_hyperlinks::ClickTarget,
        app_server: &impl AppShellBackend,
    ) {
        use crate::terminal_hyperlinks::ClickTarget;
        let destination = match destination {
            ClickTarget::External(url) => url,
            ClickTarget::Source(link) => {
                let thread_id = self.thread_id;
                let request = app_server.workspace_request_in_background(
                    thread_id,
                    super::workspace_requests::WorkspaceRequest::ReadSource(link),
                );
                self.start_backend_action(
                    super::backend_actions::ActionGroup::Workspace,
                    "opening source",
                    async move {
                        super::backend_actions::BackendActionResult::Workspace {
                            thread_id,
                            result: request.await,
                        }
                    },
                );
                return;
            }
        };
        match webbrowser::open(&destination) {
            Ok(()) => self.show_hint("opened link in browser"),
            Err(error) => {
                tracing::warn!(%destination, "failed to open transcript URL: {error}");
                self.push_error(format!("Could not open link: {error}"));
            }
        }
    }

    pub(super) fn set_pointer_position(&mut self, position: Position) -> bool {
        let changed = self.pointer_position != Some(position);
        self.pointer_position = Some(position);
        changed
    }

    pub(super) fn clear_pointer_position(&mut self) {
        self.pointer_position = None;
    }

    pub(super) fn handle_mouse_scroll(
        &mut self,
        area: Rect,
        position: Position,
        direction: MouseScrollDirection,
    ) -> bool {
        self.exit_confirmation_pending = false;
        self.set_pointer_position(position);
        let key = match direction {
            MouseScrollDirection::Up => KeyCode::Up,
            MouseScrollDirection::Down => KeyCode::Down,
        };
        if self.diff_view.is_some() {
            if super::diff_view_view::diff_view_file_selector_area(area).contains(position) {
                if let Some(view) = &mut self.diff_view {
                    match direction {
                        MouseScrollDirection::Up => {
                            view.select_previous_file();
                        }
                        MouseScrollDirection::Down => {
                            view.select_next_file();
                        }
                    }
                }
            } else {
                match direction {
                    MouseScrollDirection::Up => self.scroll_diff_view_up(),
                    MouseScrollDirection::Down => self.scroll_diff_view_down(),
                }
            }
            return false;
        }
        if let Some(preview) = &mut self.preview {
            preview.scroll(key);
            return false;
        }
        if let Some(review) = &mut self.tool_review {
            review.scroll(key);
            return false;
        }
        if self.tool_output.is_some() {
            match direction {
                MouseScrollDirection::Up => self.scroll_tool_output_up(),
                MouseScrollDirection::Down => self.scroll_tool_output_down(),
            }
            return false;
        }
        if self.agent_log.is_some() {
            match direction {
                MouseScrollDirection::Up => self.scroll_agent_log_up(),
                MouseScrollDirection::Down => self.scroll_agent_log_down(),
            }
            return false;
        }
        if self.agent_panel.is_some() {
            self.scroll_agent_panel(key);
            return false;
        }
        if let Some((panel, _, _)) = (ShellView { shell: self }).management_menu(area) {
            if panel.contains(position) {
                if let Some(state) = &mut self.pending_mcp_management
                    && !state.editing()
                {
                    match direction {
                        MouseScrollDirection::Up => state.move_up(),
                        MouseScrollDirection::Down => state.move_down(),
                    }
                }
                if let Some(state) = &mut self.pending_plugin_management {
                    match direction {
                        MouseScrollDirection::Up => state.move_up(),
                        MouseScrollDirection::Down => state.move_down(),
                    }
                }
            }
            return false;
        }
        let selector_area = (ShellView { shell: self }).selector_area(area);
        if let Some(selector) = &mut self.selector {
            if selector.option_at(selector_area, position).is_some() {
                selector.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
            }
            return false;
        }
        if self.command_palette.is_some() {
            let over_entry = (ShellView { shell: self })
                .command_palette_entry_at(area, position)
                .is_some();
            if over_entry {
                let entries = self.command_palette_entries();
                if let Some(palette) = &mut self.command_palette {
                    match direction {
                        MouseScrollDirection::Up => palette.move_up(&entries),
                        MouseScrollDirection::Down => palette.move_down(&entries),
                    }
                }
            }
            return false;
        }
        let over_approval = (ShellView { shell: self })
            .approval_area(area)
            .is_some_and(|popup| popup.contains(position));
        if over_approval {
            if let Some(pending) = &mut self.pending_approval {
                match direction {
                    MouseScrollDirection::Up => pending.scroll_up(/*amount*/ 3),
                    MouseScrollDirection::Down => pending.scroll_down(/*amount*/ 3),
                }
            }
            return false;
        }
        if let Some(popup) = (ShellView { shell: self }).question_popup(area)
            && popup.panel.contains(position)
            && let Some(pending) = &self.pending_user_input
        {
            popup.scroll(
                pending,
                match direction {
                    MouseScrollDirection::Up => -3,
                    MouseScrollDirection::Down => 3,
                },
            );
            return false;
        }
        let over_elicitation = self.pending_elicitation.is_some()
            && (ShellView { shell: self })
                .input_area(area)
                .contains(position);
        if over_elicitation {
            if let Some(pending) = &mut self.pending_elicitation {
                match direction {
                    MouseScrollDirection::Up => pending.scroll_up(/*amount*/ 3),
                    MouseScrollDirection::Down => pending.scroll_down(/*amount*/ 3),
                }
            }
            return false;
        }
        if self.has_blocking_overlay() {
            return false;
        }

        match (ShellView { shell: self }).pointer_pane_at(area, position) {
            Some(PointerPane::Transcript) => {
                match direction {
                    MouseScrollDirection::Up => {
                        self.scroll_transcript_up(TRANSCRIPT_WHEEL_SCROLL_STEP)
                    }
                    MouseScrollDirection::Down => {
                        self.scroll_transcript_down(TRANSCRIPT_WHEEL_SCROLL_STEP)
                    }
                }
                false
            }
            Some(PointerPane::Dashboard) => self.scroll_dashboard_at(area, position, direction),
            Some(PointerPane::Input) => {
                match direction {
                    MouseScrollDirection::Up => self.composer.move_up(),
                    MouseScrollDirection::Down => self.composer.move_down(),
                };
                false
            }
            None => false,
        }
    }

    fn has_blocking_overlay(&self) -> bool {
        self.diff_view.is_some()
            || self.tool_detail_open()
            || self.preview.is_some()
            || self.agent_log.is_some()
            || self.agent_panel.is_some()
            || self.pending_approval.is_some()
            || self.pending_session_delete.is_some()
            || self.pending_elicitation.is_some()
            || self.pending_external_agent_import.is_some()
            || self.pending_mcp_management.is_some()
            || self.pending_plugin_management.is_some()
            || self.pending_user_input.is_some()
            || self.safety_buffering_modal_lines().is_some()
    }

    fn scroll_dashboard_at(
        &mut self,
        area: Rect,
        position: Position,
        direction: MouseScrollDirection,
    ) -> bool {
        let nested_panel = match self.dashboard_route {
            DashboardRoute::Sessions => Some("Sessions"),
            DashboardRoute::Agents => Some("Agents"),
            DashboardRoute::Status | DashboardRoute::Settings => None,
        };
        let over_nested_panel = nested_panel.is_some_and(|title| {
            (ShellView { shell: self })
                .dashboard_panel_position_at(area, position, title)
                .is_some()
        });
        let scroll_back_to_projects = self.dashboard_route == DashboardRoute::Sessions
            && !self.session_list.renaming()
            && (self.session_list.visible_rows() == 0
                || (direction == MouseScrollDirection::Up
                    && self.session_list.at_top()
                    && self.dashboard_scroll.get() > 0));
        if over_nested_panel && !scroll_back_to_projects {
            return match (self.dashboard_route, direction) {
                (DashboardRoute::Sessions, MouseScrollDirection::Up)
                    if !self.session_list.renaming() =>
                {
                    self.session_list.scroll_up();
                    false
                }
                (DashboardRoute::Sessions, MouseScrollDirection::Down)
                    if !self.session_list.renaming() =>
                {
                    self.session_list.scroll_down()
                }
                (DashboardRoute::Agents, MouseScrollDirection::Up) => {
                    self.agent_activity.move_selection_up();
                    false
                }
                (DashboardRoute::Agents, MouseScrollDirection::Down) => {
                    self.agent_activity.move_selection_down();
                    false
                }
                (
                    DashboardRoute::Sessions | DashboardRoute::Status | DashboardRoute::Settings,
                    _,
                ) => false,
            };
        }

        let max_scroll = (ShellView { shell: self }).dashboard_scroll_max(area);
        let scroll = self.dashboard_scroll.get().min(max_scroll);
        self.dashboard_scroll.set(match direction {
            MouseScrollDirection::Up => scroll.saturating_sub(DASHBOARD_WHEEL_SCROLL_STEP),
            MouseScrollDirection::Down => scroll
                .saturating_add(DASHBOARD_WHEEL_SCROLL_STEP)
                .min(max_scroll),
        });
        false
    }
}

#[cfg(test)]
#[path = "pointer_tests.rs"]
mod tests;
