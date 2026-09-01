use super::ShellState;
use super::agent_activity::AgentChildEvent;
use super::agent_activity::AgentItemPhase;
use super::backend::AppShellBackend;
use super::session_lifecycle::RemoteThreadLifecycle;
use crate::token_usage::TokenUsage;
use base64::Engine;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::CurrentTimeReadResponse;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::protocol::SubAgentSource;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;

const UNSUPPORTED_REQUEST_ERROR: i64 = -32000;

impl ShellState {
    pub(super) async fn handle_app_server_event<S>(
        &mut self,
        app_server: &mut S,
        event: AppServerEvent,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        self.drain_agent_history_updates();
        match event {
            AppServerEvent::Lagged { skipped } => {
                self.push_system(format!("skipped {skipped} best-effort backend events"));
                self.diff_store.mark_history_truncated();
                self.refresh_open_diff_view();
                self.mark_workspace_status_refresh_due();
                self.request_queue_hydration(app_server);
            }
            AppServerEvent::ServerNotification(notification) => {
                if let ServerNotification::ExternalAgentConfigImportCompleted(notification) =
                    &notification
                    && app_server.consume_external_agent_config_import_completion()
                {
                    self.report_external_agent_import_finished(notification);
                    return Ok(());
                }
                let refresh_session_list = matches!(
                    &notification,
                    ServerNotification::ThreadStarted(_)
                        | ServerNotification::ThreadReverted(_)
                        | ServerNotification::ThreadArchived(_)
                        | ServerNotification::ThreadDeleted(_)
                        | ServerNotification::ThreadUnarchived(_)
                );
                if refresh_session_list {
                    self.invalidate_session_list_refresh();
                }
                let refresh_queue = matches!(
                    &notification,
                    ServerNotification::ThreadQueueChanged(changed)
                        if changed.thread_id == self.thread_id.to_string()
                );
                self.handle_notification(notification);
                if refresh_session_list {
                    self.start_session_list_refresh(app_server);
                }
                if refresh_queue {
                    self.request_queue_hydration(app_server);
                }
            }
            AppServerEvent::ServerRequest(request) => {
                self.handle_server_request(app_server, request).await?;
            }
            AppServerEvent::Disconnected { message } => {
                self.status = "disconnected".to_string();
                self.push_error(message);
            }
        }
        if self.workspace_status_refresh_due && self.active_turn_id.is_none() {
            self.start_workspace_status_refresh();
        }
        self.maybe_start_goal_rate_limit_recovery(app_server);
        Ok(())
    }

