use super::ShellState;
use super::TranscriptKind;
use super::backend::AppShellBackend;
use crate::legacy_core::config::Config;
use crate::temporary_structured_request::TemporaryStructuredThreadOptions;
use crate::temporary_structured_request::run_temporary_structured_turn;
use crate::temporary_structured_request::start_temporary_thread;
use crate::temporary_structured_request::unsubscribe_temporary_thread;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadSource;
use codex_context_fragments::ContextualUserFragment;
use codex_context_fragments::RecapPrompt;
use codex_protocol::ThreadId;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[path = "recap_history.rs"]
mod history;

#[derive(Default)]
pub(super) struct RecapState(Option<RecapRequest>);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RecapTrigger {
    Manual,
    Automatic,
}

struct RecapRequest {
    trigger: RecapTrigger,
    turn_revision: usize,
    owner: ThreadId,
    revision: u64,
    prompt: String,
    client: AppServerRequestHandle,
    cancellation: CancellationToken,
    stage: Option<RecapStage>,
}

enum RecapStage {
    Starting(JoinHandle<Result<String>>),
    Running {
        thread_id: String,
        notifications: mpsc::UnboundedSender<ServerNotification>,
        task: JoinHandle<Result<String>>,
    },
}

impl Drop for RecapRequest {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(RecapStage::Starting(task)) = self.stage.take() {
            let client = self.client.clone();
            tokio::spawn(async move {
                if let Ok(Ok(thread_id)) = task.await {
                    unsubscribe_temporary_thread(&client, thread_id).await;
                }
            });
        }
    }
}

impl RecapState {
    pub(super) fn cancel_automatic(&mut self) {
        if self
            .0
            .as_ref()
            .is_some_and(|request| request.trigger == RecapTrigger::Automatic)
        {
            self.0 = None;
        }
    }

    pub(super) fn has_work(&self) -> bool {
        self.0.is_some()
    }

    pub(super) fn owns_thread(&self, id: &str) -> bool {
        self.0.as_ref().is_some_and(|request| matches!(request.stage.as_ref(), Some(RecapStage::Running { thread_id, .. }) if thread_id == id))
    }

