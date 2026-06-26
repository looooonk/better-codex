use super::ShellState;
use crate::app_server_session::AppServerSession;
use crate::token_usage::TokenUsage;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::TurnStatus;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;

const UNSUPPORTED_REQUEST_ERROR: i64 = -32000;

impl ShellState {
    pub(super) async fn handle_app_server_event(
        &mut self,
        app_server: &mut AppServerSession,
        event: AppServerEvent,
    ) -> Result<()> {
        match event {
            AppServerEvent::Lagged { skipped } => {
                self.push_system(format!("skipped {skipped} best-effort backend events"));
            }
            AppServerEvent::ServerNotification(notification) => {
                self.handle_notification(notification);
            }
            AppServerEvent::ServerRequest(request) => {
                self.reject_unsupported_request(app_server, request).await?;
            }
            AppServerEvent::Disconnected { message } => {
                self.status = "disconnected".to_string();
                self.push_error(message);
            }
        }
        Ok(())
    }

    fn handle_notification(&mut self, notification: ServerNotification) {
        match notification {
            ServerNotification::AgentMessageDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string() {
                    self.streaming_assistant.push_str(&delta.delta);
                }
            }
            ServerNotification::PlanDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string() {
                    self.push_status(format!("plan: {}", delta.delta.trim()));
                }
            }
            ServerNotification::ReasoningSummaryTextDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string() {
                    self.status = "reasoning".to_string();
                }
            }
            ServerNotification::ReasoningTextDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string() {
                    self.status = "reasoning".to_string();
                }
            }
            ServerNotification::TurnStarted(started) => {
                if started.thread_id == self.thread_id.to_string() {
                    self.active_turn_id = Some(started.turn.id);
                    self.status = "thinking".to_string();
                }
            }
            ServerNotification::TurnCompleted(completed) => {
                if completed.thread_id == self.thread_id.to_string() {
                    self.finish_streaming_assistant();
                    self.active_turn_id = None;
                    self.status = match completed.turn.status {
                        TurnStatus::Completed => "ready".to_string(),
                        TurnStatus::Failed => "failed".to_string(),
                        TurnStatus::Interrupted => "interrupted".to_string(),
                        TurnStatus::InProgress => "thinking".to_string(),
                    };
                }
            }
            ServerNotification::ThreadTokenUsageUpdated(usage) => {
                if usage.thread_id == self.thread_id.to_string() {
                    self.apply_token_usage(usage.token_usage);
                }
            }
            ServerNotification::ThreadNameUpdated(updated) => {
                if updated.thread_id == self.thread_id.to_string() {
                    self.thread_name = updated.thread_name;
                }
            }
            ServerNotification::ThreadSettingsUpdated(updated) => {
                if updated.thread_id == self.thread_id.to_string() {
                    self.model = updated.thread_settings.model;
                    self.cwd = updated.thread_settings.cwd.to_string_lossy().to_string();
                    self.approval_policy = updated.thread_settings.approval_policy;
                    self.approvals_reviewer =
                        approvals_reviewer_from_api(updated.thread_settings.approvals_reviewer);
                    self.reasoning_effort = updated.thread_settings.effort;
                    self.service_tier = updated.thread_settings.service_tier;
                    self.collaboration_mode =
                        Some(Box::new(updated.thread_settings.collaboration_mode));
                    self.personality = updated.thread_settings.personality;
                }
            }
            ServerNotification::Error(error) => {
                if error.thread_id == self.thread_id.to_string() {
                    self.status = if error.will_retry {
                        "retrying".to_string()
                    } else {
                        "error".to_string()
                    };
                    self.push_error(error.error.message);
                }
            }
            ServerNotification::Warning(warning) => {
                if warning
                    .thread_id
                    .as_deref()
                    .is_none_or(|thread_id| thread_id == self.thread_id.to_string())
                {
                    self.push_status(warning.message);
                }
            }
            ServerNotification::GuardianWarning(warning) => {
                if warning.thread_id == self.thread_id.to_string() {
                    self.push_status(warning.message);
                }
            }
            ServerNotification::ConfigWarning(warning) => {
                self.push_status(warning.summary);
            }
            ServerNotification::ModelRerouted(rerouted) => {
                if rerouted.thread_id == self.thread_id.to_string() {
                    self.model = rerouted.to_model;
                    self.push_status("model rerouted");
                }
            }
            ServerNotification::ModelVerification(verification) => {
                if verification.thread_id == self.thread_id.to_string() {
                    self.push_status("model verification updated");
                }
            }
            ServerNotification::ItemCompleted(_)
            | ServerNotification::ItemStarted(_)
            | ServerNotification::CommandExecOutputDelta(_)
            | ServerNotification::ProcessOutputDelta(_)
            | ServerNotification::ProcessExited(_)
            | ServerNotification::CommandExecutionOutputDelta(_)
            | ServerNotification::FileChangeOutputDelta(_)
            | ServerNotification::FileChangePatchUpdated(_)
            | ServerNotification::McpToolCallProgress(_)
            | ServerNotification::ServerRequestResolved(_)
            | ServerNotification::TurnDiffUpdated(_)
            | ServerNotification::TurnPlanUpdated(_)
            | ServerNotification::HookStarted(_)
            | ServerNotification::HookCompleted(_)
            | ServerNotification::ThreadStarted(_)
            | ServerNotification::ThreadStatusChanged(_)
            | ServerNotification::ThreadArchived(_)
            | ServerNotification::ThreadDeleted(_)
            | ServerNotification::ThreadUnarchived(_)
            | ServerNotification::ThreadClosed(_)
            | ServerNotification::SkillsChanged(_)
            | ServerNotification::ThreadGoalUpdated(_)
            | ServerNotification::ThreadGoalCleared(_)
            | ServerNotification::ItemGuardianApprovalReviewStarted(_)
            | ServerNotification::ItemGuardianApprovalReviewCompleted(_)
            | ServerNotification::RawResponseItemCompleted(_)
            | ServerNotification::TerminalInteraction(_)
            | ServerNotification::McpServerOauthLoginCompleted(_)
            | ServerNotification::McpServerStatusUpdated(_)
            | ServerNotification::AccountUpdated(_)
            | ServerNotification::AccountRateLimitsUpdated(_)
            | ServerNotification::AppListUpdated(_)
            | ServerNotification::RemoteControlStatusChanged(_)
            | ServerNotification::ExternalAgentConfigImportProgress(_)
            | ServerNotification::ExternalAgentConfigImportCompleted(_)
            | ServerNotification::FsChanged(_)
            | ServerNotification::ReasoningSummaryPartAdded(_)
            | ServerNotification::ContextCompacted(_)
            | ServerNotification::TurnModerationMetadata(_)
            | ServerNotification::ModelSafetyBufferingUpdated(_)
            | ServerNotification::DeprecationNotice(_)
            | ServerNotification::FuzzyFileSearchSessionUpdated(_)
            | ServerNotification::FuzzyFileSearchSessionCompleted(_)
            | ServerNotification::ThreadRealtimeStarted(_)
            | ServerNotification::ThreadRealtimeItemAdded(_)
            | ServerNotification::ThreadRealtimeTranscriptDelta(_)
            | ServerNotification::ThreadRealtimeTranscriptDone(_)
            | ServerNotification::ThreadRealtimeOutputAudioDelta(_)
            | ServerNotification::ThreadRealtimeSdp(_)
            | ServerNotification::ThreadRealtimeError(_)
            | ServerNotification::ThreadRealtimeClosed(_)
            | ServerNotification::WindowsWorldWritableWarning(_)
            | ServerNotification::WindowsSandboxSetupCompleted(_)
            | ServerNotification::AccountLoginCompleted(_) => {}
        }
    }

    async fn reject_unsupported_request(
        &mut self,
        app_server: &mut AppServerSession,
        request: ServerRequest,
    ) -> Result<()> {
        let request_id = request.id().clone();
        self.push_error(format!(
            "unsupported interactive backend request: {request:?}"
        ));
        app_server
            .reject_server_request(
                request_id,
                JSONRPCErrorError {
                    code: UNSUPPORTED_REQUEST_ERROR,
                    data: None,
                    message:
                        "the first-stage app shell does not implement this interactive request yet"
                            .to_string(),
                },
            )
            .await
            .wrap_err("failed to reject unsupported app-server request")
    }

    fn apply_token_usage(&mut self, usage: ThreadTokenUsage) {
        self.token_usage = TokenUsage {
            input_tokens: usage.total.input_tokens,
            cached_input_tokens: usage.total.cached_input_tokens,
            output_tokens: usage.total.output_tokens,
            reasoning_output_tokens: usage.total.reasoning_output_tokens,
            total_tokens: usage.total.total_tokens,
        };
        self.model_context_window = usage.model_context_window;
    }
}

fn approvals_reviewer_from_api(
    reviewer: codex_app_server_protocol::ApprovalsReviewer,
) -> codex_protocol::config_types::ApprovalsReviewer {
    match reviewer {
        codex_app_server_protocol::ApprovalsReviewer::User => {
            codex_protocol::config_types::ApprovalsReviewer::User
        }
        codex_app_server_protocol::ApprovalsReviewer::AutoReview => {
            codex_protocol::config_types::ApprovalsReviewer::AutoReview
        }
    }
}