    pub(super) fn handle_notification(&mut self, notification: ServerNotification) {
        match notification {
            ServerNotification::AgentMessageDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string() {
                    self.mark_retry_recovered(&delta.turn_id);
                    self.clear_safety_buffering_for_streaming(&delta.turn_id);
                    self.push_streaming_assistant_delta(&delta.item_id, &delta.delta);
                } else if self.prepare_active_agent_thread(&delta.thread_id) {
                    self.agent_activity.record_child_progress(
                        &delta.thread_id,
                        &delta.item_id,
                        AgentChildEvent::Message,
                        &delta.delta,
                    );
                    self.agent_activity.mark_live_thread(&delta.thread_id);
                }
            }
            ServerNotification::PlanDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string() {
                    self.mark_retry_recovered(&delta.turn_id);
                    self.clear_safety_buffering_for_streaming(&delta.turn_id);
                    self.push_streaming_plan_delta(&delta.item_id, &delta.delta);
                }
            }
            ServerNotification::ReasoningSummaryTextDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string() {
                    self.clear_safety_buffering_for_streaming(&delta.turn_id);
                    self.status = "reasoning".to_string();
                } else if self.prepare_active_agent_thread(&delta.thread_id) {
                    self.agent_activity.record_child_progress(
                        &delta.thread_id,
                        &delta.item_id,
                        AgentChildEvent::Reasoning,
                        &delta.delta,
                    );
                    self.agent_activity.mark_live_thread(&delta.thread_id);
                }
            }
            ServerNotification::ReasoningTextDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string() {
                    self.clear_safety_buffering_for_streaming(&delta.turn_id);
                    self.status = "reasoning".to_string();
                }
            }
            ServerNotification::TurnStarted(started) => {
                if started.thread_id == self.thread_id.to_string() {
                    self.record_active_turn_started(started.turn.id.clone());
                    self.reset_safety_buffering_for_turn_start(&started.turn.id);
                    self.status = "thinking".to_string();
                } else if self.prepare_active_agent_thread(&started.thread_id) {
                    self.agent_activity.record_child_turn(
                        &started.thread_id,
                        &started.turn.id,
                        &started.turn.status,
                    );
                    self.agent_activity.mark_live_thread(&started.thread_id);
                }
            }
            ServerNotification::TurnCompleted(completed) => {
                if completed.thread_id == self.thread_id.to_string() {
                    let completed_active_turn =
                        self.active_turn_id.as_deref() == Some(completed.turn.id.as_str());
                    let turn_ended = !matches!(&completed.turn.status, TurnStatus::InProgress);
                    self.finish_streaming_plan();
                    self.finish_streaming_assistant();
                    self.clear_safety_buffering_for_turn_completion(&completed.turn.id);
                    if completed_active_turn {
                        self.clear_active_turn();
                    }
                    self.mark_workspace_status_refresh_due();
                    self.status = match completed.turn.status {
                        TurnStatus::Completed => "ready".to_string(),
                        TurnStatus::Failed => "failed".to_string(),
                        TurnStatus::Interrupted => "interrupted".to_string(),
                        TurnStatus::InProgress => "thinking".to_string(),
                    };
                    if completed_active_turn && turn_ended {
                        self.push_turn_separator();
                        self.request_thread_usage_refresh();
                    }
                } else if self.prepare_active_agent_thread(&completed.thread_id) {
                    self.agent_activity.record_child_turn(
                        &completed.thread_id,
                        &completed.turn.id,
                        &completed.turn.status,
                    );
                    self.agent_activity.mark_live_thread(&completed.thread_id);
                }
            }
            ServerNotification::ThreadTokenUsageUpdated(usage) => {
                if usage.thread_id == self.thread_id.to_string() {
                    self.apply_token_usage(usage.token_usage);
                }
            }
            ServerNotification::ThreadStatusChanged(changed) => {
                self.handle_remote_thread_status(&changed.thread_id, changed.status);
            }
            ServerNotification::ThreadStarted(started) => {
                let thread = started.thread;
                if thread.session_id == self.thread_id.to_string()
                    && matches!(
                        &thread.source,
                        SessionSource::SubAgent(SubAgentSource::ThreadSpawn { .. })
                    )
                {
                    self.mark_active_agent_thread(&thread.id);
                    self.agent_activity.hydrate_threads(vec![thread]);
                }
            }
            ServerNotification::ThreadArchived(archived) => self.handle_remote_thread_lifecycle(
                &archived.thread_id,
                RemoteThreadLifecycle::Archived,
            ),
            ServerNotification::ThreadDeleted(deleted) => self
                .handle_remote_thread_lifecycle(&deleted.thread_id, RemoteThreadLifecycle::Deleted),
            ServerNotification::ThreadUnarchived(unarchived) => self
                .handle_remote_thread_lifecycle(
                    &unarchived.thread_id,
                    RemoteThreadLifecycle::Unarchived,
                ),
            ServerNotification::ThreadClosed(closed) => self
                .handle_remote_thread_lifecycle(&closed.thread_id, RemoteThreadLifecycle::Closed),
            ServerNotification::ThreadNameUpdated(updated) => {
                if updated.thread_id == self.thread_id.to_string() {
                    self.thread_name = updated.thread_name;
                }
            }
            ServerNotification::ThreadSettingsUpdated(updated) => {
                if updated.thread_id == self.thread_id.to_string() {
                    let settings = updated.thread_settings;
                    self.permission_profile =
                        codex_protocol::models::PermissionProfile::from_legacy_sandbox_policy_for_cwd(
                            &settings.sandbox_policy.to_core(),
                            settings.cwd.as_path(),
                        );
                    self.active_permission_profile =
                        settings.active_permission_profile.map(Into::into);
                    self.model = settings.model;
                    if std::path::Path::new(&self.cwd) != settings.cwd.as_path() {
                        self.reset_workspace_git_status();
                    }
                    self.cwd = settings.cwd.to_string_lossy().to_string();
                    self.diff_store.set_display_root(settings.cwd.as_path());
                    self.refresh_open_diff_view();
                    self.mark_workspace_status_refresh_due();
                    self.approval_policy = settings.approval_policy;
                    self.approvals_reviewer =
                        approvals_reviewer_from_api(settings.approvals_reviewer);
                    self.reasoning_effort = settings.effort;
                    self.service_tier = settings.service_tier;
                    self.collaboration_mode = Some(Box::new(settings.collaboration_mode));
                    self.personality = settings.personality;
                }
            }
            ServerNotification::TurnDiffUpdated(updated) => {
                if updated.thread_id == self.thread_id.to_string() {
                    self.record_turn_diff(&updated.turn_id, &updated.diff);
                    self.mark_workspace_status_refresh_due();
                } else if self.is_active_agent_thread(&updated.thread_id) {
                    self.diff_store.mark_history_truncated();
                    self.refresh_open_diff_view();
                    self.mark_workspace_status_refresh_due();
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
                    self.record_active_goal(Some(updated.goal));
                }
            }
            ServerNotification::ThreadGoalCleared(cleared) => {
                if cleared.thread_id == self.thread_id.to_string() {
                    self.record_active_goal(None);
                }
            }
            ServerNotification::ThreadQueueChanged(_) => {}
            ServerNotification::ItemStarted(started) => {
                if started.thread_id == self.thread_id.to_string() {
                    self.mark_retry_recovered(&started.turn_id);
                    self.mark_active_agent_threads(&started.item);
                    self.agent_activity.reduce_started(&started.item);
                    self.mark_agent_item_live(&started.item);
                    if let Some(title) = item_activity_title(&started.item) {
                        let item_id = started.item.id().to_string();
                        self.record_item_activity(&started.item, title.clone(), "in progress");
                        if !matches!(&started.item, ThreadItem::SubAgentActivity { .. }) {
                            self.push_tool_with_status_for_item(
                                item_id,
                                title,
                                super::ToolBlockStatus::Running,
                            );
                        }
                    }
                } else if self.prepare_active_agent_thread(&started.thread_id) {
                    self.mark_active_agent_threads(&started.item);
                    self.agent_activity.reduce_started(&started.item);
                    self.agent_activity.record_child_item(
                        &started.thread_id,
                        &started.item,
                        AgentItemPhase::Started,
                    );
                    self.agent_activity.mark_live_thread(&started.thread_id);
                    self.mark_agent_item_live(&started.item);
                }
            }
            ServerNotification::ItemCompleted(completed) => {
                if completed.thread_id == self.thread_id.to_string() {
                    self.mark_retry_recovered(&completed.turn_id);
                    self.mark_active_agent_threads(&completed.item);
                    let rewind_anchor = match &completed.item {
                        ThreadItem::UserMessage {
                            client_id: None, ..
                        } => super::rewind::RewindAnchor::for_opening_item(
                            &completed.turn_id,
                            &completed.item,
                        ),
                        ThreadItem::UserMessage {
                            client_id: Some(_), ..
                        }
                        | ThreadItem::HookPrompt { .. }
                        | ThreadItem::AgentMessage { .. }
                        | ThreadItem::Plan { .. }
                        | ThreadItem::Reasoning { .. }
                        | ThreadItem::CommandExecution { .. }
                        | ThreadItem::FileChange { .. }
                        | ThreadItem::McpToolCall { .. }
                        | ThreadItem::DynamicToolCall { .. }
                        | ThreadItem::CollabAgentToolCall { .. }
                        | ThreadItem::WebSearch { .. }
                        | ThreadItem::ImageView { .. }
                        | ThreadItem::Sleep { .. }
                        | ThreadItem::ImageGeneration(_)
                        | ThreadItem::SubAgentActivity { .. }
                        | ThreadItem::EnteredReviewMode { .. }
                        | ThreadItem::ExitedReviewMode { .. }
                        | ThreadItem::ContextCompaction { .. } => None,
                    };
                    self.ingest_completed_item_for_turn(
                        &completed.turn_id,
                        completed.item.clone(),
                        super::CompletedItemOrigin::Live,
                        rewind_anchor,
                    );
                    self.mark_agent_item_live(&completed.item);
                } else if self.prepare_active_agent_thread(&completed.thread_id) {
                    let changed_workspace = matches!(
                        &completed.item,
                        ThreadItem::FileChange { .. } | ThreadItem::CommandExecution { .. }
                    );
                    let incomplete_edit_history =
                        matches!(&completed.item, ThreadItem::FileChange { .. });
                    self.mark_active_agent_threads(&completed.item);
                    self.agent_activity.reduce_completed(&completed.item);
                    self.agent_activity.record_child_item(
                        &completed.thread_id,
                        &completed.item,
                        AgentItemPhase::Completed,
                    );
                    self.agent_activity.mark_live_thread(&completed.thread_id);
                    self.mark_agent_item_live(&completed.item);
                    if incomplete_edit_history {
                        self.diff_store.mark_history_truncated();
                        self.refresh_open_diff_view();
                    }
                    if changed_workspace {
                        self.mark_workspace_status_refresh_due();
                    }
                }
            }
            ServerNotification::CommandExecutionOutputDelta(delta) => {
                if delta.thread_id == self.thread_id.to_string() {
                    self.push_output_delta_with_status_for_item(
                        delta.item_id,
                        delta.delta,
                        super::ToolBlockStatus::Running,
                    );
                } else if self.prepare_active_agent_thread(&delta.thread_id) {
                    self.agent_activity.record_child_progress(
                        &delta.thread_id,
                        &delta.item_id,
                        AgentChildEvent::Output,
                        &delta.delta,
                    );
                    self.agent_activity.mark_live_thread(&delta.thread_id);
                }
            }
            ServerNotification::FileChangePatchUpdated(updated) => {
                if updated.thread_id == self.thread_id.to_string() {
                    self.record_file_changes(
                        &updated.turn_id,
                        &updated.item_id,
                        &updated.changes,
                        codex_app_server_protocol::PatchApplyStatus::InProgress,
                    );
                    self.mark_workspace_status_refresh_due();
                    self.push_diff_with_status_for_item(
                        updated.item_id,
                        super::file_change_detail(&updated.changes),
                        super::ToolBlockStatus::Running,
                    );
                } else if self.prepare_active_agent_thread(&updated.thread_id) {
                    self.diff_store.mark_history_truncated();
                    self.refresh_open_diff_view();
                    self.mark_workspace_status_refresh_due();
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
                if resolved.thread_id == self.thread_id.to_string()
                    || self.is_active_agent_thread(&resolved.thread_id)
                    || self.has_interactive_request(&resolved.request_id)
                {
                    self.push_status("request resolved");
                    if self.remove_interactive_request(&resolved.request_id)
                        == super::InteractiveRequestRemoval::Active
                    {
                        self.activate_next_interactive_request();
                    }
                }
            }
            ServerNotification::CommandExecOutputDelta(delta) => {
                let item_id = format!("command-exec:{}", delta.process_id);
                if let Some(output) = base64::engine::general_purpose::STANDARD
                    .decode(delta.delta_base64)
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                {
                    self.push_output_delta_with_status_for_item(
                        item_id,
                        output,
                        super::ToolBlockStatus::Running,
                    );
                }
            }
            ServerNotification::Error(error) => {
                if error.thread_id == self.thread_id.to_string() {
                    let handled_safety_error =
                        !error.will_retry && self.handle_safety_access_error(&error.error.message);
                    if !handled_safety_error {
                        self.status = if error.will_retry {
                            "retrying".to_string()
                        } else {
                            "error".to_string()
                        };
                        self.push_error(error.error.message);
                    }
                } else if self.prepare_active_agent_thread(&error.thread_id) {
                    self.agent_activity.record_child_error(
                        &error.thread_id,
                        &error.turn_id,
                        &error.error.message,
                        error.will_retry,
                    );
                    self.agent_activity.mark_live_thread(&error.thread_id);
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
            ServerNotification::AccountLoginCompleted(notification) => {
                self.receive_account_login_completed(notification);
            }
            ServerNotification::ModelSafetyBufferingUpdated(updated) => {
                self.on_model_safety_buffering_updated(updated);
                if self.safety_buffering_modal_lines().is_some() {
                    self.close_agent_log();
                    self.close_tool_output();
                    self.close_diff_view();
                    self.pending_session_delete = None;
                }
            }
            ServerNotification::ProcessOutputDelta(_)
            | ServerNotification::ThreadReverted(_)
            | ServerNotification::ProcessExited(_)
            | ServerNotification::FileChangeOutputDelta(_)
            | ServerNotification::HookStarted(_)
            | ServerNotification::HookCompleted(_)
            | ServerNotification::SkillsChanged(_)
            | ServerNotification::ItemGuardianApprovalReviewStarted(_)
            | ServerNotification::ItemGuardianApprovalReviewCompleted(_)
            | ServerNotification::RawResponseItemCompleted(_)
            | ServerNotification::RawResponseCompleted(_)
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
            | ServerNotification::WindowsSandboxSetupCompleted(_) => {}
        }
    }

    fn mark_retry_recovered(&mut self, turn_id: &str) {
        if self.status == "retrying" && self.active_turn_id.as_deref() == Some(turn_id) {
            self.status = "thinking".to_string();
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
        if let Some(thread_id) = request_thread_id(&request)
            && thread_id != self.thread_id.to_string()
            && !self.is_active_agent_thread(&thread_id)
        {
            self.reject_request_with_message(
                app_server,
                request.id().clone(),
                format!("interactive request belongs to inactive thread {thread_id}"),
            )
            .await?;
            return Ok(());
        }
        if let ServerRequest::CurrentTimeRead { request_id, .. } = &request {
            let result = serde_json::to_value(CurrentTimeReadResponse {
                current_time_at: chrono::Utc::now().timestamp(),
            })?;
            let response =
                app_server.resolve_server_request_in_background(request_id.clone(), result);
            self.backend_actions.start(None, async move {
                super::backend_actions::BackendActionResult::CurrentTime {
                    result: response.await,
                }
            });
            return Ok(());
        }
        match super::PendingInteractiveRequest::from_request(&request) {
            Ok(Some(pending)) => match self.receive_interactive_request(pending) {
                Ok(()) => {
                    self.sync_composer_queue_edits(app_server);
                    Ok(())
                }
                Err(pending) => {
                    self.reject_request_with_message(
                        app_server,
                        pending.request_id(),
                        format!(
                            "interactive request queue is full: {}",
                            pending.transcript_title()
                        ),
                    )
                    .await
                }
            },
            Ok(None) => self.reject_unsupported_request(app_server, request).await,
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
        self.token_usage = token_usage_from_breakdown(usage.total);
        self.context_token_usage = token_usage_from_breakdown(usage.last);
        self.model_context_window = usage.model_context_window;
    }
}

fn request_thread_id(request: &ServerRequest) -> Option<String> {
    match request {
        ServerRequest::CommandExecutionRequestApproval { params, .. } => {
            Some(params.thread_id.clone())
        }
        ServerRequest::FileChangeRequestApproval { params, .. } => Some(params.thread_id.clone()),
        ServerRequest::ToolRequestUserInput { params, .. } => Some(params.thread_id.clone()),
        ServerRequest::McpServerElicitationRequest { params, .. } => Some(params.thread_id.clone()),
        ServerRequest::PermissionsRequestApproval { params, .. } => Some(params.thread_id.clone()),
        ServerRequest::DynamicToolCall { params, .. } => Some(params.thread_id.clone()),
        ServerRequest::CurrentTimeRead { params, .. } => Some(params.thread_id.clone()),
        ServerRequest::ApplyPatchApproval { params, .. } => {
            Some(params.conversation_id.to_string())
        }
        ServerRequest::ExecCommandApproval { params, .. } => {
            Some(params.conversation_id.to_string())
        }
        ServerRequest::ChatgptAuthTokensRefresh { .. }
        | ServerRequest::AttestationGenerate { .. } => None,
    }
}

fn token_usage_from_breakdown(breakdown: TokenUsageBreakdown) -> TokenUsage {
    TokenUsage {
        input_tokens: breakdown.input_tokens,
        cached_input_tokens: breakdown.cached_input_tokens,
        output_tokens: breakdown.output_tokens,
        reasoning_output_tokens: breakdown.reasoning_output_tokens,
        total_tokens: breakdown.total_tokens,
    }
}

pub(super) fn item_activity_title(item: &codex_app_server_protocol::ThreadItem) -> Option<String> {
    match item {
        codex_app_server_protocol::ThreadItem::UserMessage { .. }
        | codex_app_server_protocol::ThreadItem::HookPrompt { .. }
        | codex_app_server_protocol::ThreadItem::AgentMessage { .. }
        | codex_app_server_protocol::ThreadItem::Plan { .. }
        | codex_app_server_protocol::ThreadItem::Reasoning { .. }
        | codex_app_server_protocol::ThreadItem::FileChange { .. }
        | codex_app_server_protocol::ThreadItem::EnteredReviewMode { .. }
        | codex_app_server_protocol::ThreadItem::ExitedReviewMode { .. }
        | codex_app_server_protocol::ThreadItem::ContextCompaction { .. } => None,
        codex_app_server_protocol::ThreadItem::CommandExecution { command, .. } => {
            Some(super::command_display::summary(command))
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
        codex_app_server_protocol::ThreadItem::WebSearch(item) => {
            Some(format!("web search: {}", item.query))
        }
        codex_app_server_protocol::ThreadItem::ImageView { path, .. } => {
            Some(format!("view image: {path}"))
        }
        codex_app_server_protocol::ThreadItem::Sleep { duration_ms, .. } => {
            Some(format!("sleep {duration_ms}ms"))
        }
        codex_app_server_protocol::ThreadItem::ImageGeneration(_) => {
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
