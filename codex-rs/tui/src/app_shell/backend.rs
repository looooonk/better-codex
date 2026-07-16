use crate::app_server_session::AppServerSession;
use crate::app_server_session::AppServerStartedThread;
use crate::app_server_session::TurnPermissionsOverride;
use crate::config_update::write_config_batch;
use crate::legacy_core::config::Config;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigValueWriteParams;
use codex_app_server_protocol::ConfigWriteResponse;
use codex_app_server_protocol::ExternalAgentConfigDetectParams;
use codex_app_server_protocol::ExternalAgentConfigDetectResponse;
use codex_app_server_protocol::ExternalAgentConfigMigrationItem;
use codex_app_server_protocol::GetAccountRateLimitsResponse;
use codex_app_server_protocol::ListMcpServerStatusParams;
use codex_app_server_protocol::ListMcpServerStatusResponse;
use codex_app_server_protocol::McpServerOauthLoginParams;
use codex_app_server_protocol::McpServerOauthLoginResponse;
use codex_app_server_protocol::McpServerRefreshResponse;
use codex_app_server_protocol::MergeStrategy;
use codex_app_server_protocol::PluginInstallParams;
use codex_app_server_protocol::PluginInstallResponse;
use codex_app_server_protocol::PluginListParams;
use codex_app_server_protocol::PluginListResponse;
use codex_app_server_protocol::PluginUninstallParams;
use codex_app_server_protocol::PluginUninstallResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadGoalClearResponse;
use codex_app_server_protocol::ThreadGoalGetParams;
use codex_app_server_protocol::ThreadGoalGetResponse;
use codex_app_server_protocol::ThreadGoalSetResponse;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadRollbackResponse;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadStartSource;
use codex_app_server_protocol::ThreadUnsubscribeParams;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnSteerResponse;
use codex_app_server_protocol::UserInput;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::Personality;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::openai_models::ReasoningEffort;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::Result;
use std::path::PathBuf;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio::time::timeout;
use uuid::Uuid;

const MAX_CONCURRENT_THREAD_UNSUBSCRIBES: usize = 8;
const THREAD_UNSUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 3);
const THREAD_UNSUBSCRIBE_CLEANUP_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 5);

/// Backend operations the app shell drives through the app-server boundary.
///
/// Implementations should preserve app-server request semantics while allowing
/// the shell to be tested without a live server.
pub(super) trait AppShellBackend {
    fn start_thread_with_session_start_source(
        &mut self,
        config: &Config,
        session_start_source: Option<ThreadStartSource>,
    ) -> impl std::future::Future<Output = Result<AppServerStartedThread>> + Send;

