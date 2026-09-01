use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;

use anyhow::Result;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_core::ThreadManager;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ModeKind;
use codex_protocol::models::BaseInstructions;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::ThreadMemoryMode;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::UserMessageEvent;
use codex_rollout::RolloutItem;
use codex_state::BlockedSubmissionRetryPolicy;
use codex_state::QueueClaimResult;
use codex_state::QueueTerminalDisposition;
use codex_state::QueuedSubmissionRecord;
use codex_state::QueuedSubmissionState;
use codex_state::QueuedSubmissionTerminalStatus;
use codex_state::StateRuntime;
use codex_state::ThreadQueuePauseReason;
use codex_thread_store::AppendThreadItemsParams;
use codex_thread_store::CreateThreadParams;
use codex_thread_store::InMemoryThreadStore;
use codex_thread_store::ThreadPersistenceMetadata;
use codex_thread_store::ThreadStore;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::time::timeout;
use uuid::Uuid;

use super::*;
use crate::outgoing_message::OutgoingMessageSender;
use crate::request_serialization::RequestSerializationQueues;
use crate::thread_state::ThreadStateManager;
use crate::thread_status::ThreadWatchManager;

#[tokio::test]
async fn failed_ambiguous_reconciliation_blocks_visible_owner_without_waiting_for_terminal()
-> Result<()> {
    let codex_home = TempDir::new()?;
    let state = StateRuntime::init(codex_home.path().to_path_buf(), "test".to_string()).await?;
    let thread_id = ThreadId::new();
    let queued = state
        .enqueue_queued_submission(thread_id, "[]", "client-1")
        .await?;
    let turn_id = "turn-1";
    let active = match state
        .claim_queued_submission(thread_id, Some(&queued.id), turn_id)
        .await?
    {
        QueueClaimResult::Claimed(record) => record,
        claim => anyhow::bail!("unexpected queue claim: {claim:?}"),
    };

    let thread_state_manager = ThreadStateManager::new();
    thread_state_manager
        .thread_state(thread_id)
        .await
        .lock()
        .await
        .mark_queued_turn_awaiting_terminal(turn_id.to_string());
    let service = queue_service(state.clone(), thread_state_manager.clone());

    rpc_result(
        timeout(
            Duration::from_secs(1),
            service.finalize_ambiguous_start_recovery(
                thread_id,
                active,
                turn_id.to_string(),
                Err(internal_error("injected durable recovery failure")),
            ),
        )
        .await?,
    )?;

    assert!(
        !thread_state_manager
            .thread_state(thread_id)
            .await
            .lock()
            .await
            .terminal_event_pending(turn_id)
    );
    assert_eq!(
        state
            .list_queued_submissions(thread_id, /*offset*/ 0, /*limit*/ 10)
            .await?,
        vec![QueuedSubmissionRecord {
            state: QueuedSubmissionState::Pending,
            turn_id: None,
            ..queued.clone()
        }]
    );
    assert_eq!(
        state.thread_queue_pause_reason(thread_id).await?,
        Some(ThreadQueuePauseReason::Interrupted)
    );
    assert_eq!(
        state
            .claim_queued_submission_and_resume(thread_id, /*item_id*/ None, "turn-2")
            .await?
            .claim,
        QueueClaimResult::Blocked {
            owner_id: queued.id,
            retry_policy: BlockedSubmissionRetryPolicy::Forbidden,
        }
    );
    Ok(())
}

