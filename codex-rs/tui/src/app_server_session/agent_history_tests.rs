use super::*;
use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SubAgentSource;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

#[test]
fn referenced_threads_are_deduplicated_across_agent_items() {
    let turns = vec![turn(vec![
        ThreadItem::CollabAgentToolCall {
            id: "spawn".to_string(),
            tool: CollabAgentTool::SpawnAgent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: "root".to_string(),
            receiver_thread_ids: vec!["agent-b".to_string(), "agent-a".to_string()],
            prompt: None,
            model: None,
            reasoning_effort: None,
            agents_states: HashMap::from([(
                "agent-a".to_string(),
                CollabAgentState {
                    status: codex_app_server_protocol::CollabAgentStatus::Running,
                    message: None,
                },
            )]),
        },
        activity("activity", "agent-b"),
    ])];

    assert_eq!(
        referenced_agent_thread_ids(&turns),
        vec!["agent-a".to_string(), "agent-b".to_string()]
    );
}

#[test]
fn pending_thread_queue_excludes_root_and_uses_a_separate_candidate_cap() {
    let root = "agent-00".to_string();
    let turns = vec![turn(
        (0..150)
            .map(|index| activity(&format!("activity-{index}"), &format!("agent-{index:02}")))
            .collect(),
    )];
    let mut seen = HashSet::from([root]);
    let mut pending = VecDeque::new();

    enqueue_referenced_agent_threads(&turns, &mut seen, &mut pending);

    assert_eq!(pending.len(), MAX_RESUMED_AGENT_THREAD_CANDIDATES);
    assert!(!pending.iter().any(|thread_id| thread_id == "agent-00"));
}

#[test]
fn candidate_batches_bound_concurrency_attempts_and_accepted_threads_separately() {
    let mut pending = (0..150)
        .map(|index| format!("agent-{index}"))
        .collect::<VecDeque<_>>();
    let mut attempted = MAX_RESUMED_AGENT_THREAD_CANDIDATES - 3;

    assert_eq!(
        take_candidate_batch(&mut pending, &mut attempted, /*accepted*/ 0),
        vec![
            "agent-0".to_string(),
            "agent-1".to_string(),
            "agent-2".to_string(),
        ]
    );
    assert_eq!(attempted, MAX_RESUMED_AGENT_THREAD_CANDIDATES);
    assert!(take_candidate_batch(&mut pending, &mut attempted, /*accepted*/ 0).is_empty());

    attempted = 0;
    assert_eq!(
        take_candidate_batch(
            &mut pending,
            &mut attempted,
            /*accepted*/ MAX_RESUMED_AGENT_THREADS - 1,
        )
        .len(),
        1
    );
}

#[test]
fn history_requests_use_bounded_descending_full_turn_pages() {
    assert_eq!(
        agent_thread_resume_params("agent".to_string()),
        ThreadResumeParams {
            thread_id: "agent".to_string(),
            exclude_turns: true,
            initial_turns_page: Some(ThreadResumeInitialTurnsPageParams {
                limit: Some(MAX_RESUMED_AGENT_TURNS),
                sort_direction: Some(SortDirection::Desc),
                items_view: Some(TurnItemsView::Full),
            }),
            ..Default::default()
        }
    );
    assert_eq!(
        agent_thread_turns_list_params("agent".to_string()),
        ThreadTurnsListParams {
            thread_id: "agent".to_string(),
            cursor: None,
            limit: Some(MAX_RESUMED_AGENT_TURNS),
            sort_direction: Some(SortDirection::Desc),
            items_view: Some(TurnItemsView::Full),
        }
    );
}

#[test]
fn descending_turn_pages_are_restored_to_chronological_order() {
    let mut older = turn(Vec::new());
    older.id = "older".to_string();
    let mut newer = turn(Vec::new());
    newer.id = "newer".to_string();

    assert_eq!(
        chronological_turns(vec![newer.clone(), older.clone()]),
        vec![older, newer]
    );
}

#[test]
fn activity_snapshots_bound_turn_items_and_text() {
    let root = thread_id("01900000-0000-7000-8000-000000000101");
    let child = thread_id("01900000-0000-7000-8000-000000000102");
    let mut thread = metadata_thread(child, &root.to_string(), Some(root), Some(root));
    thread.turns = (0..20)
        .map(|turn_index| {
            let mut turn = turn(
                (0..130)
                    .map(|item_index| ThreadItem::AgentMessage {
                        id: format!("message-{turn_index}-{item_index}"),
                        text: "x".repeat(2_000),
                        phase: None,
                        memory_citation: None,
                    })
                    .collect(),
            );
            turn.id = format!("turn-{turn_index}");
            turn
        })
        .collect();

    let snapshot = AgentHistorySnapshot::loaded(thread);

    assert_eq!(snapshot.turns.len(), 12);
    assert!(snapshot.turns.capacity() <= 12);
    assert!(snapshot.turns.iter().all(|turn| turn.items.len() == 64));
    assert!(
        snapshot
            .turns
            .iter()
            .all(|turn| turn.items.capacity() <= 64)
    );
    assert!(snapshot.turns.iter().flat_map(|turn| &turn.items).all(
        |item| matches!(item, ThreadItem::AgentMessage { text, .. } if text.chars().count() == 512 && text.capacity() <= 512)
    ));
}

