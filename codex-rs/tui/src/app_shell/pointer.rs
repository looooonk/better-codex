use super::ShellState;
use super::agent_activity_render;
use super::backend::AppShellBackend;
use super::command_palette_view;
use super::external_agent_import::ExternalAgentImportState;
use super::header::HeaderControl;
use super::mcp_management::McpManagementState;
use super::modal_view;
use super::navigation::DashboardRoute;
use super::plugin_management::PluginManagementState;
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
        if self.tool_output.is_some() {
            if !super::tool_output_view::tool_output_panel_area(area).contains(position) {
                self.close_tool_output();
            }
            return Ok(());
        }
        if self.agent_log.is_some() {
            if !super::agent_log_view::agent_log_panel_area(area).contains(position) {
                self.close_agent_log();
            }
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
        if let Some(lines) = self
            .pending_mcp_management
            .as_ref()
            .map(McpManagementState::lines)
        {
            let key = modal_click_key(area, position, &lines, |hit| {
                self.pending_mcp_management
                    .as_mut()?
                    .click_key_at(hit.line, hit.column)
            });
            if let Some(key) = key {
                self.handle_mcp_management_key(KeyEvent::new(key, KeyModifiers::NONE), app_server)
                    .await?;
            }
            return Ok(());
        }
        if let Some(lines) = self
            .pending_plugin_management
            .as_ref()
            .map(PluginManagementState::lines)
        {
            let key = modal_click_key(area, position, &lines, |hit| {
                self.pending_plugin_management
                    .as_mut()?
                    .click_key_at(hit.line, hit.column)
            });
            if let Some(key) = key {
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
            if let Some(index) = (ShellView { shell: self }).user_input_option_at(area, position) {
                self.composer.set_text((index + 1).to_string());
                self.resolve_pending_user_input(app_server).await?;
            }
            return Ok(());
        }
        if let Some(control) = (ShellView { shell: self }).header_control_at(area, position) {
            match control {
                HeaderControl::Dashboard => self.toggle_dashboard(),
                HeaderControl::Model => self.open_model_selector(),
                HeaderControl::ReasoningEffort => self.open_reasoning_selector(),
                HeaderControl::ServiceTier => self.open_service_tier_selector(),
            }
            return Ok(());
        }
        if let Some(card) = (ShellView { shell: self }).transcript_card_at(area, position) {
            match card {
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
        if self.dashboard_route == DashboardRoute::Status
            && let Some(target) = view.dashboard_panel_position_at(area, position, "Settings")
        {
            if self
                .settings
                .select_at(target.line, target.column, target.width)
            {
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
            && let Some(target) = view.dashboard_panel_position_at(area, position, "Sessions")
        {
            match self.session_list.hit_at_line(target.line) {
                Some(SessionListHit::NewSession) => {
                    self.start_new_session(config, app_server).await?;
                }
                Some(SessionListHit::Thread(thread_id)) => {
                    self.resume_session(config, app_server, thread_id).await?;
                }
                None => {}
            }
            return Ok(());
        }
        if self.dashboard_route == DashboardRoute::Agents
            && let Some(target) = view.dashboard_panel_position_at(area, position, "Agents")
        {
            self.agents_focused = true;
            let overview_height = agent_activity_render::agent_activity_overview_lines(
                &self.agent_activity,
                target.width,
            )
            .len();
            let thread_id = target.line.checked_sub(overview_height).and_then(|line| {
                agent_activity_render::agent_activity_thread_at_line(
                    &self.agent_activity,
                    line,
                    /*line_budget*/ 24,
                )
                .map(ToString::to_string)
            });
            if let Some(thread_id) = thread_id {
                self.agent_activity.select_thread(&thread_id);
            }
        }
        Ok(())
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
    ) {
        self.exit_confirmation_pending = false;
        self.set_pointer_position(position);
        let key = match direction {
            MouseScrollDirection::Up => KeyCode::Up,
            MouseScrollDirection::Down => KeyCode::Down,
        };
        if self.diff_view.is_some() {
            match direction {
                MouseScrollDirection::Up => self.scroll_diff_view_up(),
                MouseScrollDirection::Down => self.scroll_diff_view_down(),
            }
            return;
        }
        if self.tool_output.is_some() {
            match direction {
                MouseScrollDirection::Up => self.scroll_tool_output_up(),
                MouseScrollDirection::Down => self.scroll_tool_output_down(),
            }
            return;
        }
        if self.agent_log.is_some() {
            match direction {
                MouseScrollDirection::Up => self.scroll_agent_log_up(),
                MouseScrollDirection::Down => self.scroll_agent_log_down(),
            }
            return;
        }
        if let Some(selector) = &mut self.selector {
            if selector.option_at(area, position).is_some() {
                selector.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
            }
            return;
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
            return;
        }
        if self.has_blocking_overlay() {
            return;
        }

        match (ShellView { shell: self }).pointer_pane_at(area, position) {
            Some(PointerPane::Transcript) => match direction {
                MouseScrollDirection::Up => self.scroll_transcript_up(TRANSCRIPT_WHEEL_SCROLL_STEP),
                MouseScrollDirection::Down => {
                    self.scroll_transcript_down(TRANSCRIPT_WHEEL_SCROLL_STEP)
                }
            },
            Some(PointerPane::Dashboard) => self.scroll_dashboard_at(area, position, direction),
            Some(PointerPane::Header | PointerPane::Input) | None => {}
        }
    }

    fn has_blocking_overlay(&self) -> bool {
        self.diff_view.is_some()
            || self.tool_output.is_some()
            || self.agent_log.is_some()
            || self.pending_approval.is_some()
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
    ) {
        let nested_panel = match self.dashboard_route {
            DashboardRoute::Sessions => Some("Sessions"),
            DashboardRoute::Agents => Some("Agents"),
            DashboardRoute::Status => Some("Settings"),
            DashboardRoute::Help => None,
        };
        let over_nested_panel = nested_panel.is_some_and(|title| {
            (ShellView { shell: self })
                .dashboard_panel_position_at(area, position, title)
                .is_some()
        });
        if over_nested_panel {
            match (self.dashboard_route, direction) {
                (DashboardRoute::Sessions, MouseScrollDirection::Up)
                    if !self.session_list.renaming() =>
                {
                    self.session_list.move_selection_up()
                }
                (DashboardRoute::Sessions, MouseScrollDirection::Down)
                    if !self.session_list.renaming() =>
                {
                    self.session_list.move_selection_down()
                }
                (DashboardRoute::Agents, MouseScrollDirection::Up) => {
                    self.agent_activity.move_selection_up()
                }
                (DashboardRoute::Agents, MouseScrollDirection::Down) => {
                    self.agent_activity.move_selection_down()
                }
                (DashboardRoute::Status, MouseScrollDirection::Up) if !self.settings.editing() => {
                    self.settings.move_up()
                }
                (DashboardRoute::Status, MouseScrollDirection::Down)
                    if !self.settings.editing() =>
                {
                    self.settings.move_down()
                }
                (DashboardRoute::Sessions | DashboardRoute::Status, _)
                | (DashboardRoute::Help, _) => {}
            }
            return;
        }

        let max_scroll = (ShellView { shell: self }).dashboard_scroll_max(area);
        let scroll = self.dashboard_scroll.get().min(max_scroll);
        self.dashboard_scroll.set(match direction {
            MouseScrollDirection::Up => scroll.saturating_sub(DASHBOARD_WHEEL_SCROLL_STEP),
            MouseScrollDirection::Down => scroll
                .saturating_add(DASHBOARD_WHEEL_SCROLL_STEP)
                .min(max_scroll),
        });
    }
}