#[tokio::test]
async fn duplicate_terminal_event_does_not_select_a_terminal_tombstone() -> Result<()> {
    let codex_home = TempDir::new()?;
    let state = StateRuntime::init(codex_home.path().to_path_buf(), "test".to_string()).await?;
    let thread_id = ThreadId::new();
    let old = state
        .enqueue_queued_submission(thread_id, "[]", "client-old")
        .await?;
    assert!(matches!(
        state
            .claim_queued_submission(thread_id, Some(&old.id), "turn-old")
            .await?,
        QueueClaimResult::Claimed(_)
    ));
    assert!(
        state
            .finish_queued_submission(
                thread_id,
                "turn-old",
                QueuedSubmissionTerminalStatus::Completed,
                QueueTerminalDisposition::Continue,
            )
            .await?
    );
    let follower = state
        .enqueue_queued_submission(thread_id, "[]", "client-follower")
        .await?;
    assert!(matches!(
        state
            .claim_queued_submission(thread_id, Some(&follower.id), "turn-follower")
            .await?,
        QueueClaimResult::Claimed(_)
    ));
    let service = queue_service(state, ThreadStateManager::new());

    assert_eq!(
        rpc_result(
            service
                .active_record_for_observed_turn(thread_id, "turn-old")
                .await,
        )?,
        None
    );
    assert_eq!(
        rpc_result(
            service
                .active_record_for_observed_turn(thread_id, "turn-follower")
                .await,
        )?
        .map(|record| record.id),
        Some(follower.id)
    );
    Ok(())
}

#[tokio::test]
async fn terminal_only_recovery_waits_for_exact_terminal_history() -> Result<()> {
    let codex_home = TempDir::new()?;
    let state = StateRuntime::init(codex_home.path().to_path_buf(), "test".to_string()).await?;
    let thread_id = ThreadId::new();
    let turn_id = "turn-terminal";
    let queued = state
        .enqueue_queued_submission(thread_id, "retained payload", "client-terminal")
        .await?;
    assert!(matches!(
        state
            .claim_queued_submission(thread_id, Some(&queued.id), turn_id)
            .await?,
        QueueClaimResult::Claimed(_)
    ));
    assert!(
        state
            .mark_queued_submission_inflight(thread_id, turn_id)
            .await?
    );
    let store = Arc::new(InMemoryThreadStore::default());
    seed_store_thread(&store, thread_id).await?;
    store
        .append_items(AppendThreadItemsParams {
            thread_id,
            items: vec![turn_started(turn_id), user_message("client-terminal")],
        })
        .await?;
    let service = queue_service_with_store(state.clone(), ThreadStateManager::new(), store.clone());

    assert_eq!(
        rpc_result(service.recover_terminal_only(thread_id).await)?,
        None
    );
    assert_eq!(
        state
            .active_queued_submission(thread_id)
            .await?
            .map(|record| record.id),
        Some(queued.id.clone())
    );

    store
        .append_items(AppendThreadItemsParams {
            thread_id,
            items: vec![turn_completed(turn_id)],
        })
        .await?;
    assert_eq!(
        rpc_result(service.recover_terminal_only(thread_id).await)?,
        Some(QueueTerminalDisposition::Continue)
    );
    assert_eq!(state.active_queued_submission(thread_id).await?, None);
    assert_eq!(
        state
            .queued_submission(thread_id, &queued.id)
            .await?
            .expect("terminal queue tombstone"),
        QueuedSubmissionRecord {
            payload: "[]".to_string(),
            state: QueuedSubmissionState::Terminal,
            turn_id: Some(turn_id.to_string()),
            terminal_status: Some(QueuedSubmissionTerminalStatus::Completed),
            ..queued
        }
    );
    Ok(())
}

