use super::backend::app_shell_request_id;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadUnsubscribeParams;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_protocol::ThreadId;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio::time::timeout;

const MAX_CONCURRENT_THREAD_UNSUBSCRIBES: usize = 8;
const THREAD_UNSUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 3);
const THREAD_UNSUBSCRIBE_CLEANUP_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 5);

pub(super) async fn unsubscribe_threads(
    request_handle: AppServerRequestHandle,
    thread_ids: Vec<ThreadId>,
) {
    let deadline = Instant::now() + THREAD_UNSUBSCRIBE_CLEANUP_TIMEOUT;
    for (batch_index, batch) in thread_ids
        .chunks(MAX_CONCURRENT_THREAD_UNSUBSCRIBES)
        .enumerate()
    {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            tracing::warn!(
                remaining = thread_ids
                    .len()
                    .saturating_sub(batch_index * MAX_CONCURRENT_THREAD_UNSUBSCRIBES),
                timeout = ?THREAD_UNSUBSCRIBE_CLEANUP_TIMEOUT,
                "thread subscription cleanup timed out"
            );
            break;
        }
        let request_timeout = remaining.min(THREAD_UNSUBSCRIBE_TIMEOUT);
        let mut requests = JoinSet::new();
        for thread_id in batch.iter().copied() {
            let request_handle = request_handle.clone();
            requests.spawn(async move {
                let request = request_handle.request_typed::<ThreadUnsubscribeResponse>(
                    ClientRequest::ThreadUnsubscribe {
                        request_id: app_shell_request_id("app-shell-unsubscribe"),
                        params: ThreadUnsubscribeParams {
                            thread_id: thread_id.to_string(),
                        },
                    },
                );
                (thread_id, timeout(request_timeout, request).await)
            });
        }
        while let Some(result) = requests.join_next().await {
            match result {
                Ok((_, Ok(Ok(_)))) => {}
                Ok((thread_id, Ok(Err(err)))) => {
                    tracing::warn!(%thread_id, %err, "failed to unsubscribe replaced session thread");
                }
                Ok((thread_id, Err(_))) => {
                    tracing::warn!(
                        %thread_id,
                        timeout = ?request_timeout,
                        "replaced session unsubscribe timed out"
                    );
                }
                Err(err) => tracing::warn!(%err, "replaced session unsubscribe task failed"),
            }
        }
    }
}
