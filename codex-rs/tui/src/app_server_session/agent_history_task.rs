use super::snapshot::AgentHistorySnapshot;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinError;
use tokio::task::JoinHandle;

pub(crate) struct AgentHistoryTask {
    start_tx: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
    updates_rx: mpsc::Receiver<AgentHistoryUpdate>,
    subscribed_thread_ids: Arc<Mutex<HashSet<String>>>,
}

#[derive(Debug)]
pub(crate) enum AgentHistoryUpdate {
    Discovered(AgentHistorySnapshot),
    Subscribed(String),
    Loaded(AgentHistorySnapshot),
}

impl AgentHistoryTask {
    pub(super) fn new(
        start_tx: oneshot::Sender<()>,
        handle: JoinHandle<()>,
        updates_rx: mpsc::Receiver<AgentHistoryUpdate>,
        subscribed_thread_ids: Arc<Mutex<HashSet<String>>>,
    ) -> Self {
        Self {
            start_tx: Some(start_tx),
            handle: Some(handle),
            updates_rx,
            subscribed_thread_ids,
        }
    }

    pub(crate) fn start(&mut self) {
        if let Some(start_tx) = self.start_tx.take() {
            let _ = start_tx.send(());
        }
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub(crate) fn drain_updates(&mut self, limit: usize) -> Vec<AgentHistoryUpdate> {
        std::iter::from_fn(|| self.updates_rx.try_recv().ok())
            .take(limit)
            .collect()
    }

    pub(crate) fn updates_empty(&self) -> bool {
        self.updates_rx.is_empty()
    }

    pub(crate) fn subscribed_thread_ids(&self) -> Vec<String> {
        self.subscribed_thread_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    pub(crate) fn is_subscribed(&self, thread_id: &str) -> bool {
        self.subscribed_thread_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(thread_id)
    }

    pub(crate) async fn finish(mut self) -> Result<(), JoinError> {
        self.start();
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        handle.await
    }

    pub(crate) async fn cancel(mut self) -> Vec<String> {
        if let Some(handle) = self.handle.take() {
            handle.abort();
            let _ = handle.await;
        }
        self.subscribed_thread_ids()
    }

    pub(crate) fn abort(&self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

impl fmt::Debug for AgentHistoryTask {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentHistoryTask")
            .field("started", &self.start_tx.is_none())
            .field("finished", &self.is_finished())
            .field("subscriptions", &self.subscribed_thread_ids().len())
            .finish()
    }
}

impl Drop for AgentHistoryTask {
    fn drop(&mut self) {
        self.abort();
    }
}
