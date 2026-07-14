use super::AppServerSession;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadResumeInitialTurnsPageParams;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SubAgentSource;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tokio::time::timeout;
use uuid::Uuid;

#[path = "agent_history_join.rs"]
mod join;
#[path = "agent_history_snapshot.rs"]
mod snapshot;
#[path = "agent_history_task.rs"]
mod task;
use join::join_all;
pub(crate) use snapshot::AgentHistorySnapshot;
use snapshot::referenced_agent_thread_ids;
pub(crate) use task::AgentHistoryTask;
pub(crate) use task::AgentHistoryUpdate;

const MAX_RESUMED_AGENT_THREADS: usize = 64;
const MAX_RESUMED_AGENT_THREAD_CANDIDATES: usize = 128;
const MAX_RESUMED_AGENT_TURNS: u32 = 12;
const MAX_CONCURRENT_AGENT_THREAD_REQUESTS: usize = 8;
const MAX_AGENT_HISTORY_UPDATE_QUEUE: usize = 32;
const AGENT_HISTORY_LOAD_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 5);
const AGENT_THREAD_REQUEST_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 3);

impl AppServerSession {
    pub(super) fn spawn_resumed_agent_history(
        &self,
        root_thread_id: ThreadId,
        session_id: String,
        root_turns: &[Turn],
    ) -> AgentHistoryTask {
        let request_handle = self.request_handle();
        let referenced_thread_ids = referenced_agent_thread_ids(root_turns);
        let (start_tx, start_rx) = oneshot::channel();
        let (updates_tx, updates_rx) = mpsc::channel(MAX_AGENT_HISTORY_UPDATE_QUEUE);
        let subscribed_thread_ids = Arc::new(Mutex::new(HashSet::new()));
        let task_subscribed_thread_ids = Arc::clone(&subscribed_thread_ids);
        let handle = tokio::spawn(async move {
            if start_rx.await.is_err() {
                return;
            }
            load_resumed_agent_threads(
                request_handle,
                root_thread_id.to_string(),
                session_id,
                referenced_thread_ids,
                updates_tx,
                task_subscribed_thread_ids,
            )
            .await
        });
        AgentHistoryTask::new(start_tx, handle, updates_rx, subscribed_thread_ids)
    }
}

