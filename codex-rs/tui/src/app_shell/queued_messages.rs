use super::ShellState;
use super::backend::AppShellBackend;

impl ShellState {
    pub(super) fn submit_next_queued_message<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        if self.active_turn_id.is_some() {
            return;
        }
        let Some(message) = self.composer.prepare_next_queued_message() else {
            return;
        };
        self.start_turn(
            app_server,
            message,
            super::backend_actions::TurnSubmission::Queued,
        );
    }
}
