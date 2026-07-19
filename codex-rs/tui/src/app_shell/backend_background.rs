use super::backend::AppShellTurnStart;
use super::backend::app_shell_request_id;
use crate::app_server_session::turn_permissions_overrides;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadDeleteParams;
use codex_app_server_protocol::ThreadDeleteResponse;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
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
                client_user_message_id: None,
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
