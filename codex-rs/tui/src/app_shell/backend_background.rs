use super::backend::AppShellTurnStart;
use super::backend::app_shell_request_id;
use super::queued_messages::QueueRpc;
use super::queued_messages::QueueRpcResponse;
use crate::app_server_session::turn_permissions_overrides;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadCompactStartParams;
use codex_app_server_protocol::ThreadCompactStartResponse;
use codex_app_server_protocol::ThreadDeleteParams;
use codex_app_server_protocol::ThreadDeleteResponse;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadQueueAddParams;
use codex_app_server_protocol::ThreadQueueAddResponse;
use codex_app_server_protocol::ThreadQueueDeleteParams;
use codex_app_server_protocol::ThreadQueueDeleteResponse;
use codex_app_server_protocol::ThreadQueueListParams;
use codex_app_server_protocol::ThreadQueueListResponse;
use codex_app_server_protocol::ThreadQueueReorderParams;
use codex_app_server_protocol::ThreadQueueReorderResponse;
use codex_app_server_protocol::ThreadQueueStartParams;
use codex_app_server_protocol::ThreadQueueStartResponse;
use codex_app_server_protocol::ThreadQueueUpdateParams;
use codex_app_server_protocol::ThreadQueueUpdateResponse;
use codex_app_server_protocol::ThreadSetNameParams;
use codex_app_server_protocol::ThreadSetNameResponse;
use codex_app_server_protocol::ThreadSourceKind;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_protocol::ThreadId;
use color_eyre::Result;

const DESCENDANT_PAGE_SIZE: u32 = 100;

pub(super) async fn delete_thread(
    request_handle: AppServerRequestHandle,
    thread_id: ThreadId,
) -> Result<()> {
    let _: ThreadDeleteResponse = request_handle
        .request_typed(ClientRequest::ThreadDelete {
            request_id: app_shell_request_id("app-shell-thread-delete"),
            params: ThreadDeleteParams {
                thread_id: thread_id.to_string(),
            },
        })
        .await?;
    Ok(())
}

pub(super) async fn compact_thread(
    request_handle: AppServerRequestHandle,
    thread_id: ThreadId,
) -> Result<()> {
    let _: ThreadCompactStartResponse = request_handle
        .request_typed(ClientRequest::ThreadCompactStart {
            request_id: app_shell_request_id("app-shell-thread-compact"),
            params: ThreadCompactStartParams {
                thread_id: thread_id.to_string(),
            },
        })
        .await?;
    Ok(())
}

pub(super) async fn count_descendants(
    request_handle: AppServerRequestHandle,
    thread_id: ThreadId,
) -> Result<usize> {
    let mut count = 0;
    for archived in [false, true] {
        let mut cursor = None;
        loop {
            let response: ThreadListResponse = request_handle
                .request_typed(ClientRequest::ThreadList {
                    request_id: app_shell_request_id("app-shell-descendants"),
                    params: ThreadListParams {
                        cursor,
                        limit: Some(DESCENDANT_PAGE_SIZE),
                        sort_key: None,
                        sort_direction: None,
                        model_providers: None,
                        source_kinds: Some(all_thread_source_kinds()),
                        archived: Some(archived),
                        cwd: None,
                        use_state_db_only: true,
                        search_term: None,
                        parent_thread_id: None,
                        ancestor_thread_id: Some(thread_id.to_string()),
                    },
                })
                .await?;
            count += response.data.len();
            let Some(next_cursor) = response.next_cursor else {
                break;
            };
            cursor = Some(next_cursor);
        }
    }
    Ok(count)
}

pub(super) async fn set_thread_name(
    request_handle: AppServerRequestHandle,
    thread_id: ThreadId,
    name: String,
) -> Result<()> {
    let _: ThreadSetNameResponse = request_handle
        .request_typed(ClientRequest::ThreadSetName {
            request_id: app_shell_request_id("app-shell-thread-name"),
            params: ThreadSetNameParams {
                thread_id: thread_id.to_string(),
                name,
            },
        })
        .await?;
    Ok(())
}

