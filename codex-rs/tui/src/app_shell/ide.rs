use super::ShellState;
use super::backend::AppShellBackend;
use super::backend::AppShellTurnStart;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use super::backend_actions::TurnSubmission;
use crate::ide_context::IdeContext;
use codex_app_server_protocol::UserInput;
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct IdeState {
    pub(super) enabled: bool,
    warned: bool,
}

enum FetchPurpose {
    Status,
    Submission,
}

async fn fetch_context(
    cwd: PathBuf,
    codex_home: PathBuf,
    purpose: FetchPurpose,
) -> Result<IdeContext, String> {
    tokio::task::spawn_blocking(move || {
        crate::ide_context::fetch_ide_context(&cwd, &codex_home).map_err(|error| match purpose {
            FetchPurpose::Status => error.user_facing_hint(),
            FetchPurpose::Submission => error.prompt_skip_hint(),
        })
    })
    .await
    .map_err(|error| format!("IDE context request failed: {error}"))?
}

impl ShellState {
    pub(super) fn run_ide_command(&mut self, args: &str) {
        match args.trim().to_ascii_lowercase().as_str() {
            "" => self.ide.enabled = !self.ide.enabled,
            "on" => self.ide.enabled = true,
            "off" => self.ide.enabled = false,
            "status" => {}
            _ => {
                self.push_error("usage: /ide [on|off|status]");
                return;
            }
        }
        self.ide.warned = false;
        self.backend_actions.invalidate([ActionGroup::IdeStatus]);
        if !self.ide.enabled {
            if self.status == "checking IDE connection" {
                self.status = "ready".to_string();
            }
            self.push_system("IDE context is off.");
            return;
        }
        let cwd = self.cwd.clone();
        let codex_home = self.codex_home.clone();
        self.start_backend_action(
            ActionGroup::IdeStatus,
            "checking IDE connection",
            async move {
                BackendActionResult::IdeStatus {
                    result: fetch_context(cwd.clone().into(), codex_home, FetchPurpose::Status)
                        .await,
                    cwd,
                }
            },
        );
    }

    pub(super) fn complete_ide_status(&mut self, cwd: String, result: Result<IdeContext, String>) {
        if cwd != self.cwd || !self.ide.enabled {
            return;
        }
        if self.status == "checking IDE connection" {
            self.status = if self.active_turn_id.is_some() {
                "thinking"
            } else {
                "ready"
            }
            .to_string();
        }
        match result {
            Ok(context) => {
                let detail = if crate::ide_context::has_prompt_context(&context) {
                    "Future messages will include your current IDE selection and open tabs."
                } else {
                    "Connected to your IDE."
                };
                self.push_system(format!("IDE context is on. {detail}"));
            }
            Err(error) => {
                self.ide.enabled = false;
                self.push_system(format!("IDE context could not be enabled. {error}"));
            }
        }
    }

    fn apply_ide_result(&mut self, result: Result<IdeContext, String>, items: &mut Vec<UserInput>) {
        if !self.ide.enabled {
            return;
        }
        match result {
            Ok(context) => {
                self.ide.warned = false;
                crate::ide_context::apply_ide_context_to_user_input(&context, items);
            }
            Err(error) if !self.ide.warned => {
                self.ide.warned = true;
                self.push_system(format!("IDE context was skipped for this message. {error}"));
            }
            Err(_) => {}
        }
    }

    pub(super) fn start_turn_with_ide<S: AppShellBackend>(
        &mut self,
        app_server: &S,
        params: AppShellTurnStart,
        prompt: String,
        submission: TurnSubmission,
    ) -> bool {
        if !self.ide.enabled {
            return self.start_prepared_turn(app_server, params, prompt, submission);
        }
        let cwd = self.cwd.clone().into();
        let codex_home = self.codex_home.clone();
        self.start_backend_action(ActionGroup::TurnStart, "reading IDE context", async move {
            BackendActionResult::IdeTurnPrepared {
                params,
                prompt,
                submission,
                result: fetch_context(cwd, codex_home, FetchPurpose::Submission).await,
            }
        })
    }

    pub(super) fn complete_ide_turn<S: AppShellBackend>(
        &mut self,
        app_server: &S,
        mut params: AppShellTurnStart,
        prompt: String,
        submission: TurnSubmission,
        result: Result<IdeContext, String>,
    ) {
        if params.thread_id != self.thread_id {
            return;
        }
        self.apply_ide_result(result, &mut params.items);
        self.start_prepared_turn(app_server, params, prompt, submission);
    }

