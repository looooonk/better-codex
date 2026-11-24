use std::collections::HashMap;

use super::backend::app_shell_request_id;
use super::project_hub::Chat;
use super::project_hub::Group;
use super::project_hub::PAGE_SIZE;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectListParams;
use codex_app_server_protocol::ProjectListResponse;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use color_eyre::Result;

pub(super) async fn load(
    handle: AppServerRequestHandle,
    params: ThreadListParams,
    mut pages: HashMap<String, Vec<Option<String>>>,
) -> Result<Vec<Group>> {
    let mut projects = Vec::new();
    let mut cursor = None;
    loop {
        let page: ProjectListResponse = handle
            .request_typed(ClientRequest::ProjectList {
                request_id: app_shell_request_id("projects"),
                params: ProjectListParams {
                    cursor: cursor.clone(),
                    limit: Some(100),
                    sort_key: None,
                    sort_direction: None,
                },
            })
            .await?;
        projects.extend(page.data);
        if page.next_cursor.is_none() {
            break;
        }
        color_eyre::eyre::ensure!(
            page.next_cursor != cursor,
            "project listing cursor repeated"
        );
        cursor = page.next_cursor;
    }
    let mut groups = Vec::with_capacity(projects.len());
    for project in projects {
        let stack = pages.remove(&project.id).unwrap_or_else(|| vec![None]);
        let mut params = params.clone();
        params.cursor = stack.last().cloned().flatten();
        params.limit = Some(PAGE_SIZE);
        params.project_id = Some(Some(project.id.clone()));
        params.archived = Some(false);
        params.search_term = None;
        params.use_state_db_only = true;
        let page: ThreadListResponse = handle
            .request_typed(ClientRequest::ThreadList {
                request_id: app_shell_request_id("project-chats"),
                params,
            })
            .await?;
        groups.push(Group {
            project,
            chats: page
                .data
                .into_iter()
                .filter_map(Chat::from_thread)
                .collect(),
            next: page.next_cursor,
            pages: stack,
        });
    }
    Ok(groups)
}
