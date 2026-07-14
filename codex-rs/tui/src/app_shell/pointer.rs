use super::ShellState;
use super::navigation::DashboardRoute;
use super::render::PointerPane;
use super::render::ShellView;
use crate::tui::MouseScrollDirection;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::layout::Position;
use ratatui::layout::Rect;

const TRANSCRIPT_WHEEL_SCROLL_STEP: usize = 3;

impl ShellState {
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
        self.set_pointer_position(position);
        let key = match direction {
            MouseScrollDirection::Up => KeyCode::Up,
            MouseScrollDirection::Down => KeyCode::Down,
        };
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
        self.pending_approval.is_some()
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
        let title = match self.dashboard_route {
            DashboardRoute::Sessions => "Sessions",
            DashboardRoute::Agents => "Agents",
            DashboardRoute::Settings => "Settings",
            DashboardRoute::Workspace | DashboardRoute::Help => return,
        };
        if (ShellView { shell: self })
            .dashboard_panel_position_at(area, position, title)
            .is_none()
        {
            return;
        }
        match (self.dashboard_route, direction) {
            (DashboardRoute::Sessions, MouseScrollDirection::Up) => {
                self.session_list.move_selection_up()
            }
            (DashboardRoute::Sessions, MouseScrollDirection::Down) => {
                self.session_list.move_selection_down()
            }
            (DashboardRoute::Agents, MouseScrollDirection::Up) => {
                self.agent_activity.move_selection_up()
            }
            (DashboardRoute::Agents, MouseScrollDirection::Down) => {
                self.agent_activity.move_selection_down()
            }
            (DashboardRoute::Settings, MouseScrollDirection::Up) => self.settings.move_up(),
            (DashboardRoute::Settings, MouseScrollDirection::Down) => self.settings.move_down(),
            (DashboardRoute::Workspace | DashboardRoute::Help, _) => {}
        }
    }
}
