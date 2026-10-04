use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::TurnItemsView;
use codex_protocol::ThreadId;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::bail;
use std::collections::HashSet;
use std::time::Duration;

/// Supplies typed metadata and turn pages for saved transcript readers.
pub(crate) trait HistoryReader {
    fn metadata(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<ThreadReadResponse, TypedRequestError>> + Send;
    fn turns(
        &self,
        params: ThreadTurnsListParams,
    ) -> impl std::future::Future<Output = Result<ThreadTurnsListResponse, TypedRequestError>> + Send;
}

impl HistoryReader for AppServerRequestHandle {
    fn metadata(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<ThreadReadResponse, TypedRequestError>> + Send
    {
        self.request_typed(ClientRequest::ThreadRead {
            request_id: RequestId::String(uuid::Uuid::new_v4().to_string()),
            params: ThreadReadParams {
                thread_id: thread_id.to_string(),
                include_turns: false,
            },
        })
    }
    fn turns(
        &self,
        params: ThreadTurnsListParams,
    ) -> impl std::future::Future<Output = Result<ThreadTurnsListResponse, TypedRequestError>> + Send
    {
        self.request_typed(ClientRequest::ThreadTurnsList {
            request_id: RequestId::String(uuid::Uuid::new_v4().to_string()),
            params,
        })
    }
}

pub(crate) async fn read_thread_history(
    client: impl HistoryReader,
    thread_id: ThreadId,
) -> Result<Thread> {
    let response = tokio::time::timeout(Duration::from_secs(30), client.metadata(thread_id))
        .await
        .wrap_err("thread metadata request timed out")??;
    let mut thread = response.thread;
    thread.turns.clear();
    let mut cursor = None;
    let mut cursors = HashSet::new();
    for _ in 0..100 {
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            client.turns(ThreadTurnsListParams {
                thread_id: thread_id.to_string(),
                cursor,
                limit: Some(100),
                sort_direction: Some(SortDirection::Asc),
                items_view: Some(TurnItemsView::Full),
            }),
        )
        .await
        .wrap_err("thread history request timed out")??;
        if response.data.len() > 100 {
            bail!("history page exceeded its requested size");
        }
        thread.turns.extend(response.data);
        cursor = response.next_cursor;
        let Some(next) = &cursor else {
            return Ok(thread);
        };
        if !cursors.insert(next.clone()) {
            bail!("server returned a repeated history cursor");
        }
    }
    bail!("thread exceeds the 10,000-turn history limit")
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