    pub(super) fn prepare_ide_queue<S: AppShellBackend>(
        &mut self,
        app_server: &S,
        mut mutation: super::queued_messages::QueueMutation,
    ) {
        use super::queued_messages::QueueMutation;
        let capture = match &mut mutation {
            QueueMutation::Add { input, capture_ide, .. } | QueueMutation::Update { input, capture_ide, .. } => {
                std::mem::take(capture_ide) && self.ide.enabled && !input.iter().any(|item| matches!(item, UserInput::Text { text, .. } if text.starts_with("# Context from my IDE setup:\n")))
            }
            QueueMutation::Delete { .. } | QueueMutation::Reorder { .. } | QueueMutation::Start => false,
        };
        if !capture {
            self.dispatch_queue_mutation(app_server, mutation);
            return;
        }
        let thread_id = self.thread_id;
        let cwd = self.cwd.clone().into();
        let codex_home = self.codex_home.clone();
        self.start_backend_action(
            ActionGroup::QueueMutation,
            "reading IDE context",
            async move {
                BackendActionResult::IdeQueuePrepared {
                    thread_id,
                    mutation,
                    result: fetch_context(cwd, codex_home, FetchPurpose::Submission).await,
                }
            },
        );
    }

    pub(super) fn complete_ide_queue<S: AppShellBackend>(
        &mut self,
        app_server: &S,
        thread_id: codex_protocol::ThreadId,
        mut mutation: super::queued_messages::QueueMutation,
        result: Result<IdeContext, String>,
    ) {
        if thread_id != self.thread_id {
            return;
        }
        match &mut mutation {
            super::queued_messages::QueueMutation::Add { input, .. }
            | super::queued_messages::QueueMutation::Update { input, .. } => {
                self.apply_ide_result(result, input)
            }
            super::queued_messages::QueueMutation::Delete { .. }
            | super::queued_messages::QueueMutation::Reorder { .. }
            | super::queued_messages::QueueMutation::Start => {}
        }
        self.dispatch_queue_mutation(app_server, mutation);
    }

    pub(super) fn prepare_ide_steer(
        &mut self,
        params: super::backend::AppShellTurnSteer,
        prompt: String,
    ) -> bool {
        if !self.ide.enabled {
            return false;
        }
        let cwd = self.cwd.clone().into();
        let codex_home = self.codex_home.clone();
        if self.start_backend_action(ActionGroup::TurnSteer, "reading IDE context", async move {
            BackendActionResult::IdeSteerPrepared {
                params,
                prompt,
                result: fetch_context(cwd, codex_home, FetchPurpose::Submission).await,
            }
        }) {
            self.composer.clear();
        }
        true
    }

    pub(super) fn complete_ide_steer<S: AppShellBackend>(
        &mut self,
        app_server: &S,
        mut params: super::backend::AppShellTurnSteer,
        prompt: String,
        result: Result<IdeContext, String>,
    ) {
        if params.thread_id != self.thread_id {
            return;
        }
        if self.active_turn_id.as_deref() != Some(params.turn_id.as_str()) {
            self.composer.restore_failed_submission(&prompt);
            self.composer.restore_input_images(&params.items);
            self.push_status("the active turn changed; your message was restored");
            return;
        }
        self.apply_ide_result(result, &mut params.items);
        let request = app_server.turn_steer_in_background(params.clone());
        self.start_backend_action(
            ActionGroup::TurnSteer,
            "steering current turn",
            async move {
                BackendActionResult::IdeSteerSubmitted {
                    params,
                    prompt,
                    result: request.await,
                }
            },
        );
    }

    pub(super) fn complete_ide_steer_submission(
        &mut self,
        params: super::backend::AppShellTurnSteer,
        prompt: String,
        result: color_eyre::Result<codex_app_server_protocol::TurnSteerResponse>,
    ) {
        if params.thread_id != self.thread_id {
            return;
        }
        match result {
            Ok(_) => {
                self.scroll_transcript_to_bottom();
                self.push_user_with_client_id(
                    super::format_user_inputs(&params.items),
                    params.client_user_message_id,
                );
                self.composer.remember_submission(&prompt);
                self.status = "thinking".to_string();
            }
            Err(error) => {
                self.composer.restore_failed_submission(&prompt);
                self.composer.restore_input_images(&params.items);
                self.report_action_error("failed to steer active turn", error);
            }
        }
    }

    fn start_prepared_turn<S: AppShellBackend>(
        &mut self,
        app_server: &S,
        params: AppShellTurnStart,
        prompt: String,
        submission: TurnSubmission,
    ) -> bool {
        let request = app_server.turn_start_in_background(params.clone());
        self.start_backend_action(ActionGroup::TurnStart, "thinking", async move {
            BackendActionResult::TurnStart {
                params,
                prompt,
                submission,
                result: request.await,
            }
        })
    }
}

#[cfg(test)]
#[path = "ide_tests.rs"]
mod tests;
