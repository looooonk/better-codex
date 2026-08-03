use super::*;

/// Typed request operations used by detached app-server thread workflows.
///
/// Implementations must dispatch the supplied protocol request unchanged and
/// preserve app-server transport, server, and response-decoding errors.
trait BackgroundRequestHandle: Send + Sync {
    fn send_thread_fork_request(
        &self,
        request: ClientRequest,
    ) -> impl std::future::Future<Output = Result<ThreadForkResponse, TypedRequestError>> + Send;

    fn send_thread_read_request(
        &self,
        request: ClientRequest,
    ) -> impl std::future::Future<Output = Result<ThreadReadResponse, TypedRequestError>> + Send;
}

impl BackgroundRequestHandle for AppServerRequestHandle {
    fn send_thread_fork_request(
        &self,
        request: ClientRequest,
    ) -> impl std::future::Future<Output = Result<ThreadForkResponse, TypedRequestError>> + Send
    {
        self.request_typed(request)
    }

    fn send_thread_read_request(
        &self,
        request: ClientRequest,
    ) -> impl std::future::Future<Output = Result<ThreadReadResponse, TypedRequestError>> + Send
    {
        self.request_typed(request)
    }
}

impl AppServerSession {
    pub(crate) fn resume_thread_in_background(
        &self,
        config: Config,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<AppServerStartedThread>> + Send + 'static {
        let request_handle = self.request_handle();
        let thread_params_mode = self.thread_params_mode();
        let remote_cwd_override = self.remote_cwd_override.clone();
        let session_config = self.session_config_with_effective_service_tier(&config);
        resume_thread(
            request_handle,
            config,
            session_config,
            thread_id,
            thread_params_mode,
            remote_cwd_override,
            RequestId::String(format!("app-shell-thread-resume-{}", Uuid::new_v4())),
        )
    }

    pub(crate) fn fork_thread_before_turn_in_background(
        &self,
        config: Config,
        thread_id: ThreadId,
        before_turn_id: String,
        goal_continuation: ForkGoalContinuation,
    ) -> impl std::future::Future<Output = Result<AppServerStartedThread>> + Send + 'static {
        let request_handle = self.request_handle();
        let thread_params_mode = self.thread_params_mode();
        let remote_cwd_override = self.remote_cwd_override.clone();
        let session_config = self.session_config_with_effective_service_tier(&config);
        fork_thread_before_turn(BackgroundThreadFork {
            request_handle,
            config,
            session_config,
            thread_id,
            before_turn_id,
            goal_continuation,
            thread_params_mode,
            remote_cwd_override,
            request_id: RequestId::String(format!("app-shell-thread-fork-{}", Uuid::new_v4())),
        })
    }

    pub(crate) fn thread_settings_update_in_background(
        &self,
        params: ThreadSettingsUpdateParams,
    ) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
        thread_settings_update(
            self.request_handle(),
            Arc::clone(&self.thread_settings_update_supported),
            RequestId::String(format!("app-shell-thread-settings-{}", Uuid::new_v4())),
            params,
        )
    }
}

struct BackgroundThreadFork<H = AppServerRequestHandle> {
    request_handle: H,
    config: Config,
    session_config: Config,
    thread_id: ThreadId,
    before_turn_id: String,
    goal_continuation: ForkGoalContinuation,
    thread_params_mode: ThreadParamsMode,
    remote_cwd_override: Option<PathBuf>,
    request_id: RequestId,
}

