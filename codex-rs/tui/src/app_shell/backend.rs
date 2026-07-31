use crate::app_server_session::AppServerSession;
use crate::app_server_session::AppServerStartedThread;
use crate::app_server_session::ForkGoalContinuation;
use crate::app_server_session::TurnPermissionsOverride;
use crate::config_update::write_config_batch;
use crate::legacy_core::config::Config;
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
use codex_app_server_protocol::LoginAccountParams;
use codex_app_server_protocol::LoginAccountResponse;
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
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadStartSource;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnSteerResponse;
use codex_app_server_protocol::UserInput;
use codex_config::types::TuiAppTheme;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::Personality;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::openai_models::ReasoningEffort;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::Result;
use std::path::PathBuf;
use tokio::task::JoinHandle;
use uuid::Uuid;

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

    fn resume_thread_in_background(
        &self,
        config: Config,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<AppServerStartedThread>> + Send + 'static;

    fn fork_thread(
        &mut self,
        config: Config,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<AppServerStartedThread>> + Send;

    fn fork_thread_before_turn(
        &mut self,
        config: Config,
        thread_id: ThreadId,
        before_turn_id: String,
        goal_continuation: ForkGoalContinuation,
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

    fn login_account(
        &mut self,
        params: LoginAccountParams,
    ) -> impl std::future::Future<Output = Result<LoginAccountResponse>> + Send;

    fn cancel_login_account(
        &mut self,
        login_id: String,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn logout_account(&mut self) -> impl std::future::Future<Output = Result<()>> + Send;

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

    fn thread_delete_in_background(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static;

    fn thread_descendant_count_in_background(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<usize>> + Send + 'static;

    fn thread_set_name(
        &mut self,
        thread_id: ThreadId,
        name: String,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn thread_set_name_in_background(
        &self,
        thread_id: ThreadId,
        name: String,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static;

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

    fn persist_settings_update_in_background(
        &self,
        edits: Vec<ConfigEdit>,
        thread_update: Option<ThreadSettingsUpdateParams>,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static;

    /// Persists the terminal client's theme in its local user config.
    ///
    /// Implementations must not route this write through a connected remote
    /// app server because the theme belongs to the client rendering the TUI.
    fn persist_app_theme_in_background(
        &self,
        config_path: AbsolutePathBuf,
        app_theme: TuiAppTheme,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static;

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

    fn turn_start_in_background(
        &self,
        params: AppShellTurnStart,
    ) -> impl std::future::Future<Output = Result<TurnStartResponse>> + Send + 'static;

    fn thread_compact_start_in_background(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static;

    fn turn_interrupt(
        &mut self,
        thread_id: ThreadId,
        turn_id: String,
    ) -> impl std::future::Future<Output = std::result::Result<(), TypedRequestError>> + Send;

    fn turn_steer(
        &mut self,
        params: AppShellTurnSteer,
    ) -> impl std::future::Future<Output = std::result::Result<TurnSteerResponse, TypedRequestError>>
    + Send;

    fn resolve_server_request(
        &self,
        request_id: RequestId,
        result: serde_json::Value,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send;

    fn resolve_server_request_in_background(
        &self,
        request_id: RequestId,
        result: serde_json::Value,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send + 'static;

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

pub(super) fn app_shell_request_id(prefix: &str) -> RequestId {
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

#[derive(Debug, Clone)]
pub(super) struct AppShellTurnSteer {
    pub(super) thread_id: ThreadId,
    pub(super) turn_id: String,
    pub(super) client_user_message_id: String,
    pub(super) items: Vec<UserInput>,
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

    fn resume_thread_in_background(
        &self,
        config: Config,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<AppServerStartedThread>> + Send + 'static {
        AppServerSession::resume_thread_in_background(self, config, thread_id)
    }

    async fn fork_thread(
        &mut self,
        config: Config,
        thread_id: ThreadId,
    ) -> Result<AppServerStartedThread> {
        AppServerSession::fork_thread(self, config, thread_id).await
    }

    async fn fork_thread_before_turn(
        &mut self,
        config: Config,
        thread_id: ThreadId,
        before_turn_id: String,
        goal_continuation: ForkGoalContinuation,
    ) -> Result<AppServerStartedThread> {
        AppServerSession::fork_thread_before_turn(
            self,
            config,
            thread_id,
            before_turn_id,
            goal_continuation,
        )
        .await
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

    async fn login_account(&mut self, params: LoginAccountParams) -> Result<LoginAccountResponse> {
        AppServerSession::login_account(self, params).await
    }

    async fn cancel_login_account(&mut self, login_id: String) -> Result<()> {
        AppServerSession::cancel_login_account(self, login_id).await?;
        Ok(())
    }

    async fn logout_account(&mut self) -> Result<()> {
        AppServerSession::logout_account(self).await
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

    fn thread_delete_in_background(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
        super::backend_background::delete_thread(AppServerSession::request_handle(self), thread_id)
    }

    fn thread_descendant_count_in_background(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<usize>> + Send + 'static {
        super::backend_background::count_descendants(
            AppServerSession::request_handle(self),
            thread_id,
        )
    }

    async fn thread_set_name(&mut self, thread_id: ThreadId, name: String) -> Result<()> {
        AppServerSession::thread_set_name(self, thread_id, name).await
    }

    fn thread_set_name_in_background(
        &self,
        thread_id: ThreadId,
        name: String,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
        super::backend_background::set_thread_name(
            AppServerSession::request_handle(self),
            thread_id,
            name,
        )
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

    fn persist_settings_update_in_background(
        &self,
        edits: Vec<ConfigEdit>,
        thread_update: Option<ThreadSettingsUpdateParams>,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
        let thread_update = thread_update
            .map(|params| AppServerSession::thread_settings_update_in_background(self, params));
        super::settings::persist_settings_update(
            AppServerSession::request_handle(self),
            edits,
            thread_update,
        )
    }

    // An `async fn` would capture `&self`, but callers must be able to spawn
    // this self-independent write as a `'static` background task.
    #[allow(clippy::manual_async_fn)]
    fn persist_app_theme_in_background(
        &self,
        config_path: AbsolutePathBuf,
        app_theme: TuiAppTheme,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
        super::local_app_theme::persist(config_path, app_theme)
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

    fn turn_start_in_background(
        &self,
        params: AppShellTurnStart,
    ) -> impl std::future::Future<Output = Result<TurnStartResponse>> + Send + 'static {
        super::backend_background::start_turn(AppServerSession::request_handle(self), params)
    }

    fn thread_compact_start_in_background(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
        super::backend_background::compact_thread(AppServerSession::request_handle(self), thread_id)
    }

    async fn turn_interrupt(
        &mut self,
        thread_id: ThreadId,
        turn_id: String,
    ) -> std::result::Result<(), TypedRequestError> {
        AppServerSession::turn_interrupt(self, thread_id, turn_id).await
    }

    async fn turn_steer(
        &mut self,
        params: AppShellTurnSteer,
    ) -> std::result::Result<TurnSteerResponse, TypedRequestError> {
        AppServerSession::turn_steer(
            self,
            params.thread_id,
            params.turn_id,
            params.client_user_message_id,
            params.items,
        )
        .await
    }

    async fn resolve_server_request(
        &self,
        request_id: RequestId,
        result: serde_json::Value,
    ) -> std::io::Result<()> {
        AppServerSession::resolve_server_request(self, request_id, result).await
    }

    fn resolve_server_request_in_background(
        &self,
        request_id: RequestId,
        result: serde_json::Value,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send + 'static {
        let request_handle = AppServerSession::request_handle(self);
        async move {
            request_handle
                .resolve_server_request(request_id, result)
                .await
        }
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
        super::backend_cleanup::unsubscribe_threads(
            AppServerSession::request_handle(self),
            thread_ids,
        )
        .await;
    }

    fn unsubscribe_threads_in_background(&self, thread_ids: Vec<ThreadId>) -> JoinHandle<()> {
        let request_handle = AppServerSession::request_handle(self);
        tokio::spawn(super::backend_cleanup::unsubscribe_threads(
            request_handle,
            thread_ids,
        ))
    }

    async fn shutdown(self) -> std::io::Result<()> {
        AppServerSession::shutdown(self).await
    }
}

pub(super) async fn shutdown_app_shell_backend<S>(app_server: S) -> std::io::Result<()>
where
    S: AppShellBackend,
{
    app_server.shutdown().await
}