pub(super) async fn start_turn(
    request_handle: AppServerRequestHandle,
    params: AppShellTurnStart,
) -> Result<TurnStartResponse> {
    let (sandbox_policy, permissions) =
        turn_permissions_overrides(params.permissions_override, params.cwd.as_path());
    request_handle
        .request_typed(ClientRequest::TurnStart {
            request_id: app_shell_request_id("app-shell-turn-start"),
            params: TurnStartParams {
                thread_id: params.thread_id.to_string(),
                client_user_message_id: Some(params.client_user_message_id),
                input: params.items,
                responsesapi_client_metadata: None,
                additional_context: None,
                environments: None,
                cwd: Some(params.cwd),
                runtime_workspace_roots: Some(params.workspace_roots),
                approval_policy: Some(params.approval_policy),
                approvals_reviewer: Some(params.approvals_reviewer.into()),
                sandbox_policy,
                permissions,
                model: Some(params.model),
                service_tier: params.service_tier,
                effort: params.effort,
                summary: params.summary,
                personality: params.personality,
                output_schema: params.output_schema,
                collaboration_mode: params.collaboration_mode,
                multi_agent_mode: None,
            },
        })
        .await
        .map_err(Into::into)
}

pub(super) async fn list_queued_messages(
    request_handle: AppServerRequestHandle,
    thread_id: ThreadId,
) -> Result<Vec<codex_app_server_protocol::QueuedSubmission>> {
    let mut data = Vec::new();
    let mut cursor = None;
    loop {
        let response: ThreadQueueListResponse = request_handle
            .request_typed(ClientRequest::ThreadQueueList {
                request_id: app_shell_request_id("app-shell-thread-queue-list"),
                params: ThreadQueueListParams {
                    thread_id: thread_id.to_string(),
                    cursor,
                    limit: Some(100),
                },
            })
            .await?;
        data.extend(response.data);
        let Some(next_cursor) = response.next_cursor else {
            return Ok(data);
        };
        cursor = Some(next_cursor);
    }
}

pub(super) async fn mutate_queue(
    request_handle: AppServerRequestHandle,
    thread_id: ThreadId,
    rpc: QueueRpc,
) -> Result<QueueRpcResponse> {
    let thread_id = thread_id.to_string();
    match rpc {
        QueueRpc::Add {
            input,
            client_user_message_id,
        } => {
            let response: ThreadQueueAddResponse = request_handle
                .request_typed(ClientRequest::ThreadQueueAdd {
                    request_id: app_shell_request_id("app-shell-thread-queue-add"),
                    params: ThreadQueueAddParams {
                        thread_id,
                        input,
                        client_user_message_id,
                    },
                })
                .await?;
            Ok(QueueRpcResponse::Added(response.queued_submission))
        }
        QueueRpc::Update {
            queued_submission_id,
            input,
        } => {
            let response: ThreadQueueUpdateResponse = request_handle
                .request_typed(ClientRequest::ThreadQueueUpdate {
                    request_id: app_shell_request_id("app-shell-thread-queue-update"),
                    params: ThreadQueueUpdateParams {
                        thread_id,
                        queued_submission_id,
                        input,
                    },
                })
                .await?;
            Ok(QueueRpcResponse::Updated(response.queued_submission))
        }
        QueueRpc::Delete {
            queued_submission_id,
        } => {
            let response: ThreadQueueDeleteResponse = request_handle
                .request_typed(ClientRequest::ThreadQueueDelete {
                    request_id: app_shell_request_id("app-shell-thread-queue-delete"),
                    params: ThreadQueueDeleteParams {
                        thread_id,
                        queued_submission_id,
                    },
                })
                .await?;
            Ok(QueueRpcResponse::Deleted(response.deleted))
        }
        QueueRpc::Reorder {
            queued_submission_ids,
        } => {
            let _: ThreadQueueReorderResponse = request_handle
                .request_typed(ClientRequest::ThreadQueueReorder {
                    request_id: app_shell_request_id("app-shell-thread-queue-reorder"),
                    params: ThreadQueueReorderParams {
                        thread_id,
                        queued_submission_ids,
                    },
                })
                .await?;
            Ok(QueueRpcResponse::Reordered)
        }
        QueueRpc::Start => {
            let response: ThreadQueueStartResponse = request_handle
                .request_typed(ClientRequest::ThreadQueueStart {
                    request_id: app_shell_request_id("app-shell-thread-queue-start"),
                    params: ThreadQueueStartParams {
                        thread_id,
                        queued_submission_id: None,
                    },
                })
                .await?;
            Ok(QueueRpcResponse::Started(response.turn))
        }
    }
}

fn all_thread_source_kinds() -> Vec<ThreadSourceKind> {
    vec![
        ThreadSourceKind::Cli,
        ThreadSourceKind::VsCode,
        ThreadSourceKind::Custom,
        ThreadSourceKind::Exec,
        ThreadSourceKind::AppServer,
        ThreadSourceKind::SubAgent,
        ThreadSourceKind::SubAgentReview,
        ThreadSourceKind::SubAgentCompact,
        ThreadSourceKind::SubAgentThreadSpawn,
        ThreadSourceKind::SubAgentOther,
        ThreadSourceKind::Unknown,
    ]
}