#[tokio::test]
async fn cancelling_history_waits_for_the_final_subscription_tracker() {
    let (start_tx, start_rx) = oneshot::channel();
    let (_updates_tx, updates_rx) = mpsc::channel(1);
    let subscribed_thread_ids = Arc::new(Mutex::new(HashSet::new()));
    let worker_thread_ids = Arc::clone(&subscribed_thread_ids);
    let (inserted_tx, inserted_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        start_rx.await.expect("history task should start");
        worker_thread_ids
            .lock()
            .expect("subscription tracker should lock")
            .insert("agent-child".to_string());
        inserted_tx
            .send(())
            .expect("test should observe the tracked subscription");
        std::future::pending::<()>().await;
    });
    let mut task = AgentHistoryTask::new(
        start_tx,
        handle,
        updates_rx,
        Arc::clone(&subscribed_thread_ids),
    );
    task.start();
    inserted_rx
        .await
        .expect("worker should report the tracked subscription");

    assert_eq!(task.cancel().await, vec!["agent-child".to_string()]);
}

#[tokio::test]
async fn history_update_drain_honors_its_work_budget() {
    let (start_tx, start_rx) = oneshot::channel();
    let (updates_tx, updates_rx) = mpsc::channel(8);
    for index in 0..6 {
        updates_tx
            .try_send(AgentHistoryUpdate::Subscribed(format!("agent-{index}")))
            .expect("test update should fit");
    }
    let handle = tokio::spawn(async move {
        let _ = start_rx.await;
        std::future::pending::<()>().await;
    });
    let mut task = AgentHistoryTask::new(
        start_tx,
        handle,
        updates_rx,
        Arc::new(Mutex::new(HashSet::new())),
    );

    assert_eq!(task.drain_updates(/*limit*/ 2).len(), 2);
    assert!(!task.updates_empty());
    assert_eq!(task.drain_updates(/*limit*/ 8).len(), 4);
    assert!(task.updates_empty());
    task.cancel().await;
}

#[test]
fn lineage_accepts_current_and_legacy_thread_spawn_sessions() {
    let root = thread_id("01900000-0000-7000-8000-000000000001");
    let child = thread_id("01900000-0000-7000-8000-000000000002");
    let accepted = HashSet::from([root.to_string()]);
    let current = metadata_thread(
        child,
        &root.to_string(),
        Some(root),
        /*source_parent_id*/ None,
    );
    let legacy = metadata_thread(child, &child.to_string(), Some(root), Some(root));

    assert!(has_valid_agent_lineage(
        &current,
        &child.to_string(),
        &root.to_string(),
        &accepted
    ));
    assert!(has_valid_agent_lineage(
        &legacy,
        &child.to_string(),
        &root.to_string(),
        &accepted
    ));
}

#[test]
fn lineage_rejects_unaccepted_conflicting_and_non_spawn_parents() {
    let root = thread_id("01900000-0000-7000-8000-000000000011");
    let child = thread_id("01900000-0000-7000-8000-000000000012");
    let outsider = thread_id("01900000-0000-7000-8000-000000000013");
    let accepted = HashSet::from([root.to_string()]);
    let unaccepted = metadata_thread(child, &root.to_string(), Some(outsider), Some(outsider));
    let conflicting = metadata_thread(child, &child.to_string(), Some(root), Some(outsider));
    let non_spawn_legacy = metadata_thread(
        child,
        &child.to_string(),
        Some(root),
        /*source_parent_id*/ None,
    );
    let foreign_session = metadata_thread(child, &outsider.to_string(), Some(root), Some(root));
    let orphan_current = metadata_thread(
        child,
        &root.to_string(),
        /*parent_thread_id*/ None,
        /*source_parent_id*/ None,
    );

    assert_eq!(
        [
            unaccepted,
            conflicting,
            non_spawn_legacy,
            foreign_session,
            orphan_current,
        ]
        .map(|thread| {
            has_valid_agent_lineage(&thread, &child.to_string(), &root.to_string(), &accepted)
        }),
        [false, false, false, false, false]
    );
}

fn activity(id: &str, thread_id: &str) -> ThreadItem {
    ThreadItem::SubAgentActivity {
        id: id.to_string(),
        kind: SubAgentActivityKind::Started,
        agent_thread_id: thread_id.to_string(),
        agent_path: format!("/root/{thread_id}"),
    }
}

fn turn(items: Vec<ThreadItem>) -> Turn {
    Turn {
        id: "turn".to_string(),
        items,
        items_view: TurnItemsView::default(),
        status: TurnStatus::Completed,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

fn metadata_thread(
    id: ThreadId,
    session_id: &str,
    parent_thread_id: Option<ThreadId>,
    source_parent_id: Option<ThreadId>,
) -> Thread {
    Thread {
        id: id.to_string(),
        extra: None,
        session_id: session_id.to_string(),
        forked_from_id: None,
        parent_thread_id: parent_thread_id.map(|parent| parent.to_string()),
        preview: String::new(),
        ephemeral: false,
        history_mode: ThreadHistoryMode::Legacy,
        model_provider: "openai".to_string(),
        created_at: 1,
        updated_at: 2,
        recency_at: Some(2),
        status: ThreadStatus::NotLoaded,
        path: None,
        cwd: AbsolutePathBuf::from_absolute_path_checked("/workspace")
            .expect("workspace path should be absolute"),
        cli_version: "test".to_string(),
        source: source_parent_id.map_or(SessionSource::Cli, |parent_thread_id| {
            SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id,
                depth: 1,
                agent_path: None,
                agent_nickname: None,
                agent_role: None,
            })
        }),
        thread_source: None,
        agent_nickname: None,
        agent_role: None,
        git_info: None,
        name: None,
        turns: Vec::new(),
    }
}

fn thread_id(value: &str) -> ThreadId {
    ThreadId::from_string(value).expect("thread id should be valid")
}
