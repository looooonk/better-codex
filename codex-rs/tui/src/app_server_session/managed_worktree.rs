use super::*;
use crate::managed_worktree::WorktreeMode;
use codex_app_server_protocol::ThreadBackgroundTerminalsListParams;
use codex_app_server_protocol::ThreadBackgroundTerminalsListResponse;
use codex_app_server_protocol::ThreadQueueListParams;
use codex_app_server_protocol::ThreadQueueListResponse;

pub(crate) async fn check_worktree_source(
    client: &AppServerRequestHandle,
    source_thread_id: ThreadId,
    thread_ids: &[ThreadId],
) -> Result<()> {
    for thread_id in thread_ids {
        let thread: ThreadReadResponse = client
            .request_typed(ClientRequest::ThreadRead {
                request_id: RequestId::String(format!("worktree-status-{}", Uuid::new_v4())),
                params: ThreadReadParams {
                    thread_id: thread_id.to_string(),
                    include_turns: false,
                },
            })
            .await?;
        if *thread_id == source_thread_id
            && (thread.thread.ephemeral || thread.thread.can_accept_direct_input == Some(false))
        {
            color_eyre::eyre::bail!(
                "Managed worktrees require a saved session that accepts direct input"
            );
        }
        if matches!(
            thread.thread.status,
            ThreadStatus::Active { .. } | ThreadStatus::SystemError
        ) {
            color_eyre::eyre::bail!("Creating a worktree requires idle sessions and agents");
        }
        let terminals: ThreadBackgroundTerminalsListResponse = client
            .request_typed(ClientRequest::ThreadBackgroundTerminalsList {
                request_id: RequestId::String(format!("worktree-terminals-{}", Uuid::new_v4())),
                params: ThreadBackgroundTerminalsListParams {
                    thread_id: thread_id.to_string(),
                    cursor: None,
                    limit: Some(1),
                },
            })
            .await?;
        if !terminals.data.is_empty() || terminals.next_cursor.is_some() {
            color_eyre::eyre::bail!("Stop background terminals before creating a worktree");
        }
        let queue: ThreadQueueListResponse = client
            .request_typed(ClientRequest::ThreadQueueList {
                request_id: RequestId::String(format!("worktree-queue-{}", Uuid::new_v4())),
                params: ThreadQueueListParams {
                    thread_id: thread_id.to_string(),
                    cursor: None,
                    limit: Some(1),
                },
            })
            .await?;
        if !queue.data.is_empty() || queue.next_cursor.is_some() {
            color_eyre::eyre::bail!("Clear queued input before creating a worktree");
        }
    }
    Ok(())
}

pub(crate) async fn start_managed_worktree_thread(
    client: AppServerRequestHandle,
    source_thread_id: ThreadId,
    config: Config,
    mode: WorktreeMode,
) -> Result<AppServerStartedThread> {
    match mode {
        WorktreeMode::New => {
            start_thread_with_request_handle(
                client,
                config,
                ThreadParamsMode::Embedded,
                /*remote_cwd_override*/ None,
            )
            .await
        }
        WorktreeMode::Fork => {
            let response: ThreadForkResponse = client
                .request_typed(ClientRequest::ThreadFork {
                    request_id: RequestId::String(format!("worktree-fork-{}", Uuid::new_v4())),
                    params: ThreadForkParams {
                        defer_goal_continuation: true,
                        ..thread_fork_params_from_config(
                            config.clone(),
                            source_thread_id,
                            ThreadParamsMode::Embedded,
                            /*remote_cwd_override*/ None,
                        )
                    },
                })
                .await?;
            let session_id = response.thread.session_id.clone();
            let mut started =
                started_thread_from_fork_response(response, &config, ThreadParamsMode::Embedded)
                    .await?;
            started.timeline = load_thread_timeline(&client, started.session.thread_id).await?;
            started.agent_history_task = spawn_resumed_agent_history(
                client,
                started.session.thread_id,
                session_id,
                &started.turns,
            );
            Ok(started)
        }
    }
}
