use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadTimelineEntry;
use codex_app_server_protocol::ThreadTimelineListParams;
use codex_app_server_protocol::ThreadTimelineListResponse;
use codex_protocol::ThreadId;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use std::collections::HashSet;
use std::time::Duration;
use uuid::Uuid;

const MAX_TIMELINE_PAGES: usize = 4;
const TIMELINE_PAGE_SIZE: u32 = 500;

#[derive(Debug)]
pub(crate) struct ThreadTimeline {
    pub(crate) entries: Vec<ThreadTimelineEntry>,
    pub(crate) has_older: bool,
}

/// Reads canonical history through the same transport as ordinary thread requests.
pub(crate) trait TimelineReader {
    fn read_timeline(
        &self,
        params: ThreadTimelineListParams,
    ) -> impl std::future::Future<Output = Result<ThreadTimelineListResponse, TypedRequestError>> + Send;
}

impl TimelineReader for AppServerRequestHandle {
    fn read_timeline(
        &self,
        params: ThreadTimelineListParams,
    ) -> impl std::future::Future<Output = Result<ThreadTimelineListResponse, TypedRequestError>> + Send
    {
        self.request_typed(ClientRequest::ThreadTimelineList {
            request_id: RequestId::String(format!("better-timeline-{}", Uuid::new_v4())),
            params,
        })
    }
}

pub(crate) async fn load_thread_timeline(
    client: &impl TimelineReader,
    thread_id: ThreadId,
) -> Result<Option<ThreadTimeline>> {
    let mut pages = Vec::new();
    let mut cursor = None;
    let mut seen_cursors = HashSet::new();
    for _ in 0..MAX_TIMELINE_PAGES {
        let response = tokio::time::timeout(
            Duration::from_secs(/*secs*/ 30),
            client.read_timeline(ThreadTimelineListParams {
                thread_id: thread_id.to_string(),
                cursor: cursor.take(),
                limit: Some(TIMELINE_PAGE_SIZE),
            }),
        )
        .await?;
        let page = match response {
            Ok(page) => page,
            Err(TypedRequestError::Server { source, .. }) if source.code == -32601 => {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        if page.data.len() > TIMELINE_PAGE_SIZE as usize {
            return Err(eyre!("thread timeline exceeded its requested page size"));
        }
        pages.push(page.data);
        cursor = page.next_cursor;
        let Some(next) = &cursor else {
            break;
        };
        if !seen_cursors.insert(next.clone()) {
            return Err(eyre!("thread timeline returned a repeated cursor"));
        }
    }
    let entries: Vec<_> = pages.into_iter().rev().flatten().collect();
    if !entries
        .iter()
        .any(|entry| matches!(entry, ThreadTimelineEntry::Realtime { .. }))
    {
        return Ok(None);
    }
    Ok(Some(ThreadTimeline {
        entries,
        has_older: cursor.is_some(),
    }))
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
