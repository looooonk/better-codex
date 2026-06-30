use super::ShellState;
use super::backend::AppShellBackend;
use crate::token_usage::TokenUsage;
use crate::workspace_command::WorkspaceCommandExecutor;
use base64::Engine;
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
    pub(super) async fn handle_app_server_event<S>(
        &mut self,
        app_server: &mut S,
        workspace_command_runner: &dyn WorkspaceCommandExecutor,
        event: AppServerEvent,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        match event {
            AppServerEvent::Lagged { skipped } => {
                self.push_system(format!("skipped {skipped} best-effort backend events"));
            }
            AppServerEvent::ServerNotification(notification) => {
                if let ServerNotification::ExternalAgentConfigImportCompleted(notification) =
                    &notification
                    && app_server.consume_external_agent_config_import_completion()
                {
                    self.report_external_agent_import_finished(notification);
                    return Ok(());
                }
                self.handle_notification(notification);
            }
            AppServerEvent::ServerRequest(request) => {
                self.handle_server_request(app_server, request).await?;
            }
            AppServerEvent::Disconnected { message } => {
                self.status = "disconnected".to_string();
                self.push_error(message);
            }
        }
        if self.workspace_status_refresh_due {
            self.refresh_workspace_status(workspace_command_runner)
                .await;
        }
        Ok(())
    }

    pub(super) fn handle_notification(&mut self, notification: ServerNotification) {
        match notification {
            ServerNotification::AgentMessageDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string() {
                    self.streaming_assistant.push_str(&delta.delta);
                }
            }
            ServerNotification::PlanDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string() {
                    self.streaming_plan.push_str(&delta.delta);
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
                    self.finish_streaming_plan();
                    self.finish_streaming_assistant();
                    self.active_turn_id = None;
                    self.workspace_status_refresh_due = true;
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
                    self.workspace_status_refresh_due = true;
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
            ServerNotification::TurnDiffUpdated(updated) => {
                if updated.thread_id == self.thread_id.to_string() {
                    self.latest_diff = Some(super::diff_summary_from_unified_diff(&updated.diff));
                    self.workspace_status_refresh_due = true;
                    if let Some(summary) = &self.latest_diff {
                        self.push_diff_with_status(
                            format!(
                                "diff {} files +{} -{}",
                                summary.files, summary.additions, summary.removals
                            ),
                            super::ToolBlockStatus::Running,
                        );
                    }
                }
            }
            ServerNotification::TurnPlanUpdated(updated) => {
                if updated.thread_id == self.thread_id.to_string() {
                    self.plan_explanation = updated.explanation;
                    self.plan_steps = updated.plan;
                }
            }
            ServerNotification::ThreadGoalUpdated(updated) => {
                if updated.thread_id == self.thread_id.to_string() {
                    self.active_goal = Some(updated.goal);
                }
            }
            ServerNotification::ThreadGoalCleared(cleared) => {
                if cleared.thread_id == self.thread_id.to_string() {
                    self.active_goal = None;
                }
            }
            ServerNotification::ItemStarted(started) => {
                if started.thread_id == self.thread_id.to_string()
                    && let Some(title) = item_activity_title(&started.item)
                {
                    let item_id = started.item.id().to_string();
                    self.record_item_activity(&started.item, title.clone(), "in progress");
                    self.push_tool_with_status_for_item(
                        item_id,
                        title,
                        super::ToolBlockStatus::Running,
                    );
                }
            }
            ServerNotification::ItemCompleted(completed) => {
                if completed.thread_id == self.thread_id.to_string() {
                    self.ingest_completed_item(completed.item);
                }
            }
            ServerNotification::CommandExecutionOutputDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string()
                    && let Some(output) = super::compact_multiline(delta.delta)
                {
                    self.push_output_with_status(output, super::ToolBlockStatus::Running);
                }
            }
            ServerNotification::FileChangePatchUpdated(updated) => {
                if updated.thread_id == self.thread_id.to_string() {
                    self.latest_diff = Some(super::diff_summary_from_changes(&updated.changes));
                    self.workspace_status_refresh_due = true;
                    let summary = super::file_change_summary(&updated.changes);
                    self.upsert_tool_activity(
                        updated.item_id.clone(),
                        summary,
                        "in progress".to_string(),
                    );
                    self.push_diff_with_status_for_item(
                        updated.item_id,
                        super::file_change_detail(&updated.changes),
                        super::ToolBlockStatus::Running,
                    );
                }
            }
            ServerNotification::McpToolCallProgress(progress) => {
                if progress.thread_id == self.thread_id.to_string() {
                    let title = format!("mcp progress: {}", progress.message);
                    let transcript = super::compact_multiline(title.clone());
                    self.upsert_tool_activity(
                        progress.item_id.clone(),
                        title,
                        "in progress".to_string(),
                    );
                    if let Some(transcript) = transcript {
                        self.push_tool_with_status_for_item(
                            progress.item_id,
                            transcript,
                            super::ToolBlockStatus::Running,
                        );
                    }
                }
            }
            ServerNotification::ServerRequestResolved(resolved) => {
                if resolved.thread_id == self.thread_id.to_string() {
                    self.push_status(format!("request resolved: {}", resolved.request_id));
                }
            }
            ServerNotification::CommandExecOutputDelta(delta) => {
                let output = base64::engine::general_purpose::STANDARD
                    .decode(delta.delta_base64)
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .and_then(super::compact_multiline);
                if let Some(output) = output {
                    self.push_output_with_status(output, super::ToolBlockStatus::Running);
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
            ServerNotification::AccountRateLimitsUpdated(updated) => {
                self.apply_rate_limit_update(updated.rate_limits);
            }
            ServerNotification::ProcessOutputDelta(_)
            | ServerNotification::ProcessExited(_)
            | ServerNotification::FileChangeOutputDelta(_)
            | ServerNotification::HookStarted(_)
            | ServerNotification::HookCompleted(_)
            | ServerNotification::ThreadStarted(_)
            | ServerNotification::ThreadStatusChanged(_)
            | ServerNotification::ThreadArchived(_)
            | ServerNotification::ThreadDeleted(_)
            | ServerNotification::ThreadUnarchived(_)
            | ServerNotification::ThreadClosed(_)
            | ServerNotification::SkillsChanged(_)
            | ServerNotification::ItemGuardianApprovalReviewStarted(_)
            | ServerNotification::ItemGuardianApprovalReviewCompleted(_)
            | ServerNotification::RawResponseItemCompleted(_)
            | ServerNotification::TerminalInteraction(_)
            | ServerNotification::McpServerOauthLoginCompleted(_)
            | ServerNotification::McpServerStatusUpdated(_)
            | ServerNotification::AccountUpdated(_)
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

    async fn handle_server_request<S>(
        &mut self,
        app_server: &mut S,
        request: ServerRequest,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        match super::PendingApproval::from_request(&request) {
            Ok(Some(pending)) => {
                let title = pending.title().to_string();
                if self.has_pending_interactive_request() {
                    self.reject_request_with_message(
                        app_server,
                        request.id().clone(),
                        format!("approval already pending: {title}"),
                    )
                    .await?;
                    return Ok(());
                }
                self.pending_approval = Some(pending);
                self.push_status(format!("approval requested: {title}"));
                Ok(())
            }
            Ok(None) => {
                if let Some(pending) = super::PendingElicitation::from_request(&request) {
                    let title = pending.title().to_string();
                    if self.has_pending_interactive_request() {
                        self.reject_request_with_message(
                            app_server,
                            request.id().clone(),
                            format!("interactive request already pending: {title}"),
                        )
                        .await?;
                        return Ok(());
                    }
                    self.pending_elicitation = Some(pending);
                    self.push_status(format!("elicitation requested: {title}"));
                    Ok(())
                } else if let Some(pending) = super::PendingUserInput::from_request(&request) {
                    let title = pending.title().to_string();
                    if self.has_pending_interactive_request() {
                        self.reject_request_with_message(
                            app_server,
                            request.id().clone(),
                            format!("interactive request already pending: {title}"),
                        )
                        .await?;
                        return Ok(());
                    }
                    self.pending_user_input = Some(pending);
                    self.push_status(format!("input requested: {title}"));
                    Ok(())
                } else {
                    self.reject_unsupported_request(app_server, request).await
                }
            }
            Err(message) => {
                self.reject_request_with_message(app_server, request.id().clone(), message)
                    .await
            }
        }
    }

    async fn reject_unsupported_request<S>(
        &mut self,
        app_server: &mut S,
        request: ServerRequest,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let request_id = request.id().clone();
        let message = format!(
            "unsupported interactive request: {}",
            request_name(&request)
        );
        self.reject_request_with_message(app_server, request_id, message)
            .await
    }

    async fn reject_request_with_message<S>(
        &mut self,
        app_server: &mut S,
        request_id: codex_app_server_protocol::RequestId,
        message: String,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        self.push_error(message.clone());
        app_server
            .reject_server_request(
                request_id,
                JSONRPCErrorError {
                    code: UNSUPPORTED_REQUEST_ERROR,
                    data: None,
                    message,
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

    fn has_pending_interactive_request(&self) -> bool {
        self.pending_approval.is_some()
            || self.pending_elicitation.is_some()
            || self.pending_user_input.is_some()
    }
}

pub(super) fn item_activity_title(item: &codex_app_server_protocol::ThreadItem) -> Option<String> {
    match item {
        codex_app_server_protocol::ThreadItem::UserMessage { .. }
        | codex_app_server_protocol::ThreadItem::HookPrompt { .. }
        | codex_app_server_protocol::ThreadItem::AgentMessage { .. }
        | codex_app_server_protocol::ThreadItem::Plan { .. }
        | codex_app_server_protocol::ThreadItem::Reasoning { .. }
        | codex_app_server_protocol::ThreadItem::EnteredReviewMode { .. }
        | codex_app_server_protocol::ThreadItem::ExitedReviewMode { .. }
        | codex_app_server_protocol::ThreadItem::ContextCompaction { .. } => None,
        codex_app_server_protocol::ThreadItem::CommandExecution { command, .. } => {
            Some(format!("exec {command}"))
        }
        codex_app_server_protocol::ThreadItem::FileChange { changes, .. } => {
            Some(super::file_change_summary(changes))
        }
        codex_app_server_protocol::ThreadItem::McpToolCall { server, tool, .. } => {
            Some(format!("mcp {server}/{tool}"))
        }
        codex_app_server_protocol::ThreadItem::DynamicToolCall {
            namespace, tool, ..
        } => Some(
            namespace
                .as_ref()
                .map(|namespace| format!("tool {namespace}/{tool}"))
                .unwrap_or_else(|| format!("tool {tool}")),
        ),
        codex_app_server_protocol::ThreadItem::CollabAgentToolCall { tool, .. } => {
            Some(format!("agent {tool:?}"))
        }
        codex_app_server_protocol::ThreadItem::SubAgentActivity {
            kind, agent_path, ..
        } => Some(format!("subagent {kind:?}: {agent_path}")),
        codex_app_server_protocol::ThreadItem::WebSearch { query, .. } => {
            Some(format!("web search: {query}"))
        }
        codex_app_server_protocol::ThreadItem::ImageView { path, .. } => {
            Some(format!("view image: {path}"))
        }
        codex_app_server_protocol::ThreadItem::Sleep { duration_ms, .. } => {
            Some(format!("sleep {duration_ms}ms"))
        }
        codex_app_server_protocol::ThreadItem::ImageGeneration { .. } => {
            Some("image generation".to_string())
        }
    }
}

fn request_name(request: &ServerRequest) -> &'static str {
    match request {
        ServerRequest::ExecCommandApproval { .. } => "command approval",
        ServerRequest::CommandExecutionRequestApproval { .. } => "command execution approval",
        ServerRequest::FileChangeRequestApproval { .. } => "file change approval",
        ServerRequest::ApplyPatchApproval { .. } => "apply patch approval",
        ServerRequest::PermissionsRequestApproval { .. } => "permissions approval",
        ServerRequest::ToolRequestUserInput { .. } => "tool user input",
        ServerRequest::DynamicToolCall { .. } => "dynamic tool call",
        ServerRequest::McpServerElicitationRequest { .. } => "mcp elicitation",
        ServerRequest::ChatgptAuthTokensRefresh { .. } => "chatgpt auth refresh",
        ServerRequest::CurrentTimeRead { .. } => "current time read",
        ServerRequest::AttestationGenerate { .. } => "attestation generation",
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
