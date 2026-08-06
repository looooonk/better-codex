use super::ShellState;
use super::shell_layout;
use super::shell_layout::DashboardWidthChange;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::layout::Position;
use ratatui::layout::Rect;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct DashboardResizeState {
    pub(super) preferred_width: Option<u16>,
    pub(super) dragging: bool,
}

impl ShellState {
    pub(super) fn begin_dashboard_resize(&mut self, area: Rect, position: Position) -> bool {
        if self.dashboard_resize_blocked()
            || !shell_layout::dashboard_divider_contains(self, area, position)
        {
            return false;
        }

        self.dashboard_resize.dragging = true;
        self.set_pointer_position(position);
        true
    }

    pub(super) fn update_dashboard_resize(&mut self, area: Rect, position: Position) -> bool {
        if !self.dashboard_resize.dragging {
            return false;
        }

        self.set_pointer_position(position);
        if let Some(width) = shell_layout::dashboard_width_from_divider(area, position.x) {
            self.dashboard_resize.preferred_width = Some(width);
        }
        true
    }

    pub(super) fn finish_dashboard_resize(&mut self, area: Rect, position: Position) -> bool {
        if !self.dashboard_resize.dragging {
            return false;
        }

        self.update_dashboard_resize(area, position);
        self.dashboard_resize.dragging = false;
        true
    }

    pub(super) fn cancel_dashboard_resize(&mut self) {
        self.dashboard_resize.dragging = false;
    }

    pub(super) fn handle_dashboard_resize_key(&mut self, area: Rect, key: KeyEvent) -> bool {
        if !self.dashboard_focused()
            || self.dashboard_resize_blocked()
            || !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            || key.modifiers != (KeyModifiers::SHIFT | KeyModifiers::ALT)
        {
            return false;
        }

        let change = match key.code {
            KeyCode::Left => DashboardWidthChange::Wider,
            KeyCode::Right => DashboardWidthChange::Narrower,
            _ => return false,
        };
        let Some(width) = shell_layout::calculate(self, area)
            .and_then(|layout| layout.dashboard)
            .map(|dashboard| dashboard.area().width)
        else {
            return false;
        };
        self.dashboard_resize.preferred_width = Some(shell_layout::adjust_dashboard_width(
            area.width, width, change,
        ));
        true
    }

    fn dashboard_resize_blocked(&self) -> bool {
        !self.dashboard_visible
            || self.rewind.is_active()
            || self.diff_view.is_some()
            || self.tool_output.is_some()
            || self.agent_log.is_some()
            || self.selector.is_some()
            || self.command_palette.is_some()
            || self.pending_account_auth.is_some()
            || self.pending_approval.is_some()
            || self.pending_session_delete.is_some()
            || self.pending_elicitation.is_some()
            || self.pending_external_agent_import.is_some()
            || self.pending_mcp_management.is_some()
            || self.pending_plugin_management.is_some()
            || self.pending_user_input.is_some()
            || self.safety_buffering_modal_lines().is_some()
    }
}

#[cfg(test)]
#[path = "dashboard_resize_tests.rs"]
mod tests;