    pub(super) fn forward(&self, notification: &ServerNotification) -> bool {
        let Some(RecapRequest {
            stage:
                Some(RecapStage::Running {
                    thread_id,
                    notifications,
                    ..
                }),
            ..
        }) = self.0.as_ref()
        else {
            return false;
        };
        let target = match notification {
            ServerNotification::ItemCompleted(item) => &item.thread_id,
            ServerNotification::TurnCompleted(turn) => &turn.thread_id,
            _ => return false,
        };
        if target != thread_id {
            return false;
        }
        let _ = notifications.send(notification.clone());
        true
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct GeneratedRecap {
    summary: String,
    #[serde(deserialize_with = "Option::deserialize")]
    next_action: Option<String>,
}

impl GeneratedRecap {
    fn into_message(self) -> String {
        let mut text = format!("Recap\n\n{}", self.summary);
        if let Some(next) = self.next_action {
            text.push_str(&format!("\n\nNext: {next}"));
        }
        text
    }
}

fn parse_recap(text: &str) -> Result<GeneratedRecap> {
    let mut recap: GeneratedRecap = serde_json::from_str(text)?;
    recap.summary = recap.summary.trim().to_string();
    recap.next_action = recap
        .next_action
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    if recap.summary.is_empty()
        || recap.summary.chars().count() > 700
        || recap
            .next_action
            .as_ref()
            .is_some_and(|text| text.chars().count() > 200)
    {
        return Err(eyre!("The generated recap exceeded its size limit"));
    }
    Ok(recap)
}

impl ShellState {
    fn recap_revision(&self) -> u64 {
        self.transcript
            .iter()
            .filter(|line| matches!(line.kind, TranscriptKind::User | TranscriptKind::Assistant))
            .map(|line| line.render_revision)
            .max()
            .unwrap_or_default()
    }

    pub(super) fn run_recap_command<S: AppShellBackend>(
        &mut self,
        args: &str,
        config: &Config,
        app_server: &S,
    ) -> Result<()> {
        if args == "cancel" {
            self.recap = RecapState::default();
            self.push_status("Recap cancelled");
            return Ok(());
        }
        if matches!(args, "on" | "off") {
            let thread_id = self.thread_id;
            let request = app_server.workspace_request_in_background(
                thread_id,
                super::workspace_requests::WorkspaceRequest::AutomaticRecap(args == "on"),
            );
            self.start_backend_action(
                super::backend_actions::ActionGroup::Workspace,
                "saving recap preference",
                async move {
                    super::backend_actions::BackendActionResult::Workspace {
                        thread_id,
                        result: request.await,
                    }
                },
            );
            return Ok(());
        }
        if !args.is_empty() {
            self.push_error("Usage: /recap [cancel|on|off]");
            return Ok(());
        }
        self.start_recap(config, app_server, RecapTrigger::Manual)
    }

    pub(super) fn start_recap<S: AppShellBackend>(
        &mut self,
        config: &Config,
        app_server: &S,
        trigger: RecapTrigger,
    ) -> Result<()> {
        if self.recap.has_work() {
            self.push_status("A recap is already being generated");
            return Ok(());
        }
        if self.active_turn_id.is_some()
            || self.has_pending_backend_action(super::backend_actions::ActionGroup::TurnStart)
        {
            self.push_status("Wait for the current turn to finish before generating a recap");
            return Ok(());
        }
        let history = history::recap_history(&self.transcript);
        if history.is_empty() {
            if trigger == RecapTrigger::Manual {
                self.push_status("There is no conversation history to recap");
            }
            return Ok(());
        }
        let client = app_server
            .app_server_request_handle()
            .ok_or_else(|| eyre!("Recaps are unavailable for this connection"))?;
        let options = TemporaryStructuredThreadOptions {
            thread_source: ThreadSource::Feature("system".to_string()),
            model: self.model.clone(),
            model_provider: self.model_provider_id.clone(),
            cwd: self.cwd.clone(),
            active_permission_profile: self
                .active_permission_profile
                .as_ref()
                .map(|profile| profile.id.clone()),
            mcp_server_names: config.mcp_servers.get().keys().cloned().collect(),
        };
        let cancellation = CancellationToken::new();
        let cancelled = cancellation.clone();
        let worker_client = client.clone();
        let stage = RecapStage::Starting(tokio::spawn(async move {
            let response = start_temporary_thread(&worker_client, options).await?;
            let id = response.thread.id;
            if cancelled.is_cancelled() {
                unsubscribe_temporary_thread(&worker_client, id).await;
                return Err(eyre!("Recap cancelled"));
            }
            Ok(id)
        }));
        self.recap = RecapState(Some(RecapRequest {
            trigger,
            turn_revision: self.automatic_recap.revision,
            owner: self.thread_id,
            revision: self.recap_revision(),
            prompt: RecapPrompt::new(&history).render(),
            client,
            cancellation,
            stage: Some(stage),
        }));
        if trigger == RecapTrigger::Manual {
            self.push_status("Generating recap");
        }
        Ok(())
    }

    pub(super) async fn poll_recap(&mut self) -> bool {
        let finished = self.recap.0.as_ref().is_some_and(|request| {
            request.stage.as_ref().is_some_and(|stage| match stage {
                RecapStage::Starting(task) | RecapStage::Running { task, .. } => task.is_finished(),
            })
        });
        if !finished {
            return false;
        }
        let Some(mut request) = self.recap.0.take() else {
            return false;
        };
        let Some(stage) = request.stage.take() else {
            return false;
        };
        let starting = matches!(stage, RecapStage::Starting(_));
        let result = match stage {
            RecapStage::Starting(task) | RecapStage::Running { task, .. } => task
                .await
                .map_err(Into::into)
                .and_then(std::convert::identity),
        };
        let fresh = request.turn_revision == self.automatic_recap.revision
            && request.owner == self.thread_id
            && request.revision == self.recap_revision()
            && self.active_turn_id.is_none()
            && !self.has_pending_backend_action(super::backend_actions::ActionGroup::TurnStart);
        match result {
            Ok(thread_id) if starting => {
                if !fresh {
                    let client = request.client.clone();
                    tokio::spawn(async move {
                        unsubscribe_temporary_thread(&client, thread_id).await;
                    });
                    return true;
                }
                let (notifications, receiver) = mpsc::unbounded_channel();
                let output_schema = json!({"type":"object", "properties":{"summary":{"type":"string","minLength":1,"maxLength":700},"next_action":{"type":["string","null"],"maxLength":200}}, "required":["summary","next_action"],"additionalProperties":false});
                let task = tokio::spawn(run_temporary_structured_turn(
                    request.client.clone(),
                    thread_id.clone(),
                    std::mem::take(&mut request.prompt),
                    output_schema,
                    /*effort*/ None,
                    receiver,
                    request.cancellation.clone(),
                ));
                request.stage = Some(RecapStage::Running {
                    thread_id,
                    notifications,
                    task,
                });
                self.recap.0 = Some(request);
            }
            Ok(text) if fresh => match parse_recap(&text) {
                Ok(recap) => {
                    self.automatic_recap.mark_recapped();
                    self.push_system(recap.into_message());
                    self.push_status("Recap ready");
                }
                Err(error) => {
                    if request.trigger == RecapTrigger::Automatic {
                        self.automatic_recap.mark_failed(std::time::Instant::now());
                    } else {
                        self.report_action_error("Could not generate a recap", error);
                    }
                }
            },
            Ok(_) if request.trigger == RecapTrigger::Manual => {
                self.push_status("Conversation changed; request a new recap")
            }
            Ok(_) => {}
            Err(error) if fresh => {
                if request.trigger == RecapTrigger::Automatic {
                    self.automatic_recap.mark_failed(std::time::Instant::now());
                } else {
                    self.report_action_error("Could not generate a recap", error);
                }
            }
            Err(_) => {}
        }
        true
    }
}

#[cfg(test)]
#[path = "recap_tests.rs"]
mod tests;
