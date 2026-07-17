use super::ShellState;
use super::backend::AppShellBackend;

impl ShellState {
    pub(super) async fn submit_next_queued_message<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        if self.active_turn_id.is_some() {
            return;
        }
        let Some(message) = self.composer.prepare_next_queued_message() else {
            return;
        };
        let mut preserved_composer = self.composer.clone();
        match self.submit_prompt(app_server, message.clone()).await {
            Ok(()) => preserved_composer.confirm_next_queued_message(&message),
            Err(error) => self.push_error(format!("Queued message failed to send: {error}")),
        }
        self.composer = preserved_composer;
    }
}