async fn load_resumed_agent_threads(
    request_handle: AppServerRequestHandle,
    root_thread_id: String,
    session_id: String,
    referenced_thread_ids: Vec<String>,
    updates_tx: mpsc::Sender<AgentHistoryUpdate>,
    subscribed_thread_ids: Arc<Mutex<HashSet<String>>>,
) {
    let mut seen = HashSet::from([root_thread_id.clone()]);
    let mut accepted_thread_ids = HashSet::from([root_thread_id]);
    let mut pending = VecDeque::new();
    enqueue_agent_thread_ids(referenced_thread_ids, &mut seen, &mut pending);

    let mut accepted = 0;
    let mut attempted = 0;
    let deadline = Instant::now() + AGENT_HISTORY_LOAD_TIMEOUT;
    while accepted < MAX_RESUMED_AGENT_THREADS {
        let batch = take_candidate_batch(&mut pending, &mut attempted, accepted);
        if batch.is_empty() {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let metadata = match timeout(
            remaining,
            read_agent_thread_metadata_batch(request_handle.clone(), batch),
        )
        .await
        {
            Ok(results) => results,
            Err(_) => {
                tracing::warn!(
                    timeout = ?AGENT_HISTORY_LOAD_TIMEOUT,
                    "resumed agent history hydration timed out"
                );
                break;
            }
        };
        let mut candidates = Vec::new();
        for (thread_id, result) in metadata {
            let thread = match result {
                Ok(thread) => thread,
                Err(err) => {
                    tracing::warn!(%thread_id, %err, "failed to read resumed agent metadata");
                    continue;
                }
            };
            if !has_valid_agent_lineage(&thread, &thread_id, &session_id, &accepted_thread_ids) {
                tracing::warn!(
                    %thread_id,
                    root_session_id = %session_id,
                    candidate_session_id = %thread.session_id,
                    "ignoring resumed thread outside the agent lineage"
                );
                seen.remove(&thread_id);
                continue;
            }
            if updates_tx
                .send(AgentHistoryUpdate::Discovered(
                    AgentHistorySnapshot::metadata(&thread),
                ))
                .await
                .is_err()
            {
                return;
            }
            candidates.push(thread);
        }
        if candidates.is_empty() {
            continue;
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        let results = match timeout(
            remaining,
            load_agent_thread_batch(
                request_handle.clone(),
                candidates,
                updates_tx.clone(),
                Arc::clone(&subscribed_thread_ids),
            ),
        )
        .await
        {
            Ok(results) => results,
            Err(_) => {
                tracing::warn!(
                    timeout = ?AGENT_HISTORY_LOAD_TIMEOUT,
                    "resumed agent history hydration timed out"
                );
                break;
            }
        };
        for (thread_id, thread) in results {
            if !has_valid_agent_lineage(&thread, &thread_id, &session_id, &accepted_thread_ids) {
                tracing::warn!(
                    %thread_id,
                    root_session_id = %session_id,
                    candidate_session_id = %thread.session_id,
                    "ignoring hydrated thread outside the agent lineage"
                );
                seen.remove(&thread_id);
                continue;
            }

            accepted_thread_ids.insert(thread.id.clone());
            enqueue_referenced_agent_threads(&thread.turns, &mut seen, &mut pending);
            accepted += 1;
            if updates_tx
                .send(AgentHistoryUpdate::Loaded(AgentHistorySnapshot::loaded(
                    thread,
                )))
                .await
                .is_err()
            {
                return;
            }
        }
    }
}

fn enqueue_referenced_agent_threads(
    turns: &[Turn],
    seen: &mut HashSet<String>,
    pending: &mut VecDeque<String>,
) {
    enqueue_agent_thread_ids(referenced_agent_thread_ids(turns), seen, pending);
}

fn enqueue_agent_thread_ids(
    thread_ids: Vec<String>,
    seen: &mut HashSet<String>,
    pending: &mut VecDeque<String>,
) {
    for thread_id in thread_ids {
        if seen.len().saturating_sub(/*rhs*/ 1) >= MAX_RESUMED_AGENT_THREAD_CANDIDATES {
            return;
        }
        if seen.insert(thread_id.clone()) {
            pending.push_back(thread_id);
        }
    }
}

fn take_candidate_batch(
    pending: &mut VecDeque<String>,
    attempted: &mut usize,
    accepted: usize,
) -> Vec<String> {
    let batch_size = pending
        .len()
        .min(MAX_CONCURRENT_AGENT_THREAD_REQUESTS)
        .min(MAX_RESUMED_AGENT_THREAD_CANDIDATES.saturating_sub(*attempted))
        .min(MAX_RESUMED_AGENT_THREADS.saturating_sub(accepted));
    *attempted = attempted.saturating_add(batch_size);
    pending.drain(..batch_size).collect()
}

async fn read_agent_thread_metadata_batch(
    request_handle: AppServerRequestHandle,
    thread_ids: Vec<String>,
) -> Vec<(String, Result<Thread, String>)> {
    join_all(
        thread_ids
            .into_iter()
            .map(|thread_id| {
                let request_handle = request_handle.clone();
                async move {
                    let result = read_agent_thread_metadata(request_handle, &thread_id).await;
                    (thread_id, result)
                }
            })
            .collect(),
    )
    .await
}

async fn read_agent_thread_metadata(
    request_handle: AppServerRequestHandle,
    thread_id: &str,
) -> Result<Thread, String> {
    let parsed_thread_id = ThreadId::from_string(thread_id).map_err(|err| err.to_string())?;
    let request = request_handle.request_typed::<ThreadReadResponse>(ClientRequest::ThreadRead {
        request_id: RequestId::String(format!("tui-agent-metadata-{}", Uuid::new_v4())),
        params: ThreadReadParams {
            thread_id: parsed_thread_id.to_string(),
            include_turns: false,
        },
    });
    match timeout(AGENT_THREAD_REQUEST_TIMEOUT, request).await {
        Ok(Ok(response)) => Ok(response.thread),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!(
            "thread/read timed out after {AGENT_THREAD_REQUEST_TIMEOUT:?}"
        )),
    }
}

async fn load_agent_thread_batch(
    request_handle: AppServerRequestHandle,
    threads: Vec<Thread>,
    updates_tx: mpsc::Sender<AgentHistoryUpdate>,
    subscribed_thread_ids: Arc<Mutex<HashSet<String>>>,
) -> Vec<(String, Thread)> {
    subscribed_thread_ids
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .extend(
            threads
                .iter()
                .filter(|thread| !matches!(&thread.status, ThreadStatus::NotLoaded))
                .map(|thread| thread.id.clone()),
        );
    join_all(
        threads
            .into_iter()
            .map(|thread| {
                let request_handle = request_handle.clone();
                let updates_tx = updates_tx.clone();
                async move {
                    let thread_id = thread.id.clone();
                    let result = load_agent_thread(request_handle, thread, updates_tx).await;
                    (thread_id, result)
                }
            })
            .collect(),
    )
    .await
}

async fn load_agent_thread(
    request_handle: AppServerRequestHandle,
    mut thread: Thread,
    updates_tx: mpsc::Sender<AgentHistoryUpdate>,
) -> Thread {
    match &thread.status {
        ThreadStatus::NotLoaded => {
            let request = request_handle.request_typed::<ThreadTurnsListResponse>(
                ClientRequest::ThreadTurnsList {
                    request_id: RequestId::String(format!("tui-agent-turns-{}", Uuid::new_v4())),
                    params: agent_thread_turns_list_params(thread.id.clone()),
                },
            );
            thread.turns = match timeout(AGENT_THREAD_REQUEST_TIMEOUT, request).await {
                Ok(Ok(response)) => chronological_turns(response.data),
                Ok(Err(err)) => {
                    tracing::warn!(
                        thread_id = %thread.id,
                        %err,
                        "failed to load bounded resumed agent history"
                    );
                    Vec::new()
                }
                Err(_) => {
                    tracing::warn!(
                        thread_id = %thread.id,
                        timeout = ?AGENT_THREAD_REQUEST_TIMEOUT,
                        "bounded resumed agent history request timed out"
                    );
                    Vec::new()
                }
            };
            return thread;
        }
        ThreadStatus::Idle | ThreadStatus::SystemError | ThreadStatus::Active { .. } => {}
    }

    let thread_id = thread.id.clone();
    if updates_tx
        .send(AgentHistoryUpdate::Subscribed(thread_id.clone()))
        .await
        .is_err()
    {
        return thread;
    }
    let request =
        request_handle.request_typed::<ThreadResumeResponse>(ClientRequest::ThreadResume {
            request_id: RequestId::String(format!("tui-agent-history-{}", Uuid::new_v4())),
            params: agent_thread_resume_params(thread_id.clone()),
        });
    match timeout(AGENT_THREAD_REQUEST_TIMEOUT, request).await {
        Ok(Ok(response)) => agent_thread_from_resume_response(response),
        Ok(Err(err)) => {
            tracing::warn!(%thread_id, %err, "failed to subscribe to resumed agent thread");
            thread
        }
        Err(_) => {
            tracing::warn!(
                %thread_id,
                timeout = ?AGENT_THREAD_REQUEST_TIMEOUT,
                "resumed agent thread subscription timed out"
            );
            thread
        }
    }
}

fn agent_thread_resume_params(thread_id: String) -> ThreadResumeParams {
    ThreadResumeParams {
        thread_id,
        exclude_turns: true,
        initial_turns_page: Some(ThreadResumeInitialTurnsPageParams {
            limit: Some(MAX_RESUMED_AGENT_TURNS),
            sort_direction: Some(SortDirection::Desc),
            items_view: Some(TurnItemsView::Full),
        }),
        ..Default::default()
    }
}

fn agent_thread_turns_list_params(thread_id: String) -> ThreadTurnsListParams {
    ThreadTurnsListParams {
        thread_id,
        cursor: None,
        limit: Some(MAX_RESUMED_AGENT_TURNS),
        sort_direction: Some(SortDirection::Desc),
        items_view: Some(TurnItemsView::Full),
    }
}

fn agent_thread_from_resume_response(response: ThreadResumeResponse) -> Thread {
    let mut thread = response.thread;
    thread.turns = chronological_turns(
        response
            .initial_turns_page
            .map_or_else(Vec::new, |page| page.data),
    );
    thread
}

fn chronological_turns(mut turns: Vec<Turn>) -> Vec<Turn> {
    turns.truncate(MAX_RESUMED_AGENT_TURNS as usize);
    turns.reverse();
    turns
}

fn has_valid_agent_lineage(
    thread: &Thread,
    expected_thread_id: &str,
    root_session_id: &str,
    accepted_thread_ids: &HashSet<String>,
) -> bool {
    if thread.id != expected_thread_id {
        return false;
    }
    let source_parent_id = match &thread.source {
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        }) => Some(parent_thread_id.to_string()),
        SessionSource::Cli
        | SessionSource::VsCode
        | SessionSource::Exec
        | SessionSource::AppServer
        | SessionSource::Custom(_)
        | SessionSource::SubAgent(
            SubAgentSource::Review
            | SubAgentSource::Compact
            | SubAgentSource::MemoryConsolidation
            | SubAgentSource::Other(_),
        )
        | SessionSource::Unknown => None,
    };
    if let (Some(parent_thread_id), Some(source_parent_id)) = (
        thread.parent_thread_id.as_deref(),
        source_parent_id.as_deref(),
    ) && parent_thread_id != source_parent_id
    {
        return false;
    }
    let parent_thread_id = source_parent_id
        .as_deref()
        .or(thread.parent_thread_id.as_deref());
    let Some(parent_thread_id) = parent_thread_id else {
        return false;
    };
    if !accepted_thread_ids.contains(parent_thread_id) {
        return false;
    }
    thread.session_id == root_session_id
        || (thread.session_id == thread.id && source_parent_id.is_some())
}

#[cfg(test)]
#[path = "agent_history_tests.rs"]
mod tests;
