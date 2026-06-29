use crate::app_server_session::AppServerSession;
use crate::app_server_session::AppServerStartedThread;
use crate::app_server_session::TurnPermissionsOverride;
use crate::config_update::write_config_batch;
use crate::legacy_core::config::Config;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigWriteResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadStartSource;
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

    fn write_config(
        &mut self,
        edits: Vec<ConfigEdit>,
    ) -> impl std::future::Future<Output = Result<ConfigWriteResponse>> + Send;

    fn thread_settings_update(
        &mut self,
        params: ThreadSettingsUpdateParams,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn turn_start(
        &mut self,
        params: AppShellTurnStart,
    ) -> impl std::future::Future<Output = Result<TurnStartResponse>> + Send;

    fn turn_interrupt(
        &mut self,
        thread_id: ThreadId,
        turn_id: String,
    ) -> impl std::future::Future<Output = std::result::Result<(), TypedRequestError>> + Send;

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

    fn shutdown(self) -> impl std::future::Future<Output = std::io::Result<()>> + Send
    where
        Self: Sized;
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

    async fn write_config(&mut self, edits: Vec<ConfigEdit>) -> Result<ConfigWriteResponse> {
        write_config_batch(AppServerSession::request_handle(self), edits).await
    }

    async fn thread_settings_update(&mut self, params: ThreadSettingsUpdateParams) -> Result<()> {
        AppServerSession::thread_settings_update(self, params).await
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
