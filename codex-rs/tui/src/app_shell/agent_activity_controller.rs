use super::ShellState;
use super::navigation::DashboardRoute;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;

const AGENT_PAGE_STEP: usize = 8;

impl ShellState {
    pub(in crate::app_shell) fn handle_agent_activity_key(&mut self, key: KeyEvent) -> bool {
        if !self.dashboard_visible
            || self.dashboard_route != DashboardRoute::Agents
            || !self.agents_focused
            || !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            || !matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.agents_focused = false;
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.agent_activity.move_selection_up();
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.agent_activity.move_selection_down();
                true
            }
            KeyCode::PageUp => {
                for _ in 0..AGENT_PAGE_STEP {
                    self.agent_activity.move_selection_up();
                }
                true
            }
            KeyCode::PageDown => {
                for _ in 0..AGENT_PAGE_STEP {
                    self.agent_activity.move_selection_down();
                }
                true
            }
            KeyCode::Home | KeyCode::Char('g') => {
                let first = self
                    .agent_activity
                    .ordered_agents()
                    .first()
                    .map(|agent| agent.thread_id.clone());
                if let Some(thread_id) = first {
                    self.agent_activity.select_thread(&thread_id);
                }
                true
            }
            KeyCode::End | KeyCode::Char('G') => {
                let last = self
                    .agent_activity
                    .ordered_agents()
                    .last()
                    .map(|agent| agent.thread_id.clone());
                if let Some(thread_id) = last {
                    self.agent_activity.select_thread(&thread_id);
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "agent_activity_controller_tests.rs"]
mod tests;