#[tokio::test]
async fn terminal_without_input_becomes_a_visible_retryable_owner() -> Result<()> {
    let codex_home = TempDir::new()?;
    let state = StateRuntime::init(codex_home.path().to_path_buf(), "test".to_string()).await?;
    let thread_id = ThreadId::new();
    let turn_id = "turn-without-input";
    let queued = state
        .enqueue_queued_submission(thread_id, "retained payload", "client-missing")
        .await?;
    assert!(matches!(
        state
            .claim_queued_submission(thread_id, Some(&queued.id), turn_id)
            .await?,
        QueueClaimResult::Claimed(_)
    ));
    let store = Arc::new(InMemoryThreadStore::default());
    seed_store_thread(&store, thread_id).await?;
    store
        .append_items(AppendThreadItemsParams {
            thread_id,
            items: vec![turn_started(turn_id), turn_completed(turn_id)],
        })
        .await?;
    let service = queue_service_with_store(state.clone(), ThreadStateManager::new(), store);

    assert_eq!(
        rpc_result(service.recover_terminal_only(thread_id).await)?,
        Some(QueueTerminalDisposition::Pause(
            ThreadQueuePauseReason::Interrupted,
        ))
    );
    assert_eq!(
        state
            .list_queued_submissions(thread_id, /*offset*/ 0, /*limit*/ 10)
            .await?,
        vec![QueuedSubmissionRecord {
            state: QueuedSubmissionState::Pending,
            turn_id: None,
            ..queued.clone()
        }]
    );
    assert_eq!(
        state
            .claim_queued_submission_and_resume(thread_id, /*item_id*/ None, "turn-retry")
            .await?
            .claim,
        QueueClaimResult::Claimed(QueuedSubmissionRecord {
            state: QueuedSubmissionState::Starting,
            turn_id: Some("turn-retry".to_string()),
            ..queued
        })
    );
    Ok(())
}

fn queue_service(
    state: Arc<StateRuntime>,
    thread_state_manager: ThreadStateManager,
) -> ThreadQueueService {
    queue_service_with_store(
        state,
        thread_state_manager,
        Arc::new(InMemoryThreadStore::default()),
    )
}

fn rpc_result<T>(result: std::result::Result<T, JSONRPCErrorError>) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!(error.message))
}

fn queue_service_with_store(
    state: Arc<StateRuntime>,
    thread_state_manager: ThreadStateManager,
    thread_store: Arc<dyn ThreadStore>,
) -> ThreadQueueService {
    let (outgoing_tx, _outgoing_rx) = mpsc::channel(4);
    let outgoing = Arc::new(OutgoingMessageSender::new(
        outgoing_tx,
        codex_analytics::AnalyticsEventsClient::disabled(),
    ));
    ThreadQueueService::new(
        Weak::<ThreadManager>::new(),
        thread_store,
        outgoing.clone(),
        Some(state),
        RequestSerializationQueues::default(),
        thread_state_manager,
        ThreadWatchManager::new_with_outgoing(outgoing),
    )
}

async fn seed_store_thread(store: &InMemoryThreadStore, thread_id: ThreadId) -> Result<()> {
    store
        .create_thread(CreateThreadParams {
            session_id: thread_id.into(),
            thread_id,
            extra_config: None,
            forked_from_id: None,
            parent_thread_id: None,
            source: SessionSource::Exec,
            thread_source: None,
            originator: "test".to_string(),
            base_instructions: BaseInstructions::default(),
            dynamic_tools: Vec::new(),
            selected_capability_roots: Vec::new(),
            multi_agent_version: None,
            history_mode: ThreadHistoryMode::Legacy,
            subagent_history_start_ordinal: None,
            initial_window_id: Uuid::now_v7().to_string(),
            metadata: ThreadPersistenceMetadata {
                cwd: None,
                model_provider: "test".to_string(),
                memory_mode: ThreadMemoryMode::Disabled,
            },
        })
        .await?;
    Ok(())
}

fn turn_started(turn_id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: turn_id.to_string(),
        trace_id: None,
        started_at: None,
        model_context_window: None,
        collaboration_mode_kind: ModeKind::Default,
    }))
}

fn user_message(client_id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
        client_id: Some(client_id.to_string()),
        message: "queued".to_string(),
        ..Default::default()
    }))
}

fn turn_completed(turn_id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
        turn_id: turn_id.to_string(),
        last_agent_message: None,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
        time_to_first_token_ms: None,
    }))
}
