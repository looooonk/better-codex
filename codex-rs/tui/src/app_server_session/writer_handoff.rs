//! Wait for another local Codex runtime to finish releasing an idle chat.
use super::bootstrap_request_error;
use codex_app_server_client::TypedRequestError;
use codex_protocol::ThreadId;
use codex_rollout::WriterLockCoordinator;
use color_eyre::Result;
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub(crate) const WAIT_MESSAGE: &str =
    "Waiting for another Codex session to release this chat. Esc cancels.";
// The standard daemon waits 60s after its last subscriber disconnects before unloading.
const MAX_WAIT: Duration = Duration::from_secs(75);
const POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Default)]
pub(crate) struct WriterHandoff {
    waiting: watch::Sender<Option<CancellationToken>>,
}

#[derive(Debug, thiserror::Error)]
#[error("Chat switch canceled.")]
pub(crate) struct Canceled;

impl WriterHandoff {
    pub(crate) fn subscribe(&self) -> watch::Receiver<Option<CancellationToken>> {
        self.waiting.subscribe()
    }

    pub(super) async fn resume<T, F: Future<Output = Result<T, TypedRequestError>>>(
        &self,
        codex_home: &Path,
        thread_id: ThreadId,
        mut send: impl FnMut() -> F,
    ) -> Result<T> {
        let busy_message = format!("thread {thread_id} already has an active writer");
        let writer_busy = |error: &TypedRequestError| {
            matches!(error,
            TypedRequestError::Server { source, .. }
                if source.code == super::JSONRPC_INVALID_REQUEST && source.message == busy_message)
        };
        let first_error = match send().await {
            Err(error) if writer_busy(&error) => error,
            other => {
                return other
                    .map_err(|err| bootstrap_request_error("thread/resume failed in TUI", err));
            }
        };
        let deadline = Instant::now() + MAX_WAIT;
        let cancel = CancellationToken::new();
        self.waiting.send_replace(Some(cancel.clone()));
        let locks = Arc::new(WriterLockCoordinator::new(codex_home));
        let result = loop {
            tokio::select! {
                biased;
                () = cancel.cancelled() => break Err(Canceled.into()),
                () = tokio::time::sleep_until(deadline) => break Err(bootstrap_request_error("Timed out waiting for the other Codex session to release this chat", first_error)),
                () = tokio::time::sleep(POLL_INTERVAL) => {}
            }
            // Poll a read-only lock snapshot. A large history need only be loaded again once
            // its writer appears available. The actual resume always arbitrates ownership.
            let locks = Arc::clone(&locks);
            let owners =
                tokio::task::spawn_blocking(move || locks.active_writers(&[thread_id])).await;
            if matches!(owners, Ok(Ok(threads)) if !threads.is_empty()) {
                continue;
            }
            if cancel.is_cancelled() {
                break Err(Canceled.into());
            }
            match send().await {
                Err(error) if writer_busy(&error) => continue,
                other => {
                    break other
                        .map_err(|err| bootstrap_request_error("thread/resume failed in TUI", err));
                }
            }
        };
        self.waiting.send_replace(None);
        result
    }
}

#[cfg(test)]
#[path = "writer_handoff_tests.rs"]
mod tests;
