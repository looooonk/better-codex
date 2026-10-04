use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadSortKey;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::bail;
use std::collections::HashSet;
use std::time::Duration;

const MAX_PAGES: usize = 100;
const PAGE_SIZE: u32 = 100;

/// Supplies native session metadata while keeping name resolution bounded across transports.
trait SessionNameReader {
    fn list(
        &self,
        params: ThreadListParams,
    ) -> impl std::future::Future<Output = Result<ThreadListResponse, TypedRequestError>> + Send;
    fn read(
        &self,
        thread_id: String,
    ) -> impl std::future::Future<Output = Result<Thread, TypedRequestError>> + Send;
}

impl SessionNameReader for AppServerRequestHandle {
    fn list(
        &self,
        params: ThreadListParams,
    ) -> impl std::future::Future<Output = Result<ThreadListResponse, TypedRequestError>> + Send
    {
        self.request_typed(ClientRequest::ThreadList {
            request_id: RequestId::String("better-codex-session-name-list".to_string()),
            params,
        })
    }

    async fn read(&self, thread_id: String) -> Result<Thread, TypedRequestError> {
        let response: ThreadReadResponse = self
            .request_typed(ClientRequest::ThreadRead {
                request_id: RequestId::String("better-codex-session-name-read".to_string()),
                params: ThreadReadParams {
                    thread_id,
                    include_turns: false,
                },
            })
            .await?;
        Ok(response.thread)
    }
}

pub(crate) async fn lookup(
    client: AppServerRequestHandle,
    name: &str,
    archived: bool,
) -> Result<Option<Thread>> {
    lookup_with_reader(&client, name, archived).await
}

fn display_label(thread: &Thread) -> &str {
    thread
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| thread.preview.trim())
}

async fn lookup_with_reader(
    client: &impl SessionNameReader,
    name: &str,
    archived: bool,
) -> Result<Option<Thread>> {
    if name.trim().is_empty() {
        return Ok(None);
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(/*secs*/ 30);
    let mut cursor = None;
    let mut cursors = HashSet::new();
    let mut matched: Option<Thread> = None;
    for _ in 0..MAX_PAGES {
        let response = tokio::time::timeout_at(
            deadline,
            client.list(ThreadListParams {
                cursor,
                limit: Some(PAGE_SIZE),
                sort_key: Some(ThreadSortKey::UpdatedAt),
                source_kinds: Some(crate::resume_source_kinds(
                    /*include_non_interactive*/ true,
                )),
                archived: Some(archived),
                search_term: None,
                sort_direction: None,
                model_providers: None,
                originators: None,
                section_id: None,
                project_id: None,
                cwd: None,
                use_state_db_only: false,
                parent_thread_id: None,
                ancestor_thread_id: None,
            }),
        )
        .await
        .wrap_err("session name lookup timed out; use the session UUID")?
        .wrap_err("failed to list sessions while resolving session name")?;
        if response.data.len() > PAGE_SIZE as usize {
            bail!("server exceeded the session page size; use the session UUID");
        }
        for thread in response
            .data
            .into_iter()
            .filter(|thread| display_label(thread) == name)
        {
            let current = tokio::time::timeout_at(deadline, client.read(thread.id.clone()))
                .await
                .wrap_err("session name lookup timed out; use the session UUID")?
                .wrap_err("failed to verify session name; use the session UUID")?;
            if current.id != thread.id || display_label(&current) != name {
                continue;
            }
            if let Some(previous) = &matched
                && previous.id != current.id
            {
                bail!(
                    "multiple sessions match '{name}' ({} and {}); use the session UUID",
                    previous.id,
                    current.id
                );
            }
            matched = Some(current);
        }
        let Some(next) = response.next_cursor else {
            // Older servers can skip equal timestamps at a page boundary.
            if let Some(thread) = &matched
                && !cursors.is_empty()
            {
                bail!(
                    "cannot verify a unique session name across server pages; matching UUID: {}. Use it only if this is the session you want",
                    thread.id
                );
            }
            return Ok(matched);
        };
        if !cursors.insert(next.clone()) {
            bail!("server repeated a session cursor; use the session UUID");
        }
        cursor = Some(next);
    }
    bail!("session name lookup exceeded 10,000 results; use the session UUID")
}

#[cfg(test)]
#[path = "session_name_lookup_tests.rs"]
mod tests;
