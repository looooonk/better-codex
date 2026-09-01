use super::ShellState;
use super::backend::AppShellBackend;
use codex_app_server_protocol::UserInput;

impl ShellState {
    pub(super) fn queue_current_message<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        let prompt = self.composer.submission_text();
        if prompt.trim().is_empty()
            || self.reject_oversized_input(prompt.len())
            || self.reject_unavailable_session_action()
        {
            return;
        }
        let request = app_server.thread_queue_add_in_background(
            self.thread_id,
            vec![UserInput::Text {
                text: prompt.clone(),
                text_elements: Vec::new(),
            }],
            format!("better-codex-queue-{}", uuid::Uuid::new_v4()),
        );
        let prompt_for_request = prompt.clone();
        if self.backend_actions.start(/*group*/ None, async move {
            super::backend_actions::BackendActionResult::QueueAdd {
                prompt: prompt_for_request,
                result: request.await,
            }
        }) {
            self.composer.clear();
            self.status = "queueing message".to_string();
        }
    }
}