    fn resume_thread(
        &mut self,
        config: Config,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<AppServerStartedThread>> + Send;

    fn fork_thread(
        &mut self,
        config: Config,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<AppServerStartedThread>> + Send;

    fn thread_list(
        &mut self,
        params: ThreadListParams,
    ) -> impl std::future::Future<Output = Result<ThreadListResponse>> + Send;

    /// Starts a session-list lookup without borrowing the event-loop-owned backend.
    fn thread_list_in_background(
        &self,
        params: ThreadListParams,
    ) -> impl std::future::Future<Output = Result<ThreadListResponse>> + Send + 'static;

    /// Reads a complete thread transcript without borrowing the event-loop-owned backend.
    fn thread_read_full_in_background(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<Thread>> + Send + 'static;

    /// Starts an account rate-limit lookup without borrowing the event-loop-owned backend.
    fn account_rate_limits_in_background(
        &self,
    ) -> impl std::future::Future<Output = Result<GetAccountRateLimitsResponse>> + Send + 'static;

    fn thread_archive(
        &mut self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn thread_unarchive(
        &mut self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<Thread>> + Send;

    fn thread_delete(
        &mut self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn thread_set_name(
        &mut self,
        thread_id: ThreadId,
        name: String,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn thread_goal_get(
        &mut self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<ThreadGoalGetResponse>> + Send;

    /// Starts a goal lookup without borrowing the event-loop-owned backend.
    fn thread_goal_get_in_background(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<ThreadGoalGetResponse>> + Send + 'static;

    fn thread_goal_set(
        &mut self,
        thread_id: ThreadId,
        objective: Option<String>,
        status: Option<ThreadGoalStatus>,
        token_budget: Option<Option<i64>>,
    ) -> impl std::future::Future<Output = Result<ThreadGoalSetResponse>> + Send;

    fn thread_goal_clear(
        &mut self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<ThreadGoalClearResponse>> + Send;

    fn write_config(
        &mut self,
        edits: Vec<ConfigEdit>,
    ) -> impl std::future::Future<Output = Result<ConfigWriteResponse>> + Send;

    fn thread_settings_update(
        &mut self,
        params: ThreadSettingsUpdateParams,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn mcp_server_status_list(
        &mut self,
        params: ListMcpServerStatusParams,
    ) -> impl std::future::Future<Output = Result<ListMcpServerStatusResponse>> + Send;

    fn mcp_server_oauth_login(
        &mut self,
        params: McpServerOauthLoginParams,
    ) -> impl std::future::Future<Output = Result<McpServerOauthLoginResponse>> + Send;

    fn mcp_server_refresh(
        &mut self,
    ) -> impl std::future::Future<Output = Result<McpServerRefreshResponse>> + Send;

    fn mcp_server_write_config(
        &mut self,
        server_name: String,
        value: serde_json::Value,
        merge_strategy: MergeStrategy,
    ) -> impl std::future::Future<Output = Result<ConfigWriteResponse>> + Send;

    fn plugin_list(
        &mut self,
        params: PluginListParams,
    ) -> impl std::future::Future<Output = Result<PluginListResponse>> + Send;

    fn plugin_install(
        &mut self,
        params: PluginInstallParams,
    ) -> impl std::future::Future<Output = Result<PluginInstallResponse>> + Send;

    fn plugin_uninstall(
        &mut self,
        params: PluginUninstallParams,
    ) -> impl std::future::Future<Output = Result<PluginUninstallResponse>> + Send;

    fn plugin_set_enabled(
        &mut self,
        plugin_id: String,
        enabled: bool,
    ) -> impl std::future::Future<Output = Result<ConfigWriteResponse>> + Send;

    fn uses_remote_workspace(&self) -> bool;

    fn uses_embedded_app_server(&self) -> bool;

    fn external_agent_config_import_in_progress(&self) -> bool;

    fn external_agent_config_detect(
        &mut self,
        params: ExternalAgentConfigDetectParams,
    ) -> impl std::future::Future<Output = Result<ExternalAgentConfigDetectResponse>> + Send;

    fn external_agent_config_import(
        &mut self,
        migration_items: Vec<ExternalAgentConfigMigrationItem>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn consume_external_agent_config_import_completion(&self) -> bool;

    fn turn_start(
        &mut self,
        params: AppShellTurnStart,
    ) -> impl std::future::Future<Output = Result<TurnStartResponse>> + Send;

    fn turn_interrupt(
        &mut self,
        thread_id: ThreadId,
        turn_id: String,
    ) -> impl std::future::Future<Output = std::result::Result<(), TypedRequestError>> + Send;

    fn thread_rollback(
        &mut self,
        thread_id: ThreadId,
        num_turns: u32,
    ) -> impl std::future::Future<Output = Result<ThreadRollbackResponse>> + Send;

    fn turn_steer(
        &mut self,
        thread_id: ThreadId,
        turn_id: String,
        items: Vec<UserInput>,
    ) -> impl std::future::Future<Output = std::result::Result<TurnSteerResponse, TypedRequestError>>
    + Send;

    fn resolve_server_request(
        &self,
        request_id: RequestId,
        result: serde_json::Value,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send;

    fn reject_server_request(
        &self,
        request_id: RequestId,
        error: codex_app_server_protocol::JSONRPCErrorError,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send;

    fn unsubscribe_thread(
        &mut self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn unsubscribe_threads(
        &self,
        thread_ids: Vec<ThreadId>,
    ) -> impl std::future::Future<Output = ()> + Send;

    /// Starts best-effort subscription cleanup without blocking the shell event loop.
    fn unsubscribe_threads_in_background(&self, thread_ids: Vec<ThreadId>) -> JoinHandle<()>;

    fn shutdown(self) -> impl std::future::Future<Output = std::io::Result<()>> + Send
    where
        Self: Sized;
}

fn app_shell_request_id(prefix: &str) -> RequestId {
    RequestId::String(format!("{prefix}-{}", Uuid::new_v4()))
}

#[derive(Debug, Clone)]
pub(super) struct AppShellTurnStart {
    pub(super) thread_id: ThreadId,
    pub(super) items: Vec<UserInput>,
    pub(super) cwd: PathBuf,
    pub(super) approval_policy: AskForApproval,
    pub(super) approvals_reviewer: ApprovalsReviewer,
    pub(super) permissions_override: TurnPermissionsOverride,
    pub(super) workspace_roots: Vec<AbsolutePathBuf>,
    pub(super) model: String,
    pub(super) effort: Option<ReasoningEffort>,
    pub(super) summary: Option<ReasoningSummary>,
    pub(super) service_tier: Option<Option<String>>,
    pub(super) collaboration_mode: Option<CollaborationMode>,
    pub(super) personality: Option<Personality>,
    pub(super) output_schema: Option<serde_json::Value>,
}

impl AppShellBackend for AppServerSession {
    async fn start_thread_with_session_start_source(
        &mut self,
        config: &Config,
        session_start_source: Option<ThreadStartSource>,
    ) -> Result<AppServerStartedThread> {
        AppServerSession::start_thread_with_session_start_source(self, config, session_start_source)
            .await
    }

    async fn resume_thread(
        &mut self,
        config: Config,
        thread_id: ThreadId,
    ) -> Result<AppServerStartedThread> {
        AppServerSession::resume_thread(self, config, thread_id).await
    }

    async fn fork_thread(
        &mut self,
        config: Config,
        thread_id: ThreadId,
    ) -> Result<AppServerStartedThread> {
        AppServerSession::fork_thread(self, config, thread_id).await
    }

    async fn thread_list(&mut self, params: ThreadListParams) -> Result<ThreadListResponse> {
        AppServerSession::thread_list(self, params).await
    }

    fn thread_list_in_background(
        &self,
        params: ThreadListParams,
    ) -> impl std::future::Future<Output = Result<ThreadListResponse>> + Send + 'static {
        let request_handle = AppServerSession::request_handle(self);
        async move {
            request_handle
                .request_typed(ClientRequest::ThreadList {
                    request_id: app_shell_request_id("app-shell-session-list"),
                    params,
                })
                .await
                .map_err(Into::into)
        }
    }

    fn thread_read_full_in_background(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<Thread>> + Send + 'static {
        let request_handle = AppServerSession::request_handle(self);
        async move {
            let response = request_handle
                .request_typed::<ThreadReadResponse>(ClientRequest::ThreadRead {
                    request_id: app_shell_request_id("app-shell-thread-log"),
                    params: ThreadReadParams {
                        thread_id: thread_id.to_string(),
                        include_turns: true,
                    },
                })
                .await?;
            Ok(response.thread)
        }
    }

    fn account_rate_limits_in_background(
        &self,
    ) -> impl std::future::Future<Output = Result<GetAccountRateLimitsResponse>> + Send + 'static
    {
        let request_handle = AppServerSession::request_handle(self);
        async move {
            request_handle
                .request_typed(ClientRequest::GetAccountRateLimits {
                    request_id: app_shell_request_id("app-shell-rate-limits"),
                    params: None,
                })
                .await
                .map_err(Into::into)
        }
    }

    async fn thread_archive(&mut self, thread_id: ThreadId) -> Result<()> {
        AppServerSession::thread_archive(self, thread_id).await
    }

    async fn thread_unarchive(&mut self, thread_id: ThreadId) -> Result<Thread> {
        AppServerSession::thread_unarchive(self, thread_id).await
    }

    async fn thread_delete(&mut self, thread_id: ThreadId) -> Result<()> {
        AppServerSession::thread_delete(self, thread_id).await
    }

    async fn thread_set_name(&mut self, thread_id: ThreadId, name: String) -> Result<()> {
        AppServerSession::thread_set_name(self, thread_id, name).await
    }

    async fn thread_goal_get(&mut self, thread_id: ThreadId) -> Result<ThreadGoalGetResponse> {
        AppServerSession::thread_goal_get(self, thread_id).await
    }

    fn thread_goal_get_in_background(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<ThreadGoalGetResponse>> + Send + 'static {
        let request_handle = AppServerSession::request_handle(self);
        async move {
            request_handle
                .request_typed(ClientRequest::ThreadGoalGet {
                    request_id: app_shell_request_id("app-shell-goal"),
                    params: ThreadGoalGetParams {
                        thread_id: thread_id.to_string(),
                    },
                })
                .await
                .map_err(Into::into)
        }
    }

    async fn thread_goal_set(
        &mut self,
        thread_id: ThreadId,
        objective: Option<String>,
        status: Option<ThreadGoalStatus>,
        token_budget: Option<Option<i64>>,
    ) -> Result<ThreadGoalSetResponse> {
        AppServerSession::thread_goal_set(self, thread_id, objective, status, token_budget).await
    }

    async fn thread_goal_clear(&mut self, thread_id: ThreadId) -> Result<ThreadGoalClearResponse> {
        AppServerSession::thread_goal_clear(self, thread_id).await
    }

    async fn write_config(&mut self, edits: Vec<ConfigEdit>) -> Result<ConfigWriteResponse> {
        write_config_batch(AppServerSession::request_handle(self), edits).await
    }

    async fn thread_settings_update(&mut self, params: ThreadSettingsUpdateParams) -> Result<()> {
        AppServerSession::thread_settings_update(self, params).await
    }

    async fn mcp_server_status_list(
        &mut self,
        params: ListMcpServerStatusParams,
    ) -> Result<ListMcpServerStatusResponse> {
        AppServerSession::request_handle(self)
            .request_typed(ClientRequest::McpServerStatusList {
                request_id: app_shell_request_id("app-shell-mcp"),
                params,
            })
            .await
            .map_err(Into::into)
    }

    async fn mcp_server_oauth_login(
        &mut self,
        params: McpServerOauthLoginParams,
    ) -> Result<McpServerOauthLoginResponse> {
        AppServerSession::request_handle(self)
            .request_typed(ClientRequest::McpServerOauthLogin {
                request_id: app_shell_request_id("app-shell-mcp-login"),
                params,
            })
            .await
            .map_err(Into::into)
    }

    async fn mcp_server_refresh(&mut self) -> Result<McpServerRefreshResponse> {
        AppServerSession::request_handle(self)
            .request_typed(ClientRequest::McpServerRefresh {
                request_id: app_shell_request_id("app-shell-mcp-refresh"),
                params: None,
            })
            .await
            .map_err(Into::into)
    }

    async fn mcp_server_write_config(
        &mut self,
        server_name: String,
        value: serde_json::Value,
        merge_strategy: MergeStrategy,
    ) -> Result<ConfigWriteResponse> {
        AppServerSession::request_handle(self)
            .request_typed(ClientRequest::ConfigValueWrite {
                request_id: app_shell_request_id("app-shell-mcp-config"),
                params: ConfigValueWriteParams {
                    key_path: format!("mcp_servers.{}", serde_json::Value::String(server_name)),
                    value,
                    merge_strategy,
                    file_path: None,
                    expected_version: None,
                },
            })
            .await
            .map_err(Into::into)
    }

    async fn plugin_list(&mut self, params: PluginListParams) -> Result<PluginListResponse> {
        AppServerSession::request_handle(self)
            .request_typed(ClientRequest::PluginList {
                request_id: RequestId::String(format!("app-shell-plugin-{}", Uuid::new_v4())),
                params,
            })
            .await
            .map_err(Into::into)
    }

    async fn plugin_install(
        &mut self,
        params: PluginInstallParams,
    ) -> Result<PluginInstallResponse> {
        AppServerSession::request_handle(self)
            .request_typed(ClientRequest::PluginInstall {
                request_id: app_shell_request_id("app-shell-plugin-install"),
                params,
            })
            .await
            .map_err(Into::into)
    }

    async fn plugin_uninstall(
        &mut self,
        params: PluginUninstallParams,
    ) -> Result<PluginUninstallResponse> {
        AppServerSession::request_handle(self)
            .request_typed(ClientRequest::PluginUninstall {
                request_id: app_shell_request_id("app-shell-plugin-uninstall"),
                params,
            })
            .await
            .map_err(Into::into)
    }

    async fn plugin_set_enabled(
        &mut self,
        plugin_id: String,
        enabled: bool,
    ) -> Result<ConfigWriteResponse> {
        AppServerSession::request_handle(self)
            .request_typed(ClientRequest::ConfigValueWrite {
                request_id: app_shell_request_id("app-shell-plugin-enable"),
                params: ConfigValueWriteParams {
                    key_path: format!("plugins.{plugin_id}"),
                    value: serde_json::json!({ "enabled": enabled }),
                    merge_strategy: MergeStrategy::Upsert,
                    file_path: None,
                    expected_version: None,
                },
            })
            .await
            .map_err(Into::into)
    }

    fn uses_remote_workspace(&self) -> bool {
        AppServerSession::uses_remote_workspace(self)
    }

    fn uses_embedded_app_server(&self) -> bool {
        AppServerSession::uses_embedded_app_server(self)
    }

    fn external_agent_config_import_in_progress(&self) -> bool {
        AppServerSession::external_agent_config_import_in_progress(self)
    }

    async fn external_agent_config_detect(
        &mut self,
        params: ExternalAgentConfigDetectParams,
    ) -> Result<ExternalAgentConfigDetectResponse> {
        AppServerSession::external_agent_config_detect(self, params).await
    }

    async fn external_agent_config_import(
        &mut self,
        migration_items: Vec<ExternalAgentConfigMigrationItem>,
    ) -> Result<()> {
        AppServerSession::external_agent_config_import(self, migration_items).await
    }

    fn consume_external_agent_config_import_completion(&self) -> bool {
        AppServerSession::consume_external_agent_config_import_completion(self)
    }

    async fn turn_start(&mut self, params: AppShellTurnStart) -> Result<TurnStartResponse> {
        AppServerSession::turn_start(
            self,
            params.thread_id,
            params.items,
            params.cwd,
            params.approval_policy,
            params.approvals_reviewer,
            params.permissions_override,
            &params.workspace_roots,
            params.model,
            params.effort,
            params.summary,
            params.service_tier,
            params.collaboration_mode,
            params.personality,
            params.output_schema,
        )
        .await
    }

    async fn turn_interrupt(
        &mut self,
        thread_id: ThreadId,
        turn_id: String,
    ) -> std::result::Result<(), TypedRequestError> {
        AppServerSession::turn_interrupt(self, thread_id, turn_id).await
    }

    async fn thread_rollback(
        &mut self,
        thread_id: ThreadId,
        num_turns: u32,
    ) -> Result<ThreadRollbackResponse> {
        AppServerSession::thread_rollback(self, thread_id, num_turns).await
    }

    async fn turn_steer(
        &mut self,
        thread_id: ThreadId,
        turn_id: String,
        items: Vec<UserInput>,
    ) -> std::result::Result<TurnSteerResponse, TypedRequestError> {
        AppServerSession::turn_steer(self, thread_id, turn_id, items).await
    }

    async fn resolve_server_request(
        &self,
        request_id: RequestId,
        result: serde_json::Value,
    ) -> std::io::Result<()> {
        AppServerSession::resolve_server_request(self, request_id, result).await
    }

    async fn reject_server_request(
        &self,
        request_id: RequestId,
        error: codex_app_server_protocol::JSONRPCErrorError,
    ) -> std::io::Result<()> {
        AppServerSession::reject_server_request(self, request_id, error).await
    }

    async fn unsubscribe_thread(&mut self, thread_id: ThreadId) -> Result<()> {
        AppServerSession::thread_unsubscribe(self, thread_id).await
    }

    async fn unsubscribe_threads(&self, thread_ids: Vec<ThreadId>) {
        unsubscribe_threads_with_timeout(AppServerSession::request_handle(self), thread_ids).await;
    }

    fn unsubscribe_threads_in_background(&self, thread_ids: Vec<ThreadId>) -> JoinHandle<()> {
        let request_handle = AppServerSession::request_handle(self);
        tokio::spawn(unsubscribe_threads_with_timeout(request_handle, thread_ids))
    }

    async fn shutdown(self) -> std::io::Result<()> {
        AppServerSession::shutdown(self).await
    }
}

async fn unsubscribe_threads_with_timeout(
    request_handle: AppServerRequestHandle,
    thread_ids: Vec<ThreadId>,
) {
    let deadline = Instant::now() + THREAD_UNSUBSCRIBE_CLEANUP_TIMEOUT;
    for (batch_index, batch) in thread_ids
        .chunks(MAX_CONCURRENT_THREAD_UNSUBSCRIBES)
        .enumerate()
    {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            tracing::warn!(
                remaining = thread_ids
                    .len()
                    .saturating_sub(batch_index * MAX_CONCURRENT_THREAD_UNSUBSCRIBES),
                timeout = ?THREAD_UNSUBSCRIBE_CLEANUP_TIMEOUT,
                "thread subscription cleanup timed out"
            );
            break;
        }
        let request_timeout = remaining.min(THREAD_UNSUBSCRIBE_TIMEOUT);
        let mut requests = JoinSet::new();
        for thread_id in batch.iter().copied() {
            let request_handle = request_handle.clone();
            requests.spawn(async move {
                let request = request_handle.request_typed::<ThreadUnsubscribeResponse>(
                    ClientRequest::ThreadUnsubscribe {
                        request_id: app_shell_request_id("app-shell-unsubscribe"),
                        params: ThreadUnsubscribeParams {
                            thread_id: thread_id.to_string(),
                        },
                    },
                );
                (thread_id, timeout(request_timeout, request).await)
            });
        }
        while let Some(result) = requests.join_next().await {
            match result {
                Ok((_, Ok(Ok(_)))) => {}
                Ok((thread_id, Ok(Err(err)))) => {
                    tracing::warn!(%thread_id, %err, "failed to unsubscribe replaced session thread");
                }
                Ok((thread_id, Err(_))) => {
                    tracing::warn!(
                        %thread_id,
                        timeout = ?request_timeout,
                        "replaced session unsubscribe timed out"
                    );
                }
                Err(err) => tracing::warn!(%err, "replaced session unsubscribe task failed"),
            }
        }
    }
}

pub(super) async fn shutdown_app_shell_backend<S>(app_server: S) -> std::io::Result<()>
where
    S: AppShellBackend,
{
    app_server.shutdown().await
}