async fn fork_thread_before_turn<H>(
    request: BackgroundThreadFork<H>,
) -> Result<AppServerStartedThread>
where
    H: BackgroundRequestHandle,
{
    let BackgroundThreadFork {
        request_handle,
        config,
        session_config,
        thread_id,
        before_turn_id,
        goal_continuation,
        thread_params_mode,
        remote_cwd_override,
        request_id,
    } = request;
    let response = request_handle
        .send_thread_fork_request(ClientRequest::ThreadFork {
            request_id,
            params: ThreadForkParams {
                last_turn_id: None,
                before_turn_id: Some(before_turn_id),
                defer_goal_continuation: goal_continuation
                    == ForkGoalContinuation::DeferUntilNextTurn,
                ..thread_fork_params_from_config(
                    session_config,
                    thread_id,
                    thread_params_mode,
                    remote_cwd_override.as_deref(),
                )
            },
        })
        .await
        .map_err(|err| bootstrap_request_error("thread/fork failed in TUI", err))?;
    let fork_parent_title =
        fork_parent_title(&request_handle, response.thread.forked_from_id.as_deref()).await;
    let mut started =
        started_thread_from_fork_response(response, &config, thread_params_mode).await?;
    started.session.fork_parent_title = fork_parent_title;
    Ok(started)
}

pub(super) async fn resume_thread(
    request_handle: AppServerRequestHandle,
    config: Config,
    session_config: Config,
    thread_id: ThreadId,
    thread_params_mode: ThreadParamsMode,
    remote_cwd_override: Option<PathBuf>,
    request_id: RequestId,
) -> Result<AppServerStartedThread> {
    let response: ThreadResumeResponse = request_handle
        .request_typed(ClientRequest::ThreadResume {
            request_id,
            params: thread_resume_params_from_config(
                session_config,
                thread_id,
                thread_params_mode,
                remote_cwd_override.as_deref(),
            ),
        })
        .await
        .map_err(|err| bootstrap_request_error("thread/resume failed in TUI", err))?;
    let fork_parent_title =
        fork_parent_title(&request_handle, response.thread.forked_from_id.as_deref()).await;
    let session_id = response.thread.session_id.clone();
    let mut started =
        started_thread_from_resume_response(response, &config, thread_params_mode).await?;
    started.session.fork_parent_title = fork_parent_title;
    started.agent_history_task = agent_history::spawn_resumed_agent_history(
        request_handle,
        started.session.thread_id,
        session_id,
        &started.turns,
    );
    Ok(started)
}

pub(super) async fn thread_settings_update(
    request_handle: AppServerRequestHandle,
    supported: Arc<AtomicBool>,
    request_id: RequestId,
    params: ThreadSettingsUpdateParams,
) -> Result<()> {
    if !supported.load(Ordering::Relaxed) {
        return Ok(());
    }
    match request_handle
        .request_typed::<ThreadSettingsUpdateResponse>(ClientRequest::ThreadSettingsUpdate {
            request_id,
            params,
        })
        .await
    {
        Ok(_) => Ok(()),
        Err(TypedRequestError::Server { source, .. })
            if is_thread_settings_update_unsupported(&source) =>
        {
            supported.store(false, Ordering::Relaxed);
            Ok(())
        }
        Err(err) => Err(err).wrap_err("thread/settings/update failed in TUI"),
    }
}

async fn fork_parent_title<H>(request_handle: &H, forked_from_id: Option<&str>) -> Option<String>
where
    H: BackgroundRequestHandle,
{
    let forked_from_id = forked_from_id?;
    let thread_id = match ThreadId::from_string(forked_from_id) {
        Ok(thread_id) => thread_id,
        Err(err) => {
            tracing::warn!("Failed to parse fork parent thread id from app server: {err}");
            return None;
        }
    };
    match request_handle
        .send_thread_read_request(ClientRequest::ThreadRead {
            request_id: RequestId::String(format!("app-shell-fork-parent-{}", Uuid::new_v4())),
            params: ThreadReadParams {
                thread_id: thread_id.to_string(),
                include_turns: false,
            },
        })
        .await
    {
        Ok(response) => response.thread.name,
        Err(err) => {
            tracing::warn!(%err, "Failed to read fork parent metadata from app server");
            None
        }
    }
}

#[cfg(test)]
#[path = "background_tests.rs"]
mod tests;
