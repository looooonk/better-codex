use std::path::Path;

use codex_protocol::ThreadId;
use codex_protocol::models::BaseInstructions;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::ThreadMemoryMode;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_rollout::RolloutItem;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::LocalThreadStore;
use super::publish_replacement;
use super::replacement_write_error;
use super::super::test_support::test_config;
use crate::AppendThreadItemsParams;
use crate::CreateThreadParams;
use crate::ListTurnsParams;
use crate::ResumeThreadParams;
use crate::RevertThreadParams;
use crate::SortDirection;
use crate::StoredTurnItemsView;
use crate::ThreadPersistenceMetadata;
use crate::ThreadStore;
use crate::ThreadStoreError;

#[tokio::test]
async fn revert_keeps_thread_identity_and_hides_suffix_across_compressed_lineage() {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let state_db = codex_state::StateRuntime::init(
        config.sqlite_home.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .expect("initialize state database");
    let store = LocalThreadStore::new(config, Some(state_db.clone()));
    let thread_id = ThreadId::new();
    create_paginated_thread(&store, thread_id).await;
    store
        .append_items(AppendThreadItemsParams {
            thread_id,
            items: vec![
                turn_started("turn-1"),
                turn_completed("turn-1"),
                turn_started("turn-2"),
                turn_completed("turn-2"),
            ],
        })
        .await
        .expect("append turns");
    let original_path = store
        .live_rollout_path(thread_id)
        .await
        .expect("source rollout path");
    store
        .shutdown_thread(thread_id)
        .await
        .expect("close source writer");
    reconcile_rollout(&state_db, original_path.as_path()).await;
    compress_rollout(original_path.as_path());

    store
        .revert_thread(RevertThreadParams {
            thread_id,
            before_turn_id: "turn-2".to_string(),
        })
        .await
        .expect("revert before second turn");
    let first_replacement_path = selected_path(&state_db, thread_id).await;
    assert_ne!(first_replacement_path, original_path);
    assert_ne!(
        codex_rollout::rollout_id_from_path(first_replacement_path.as_path()),
        Some(thread_id)
    );
    let replacement_meta = codex_rollout::read_session_meta_line(first_replacement_path.as_path())
        .await
        .expect("read replacement metadata")
        .meta;
    assert_eq!(replacement_meta.id, thread_id);
    assert_eq!(turn_ids(&store, thread_id).await, vec!["turn-1"]);

    store
        .revert_thread(RevertThreadParams {
            thread_id,
            before_turn_id: "turn-1".to_string(),
        })
        .await
        .expect("revert before first turn");
    let second_replacement_path = selected_path(&state_db, thread_id).await;
    assert_ne!(second_replacement_path, first_replacement_path);
    assert!(original_path.exists());
    assert!(first_replacement_path.exists());
    assert_eq!(turn_ids(&store, thread_id).await, Vec::<String>::new());
}

#[tokio::test]
async fn failed_revert_keeps_compressed_selection_readable() {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let state_db = codex_state::StateRuntime::init(
        config.sqlite_home.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .expect("initialize state database");
    let store = LocalThreadStore::new(config, Some(state_db.clone()));
    let thread_id = ThreadId::new();
    create_paginated_thread(&store, thread_id).await;
    store
        .append_items(AppendThreadItemsParams {
            thread_id,
            items: vec![turn_started("turn-1"), turn_completed("turn-1")],
        })
        .await
        .expect("append turn");
    let original_path = store
        .live_rollout_path(thread_id)
        .await
        .expect("source rollout path");
    store
        .shutdown_thread(thread_id)
        .await
        .expect("close source writer");
    reconcile_rollout(&state_db, original_path.as_path()).await;
    let original_contents = std::fs::read(original_path.as_path()).expect("read rollout");
    compress_rollout(original_path.as_path());
    let compressed_path = original_path.with_extension("jsonl.zst");

    let err = store
        .revert_thread(RevertThreadParams {
            thread_id,
            before_turn_id: "missing-turn".to_string(),
        })
        .await
        .expect_err("missing turn should fail");

    assert!(err.to_string().contains("turn not found"), "{err}");
    assert_eq!(selected_path(&state_db, thread_id).await, original_path);
    assert_eq!(std::fs::read(&original_path).expect("read plain copy"), original_contents);
    assert!(compressed_path.exists(), "failed revert keeps immutable source");
    store
        .resume_thread(ResumeThreadParams {
            thread_id,
            rollout_path: None,
            history: None,
            include_archived: false,
            metadata: ThreadPersistenceMetadata {
                cwd: Some(std::env::current_dir().expect("cwd")),
                model_provider: "test-provider".to_string(),
                memory_mode: ThreadMemoryMode::Enabled,
            },
        })
        .await
        .expect("resume after failed revert");
    assert!(!compressed_path.exists(), "resuming canonicalizes representation");
    store
        .shutdown_thread(thread_id)
        .await
        .expect("shutdown resumed writer");
    assert_eq!(turn_ids(&store, thread_id).await, vec!["turn-1"]);
}

#[tokio::test]
async fn stale_revert_publication_removes_replacement_without_mutating_selection() {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let state_db = codex_state::StateRuntime::init(
        config.sqlite_home.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .expect("initialize state database");
    let thread_id = ThreadId::new();
    let selected = home.path().join("selected.jsonl");
    let stale = home.path().join("stale.jsonl");
    let replacement = home.path().join("replacement.jsonl");
    std::fs::write(replacement.as_path(), "replacement").expect("write replacement");
    seed_selected_rollout(&state_db, thread_id, selected.clone()).await;

    let err = publish_replacement(
        &state_db,
        thread_id,
        stale.as_path(),
        replacement.as_path(),
    )
    .await
    .expect_err("stale publication should conflict");

    assert!(matches!(err, ThreadStoreError::Conflict { .. }));
    assert!(!replacement.exists());
    assert_eq!(selected_path(&state_db, thread_id).await, selected);
}

#[test]
fn replacement_write_failures_never_allow_publication() {
    let error = |message| Some(std::io::Error::other(message));

    assert_eq!(
        replacement_write_error(/*persist_error*/ None, /*shutdown_error*/ None),
        None
    );
    assert_eq!(
        replacement_write_error(error("persist"), /*shutdown_error*/ None),
        Some("failed to persist replacement rollout: persist".to_string())
    );
    assert_eq!(
        replacement_write_error(/*persist_error*/ None, error("shutdown")),
        Some("failed to shut down replacement rollout: shutdown".to_string())
    );
    assert_eq!(
        replacement_write_error(error("persist"), error("shutdown")),
        Some(
            "failed to persist replacement rollout: persist; shutdown failed: shutdown".to_string()
        )
    );
}

#[tokio::test]
async fn revert_rejects_noncanonical_paginated_selection() {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let state_db = codex_state::StateRuntime::init(
        config.sqlite_home.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .expect("initialize state database");
    let store = LocalThreadStore::new(config, Some(state_db.clone()));
    let thread_id = ThreadId::new();
    let path = home.path().join("sessions/noncanonical.jsonl");
    std::fs::create_dir_all(path.parent().expect("session directory"))
        .expect("create session directory");
    let meta = codex_rollout::RolloutLine {
        timestamp: "2026-08-20T00:00:00Z".to_string(),
        ordinal: Some(0),
        item: RolloutItem::SessionMeta(codex_protocol::protocol::SessionMetaLine {
            meta: codex_protocol::protocol::SessionMeta {
                id: thread_id,
                history_mode: ThreadHistoryMode::Paginated,
                ..Default::default()
            },
            git: None,
        }),
    };
    std::fs::write(
        path.as_path(),
        format!("{}\n", serde_json::to_string(&meta).expect("serialize metadata")),
    )
    .expect("write rollout");
    seed_selected_rollout(&state_db, thread_id, path).await;

    let err = store
        .revert_thread(RevertThreadParams {
            thread_id,
            before_turn_id: "turn-1".to_string(),
        })
        .await
        .expect_err("noncanonical selection should fail closed");

    assert!(err.to_string().contains("canonical rollout filename"), "{err}");
}

async fn create_paginated_thread(store: &LocalThreadStore, thread_id: ThreadId) {
    store
        .create_thread(CreateThreadParams {
            session_id: thread_id.into(),
            thread_id,
            extra_config: None,
            forked_from_id: None,
            parent_thread_id: None,
            source: SessionSource::Exec,
            thread_source: None,
            originator: "test_originator".to_string(),
            base_instructions: BaseInstructions::default(),
            dynamic_tools: Vec::new(),
            selected_capability_roots: Vec::new(),
            multi_agent_version: None,
            history_mode: ThreadHistoryMode::Paginated,
            history_base: None,
            subagent_history_start_ordinal: None,
            initial_window_id: "window-1".to_string(),
            metadata: ThreadPersistenceMetadata {
                cwd: Some(std::env::current_dir().expect("cwd")),
                model_provider: "test-provider".to_string(),
                memory_mode: ThreadMemoryMode::Enabled,
            },
        })
        .await
        .expect("create paginated thread");
}

async fn turn_ids(store: &LocalThreadStore, thread_id: ThreadId) -> Vec<String> {
    store
        .list_turns(ListTurnsParams {
            thread_id,
            include_archived: false,
            cursor: None,
            page_size: 10,
            sort_direction: SortDirection::Asc,
            items_view: StoredTurnItemsView::NotLoaded,
        })
        .await
        .expect("list turns")
        .turns
        .into_iter()
        .map(|turn| turn.turn_id)
        .collect()
}

async fn reconcile_rollout(state_db: &codex_rollout::StateDbHandle, path: &Path) {
    codex_rollout::state_db::reconcile_rollout(
        Some(state_db.as_ref()),
        path,
        "test-provider",
        /*builder*/ None,
        &[],
        /*archived_only*/ Some(false),
        /*new_thread_memory_mode*/ None,
    )
    .await;
}

async fn seed_selected_rollout(
    state_db: &codex_rollout::StateDbHandle,
    thread_id: ThreadId,
    path: std::path::PathBuf,
) {
    let mut builder = codex_state::ThreadMetadataBuilder::new(
        thread_id,
        path,
        chrono::Utc::now(),
        SessionSource::Exec,
    );
    builder.history_mode = ThreadHistoryMode::Paginated;
    state_db
        .upsert_thread(&builder.build("test-provider"))
        .await
        .expect("seed selected rollout");
}

async fn selected_path(
    state_db: &codex_rollout::StateDbHandle,
    thread_id: ThreadId,
) -> std::path::PathBuf {
    state_db
        .get_thread(thread_id)
        .await
        .expect("read metadata")
        .expect("thread metadata")
        .rollout_path
}

fn turn_started(turn_id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: turn_id.to_string(),
        trace_id: None,
        started_at: Some(10),
        model_context_window: None,
        collaboration_mode_kind: Default::default(),
    }))
}

fn turn_completed(turn_id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
        turn_id: turn_id.to_string(),
        last_agent_message: None,
        error: None,
        started_at: Some(10),
        completed_at: Some(20),
        duration_ms: Some(10_000),
        time_to_first_token_ms: None,
    }))
}

fn compress_rollout(path: &Path) {
    let contents = std::fs::read(path).expect("read rollout");
    let compressed = zstd::stream::encode_all(contents.as_slice(), 3).expect("compress rollout");
    std::fs::write(path.with_extension("jsonl.zst"), compressed)
        .expect("write compressed rollout");
    std::fs::remove_file(path).expect("remove plain rollout");
}
