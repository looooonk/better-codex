use std::path::Path;

use codex_protocol::RolloutId;
use codex_protocol::ThreadId;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::ThreadMemoryMode;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_rollout::RolloutItem;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::super::test_support::test_config;
use super::LocalThreadStore;
use super::normalize_composite_selection;
use super::publication::publish_replacement;
use super::publication::replacement_write_error;
use super::repair_composite_selection;
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
async fn revert_keeps_head_context_and_hides_suffix_across_compressed_sources() {
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
                user_message("kept context"),
                turn_completed("turn-1"),
                turn_started("turn-2"),
                user_message("discarded context"),
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
    assert_eq!(first_replacement_path, original_path);
    assert_eq!(
        codex_rollout::rollout_id_from_path(first_replacement_path.as_path()),
        Some(thread_id)
    );
    let replacement_meta = codex_rollout::read_session_meta_line(first_replacement_path.as_path())
        .await
        .expect("read replacement metadata")
        .meta;
    assert_eq!(replacement_meta.id, thread_id);
    let first_rollout_id = replacement_meta
        .rollout_id
        .expect("replacement rollout identity");
    assert_ne!(first_rollout_id, thread_id);
    assert_eq!(replacement_meta.history_base, None);
    assert_eq!(
        head_only_user_messages(first_replacement_path.as_path()).await,
        vec!["kept context"]
    );
    assert_eq!(turn_ids(&store, thread_id).await, vec!["turn-1"]);

    store
        .revert_thread(RevertThreadParams {
            thread_id,
            before_turn_id: "turn-1".to_string(),
        })
        .await
        .expect("revert before first turn");
    let second_replacement_path = selected_path(&state_db, thread_id).await;
    assert_eq!(second_replacement_path, first_replacement_path);
    assert!(original_path.exists());
    assert!(first_replacement_path.exists());
    let second_meta = codex_rollout::read_session_meta_line(second_replacement_path.as_path())
        .await
        .expect("read second replacement metadata")
        .meta;
    assert_ne!(second_meta.rollout_id, Some(first_rollout_id));
    assert_ne!(second_meta.rollout_id, Some(thread_id));
    assert_eq!(
        head_only_user_messages(second_replacement_path.as_path()).await,
        Vec::<String>::new()
    );
    assert_eq!(turn_ids(&store, thread_id).await, Vec::<String>::new());
    assert_eq!(
        codex_rollout::find_thread_paths_by_id(home.path(), thread_id)
            .await
            .expect("visible rollout paths"),
        vec![original_path.clone()]
    );
    assert_eq!(
        codex_rollout::find_thread_path_by_id_str(
            home.path(),
            thread_id.to_string().as_str(),
            /*state_db_ctx*/ None,
        )
        .await
        .expect("database-less fallback"),
        Some(original_path)
    );
    let revisions = codex_rollout::find_rollout_revision_paths_by_thread(home.path(), thread_id)
        .await
        .expect("retained revisions");
    assert!(revisions.iter().any(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(first_rollout_id.to_string().as_str()))
    }));
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
    assert_eq!(
        std::fs::read(&original_path).expect("read plain copy"),
        original_contents
    );
    assert!(
        compressed_path.exists(),
        "failed revert keeps immutable source"
    );
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
    assert!(
        !compressed_path.exists(),
        "resuming canonicalizes representation"
    );
    store
        .shutdown_thread(thread_id)
        .await
        .expect("shutdown resumed writer");
    assert_eq!(turn_ids(&store, thread_id).await, vec!["turn-1"]);
}

#[tokio::test]
async fn stale_revert_publication_restores_existing_stable_representations() {
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
    let selected = home.path().join("selected.jsonl");
    let stale = home.path().join("stale.jsonl");
    let replacement = home.path().join("replacement.jsonl");
    std::fs::write(replacement.as_path(), "replacement").expect("write replacement");
    seed_selected_rollout(&state_db, thread_id, selected.clone()).await;

    let stable = home.path().join("stable.jsonl");
    let compressed_stable = write_stable_representations(stable.as_path());
    let err = publish_replacement(
        &store,
        &state_db,
        thread_id,
        stale.as_path(),
        replacement.as_path(),
        stable.as_path(),
        &[],
    )
    .await
    .expect_err("stale publication should conflict");

    assert!(matches!(err, ThreadStoreError::Conflict { .. }));
    assert!(!replacement.exists());
    assert_stable_representations(stable.as_path(), compressed_stable.as_path());
    assert_eq!(selected_path(&state_db, thread_id).await, selected);
    assert_no_revert_rollback_artifacts(home.path());
}

#[tokio::test]
async fn database_error_during_revert_publication_restores_existing_stable_representations() {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let state_db = codex_state::StateRuntime::init(
        config.sqlite_home.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .expect("initialize state database");
    let store = LocalThreadStore::new(config.clone(), Some(state_db.clone()));
    let thread_id = ThreadId::new();
    let selected = home.path().join("selected.jsonl");
    let replacement = home.path().join("replacement.jsonl");
    std::fs::write(replacement.as_path(), "replacement").expect("write replacement");
    seed_selected_rollout(&state_db, thread_id, selected.clone()).await;

    let stable = home.path().join("stable.jsonl");
    let compressed_stable = write_stable_representations(stable.as_path());
    state_db.close().await;

    let err = publish_replacement(
        &store,
        &state_db,
        thread_id,
        selected.as_path(),
        replacement.as_path(),
        stable.as_path(),
        &[],
    )
    .await
    .expect_err("closed database should fail publication");

    assert!(matches!(err, ThreadStoreError::Internal { .. }));
    assert!(!replacement.exists());
    assert_stable_representations(stable.as_path(), compressed_stable.as_path());
    assert_no_revert_rollback_artifacts(home.path());

    let reopened_state_db =
        codex_state::StateRuntime::init(config.sqlite_home, config.default_model_provider_id)
            .await
            .expect("reopen state database");
    assert_eq!(selected_path(&reopened_state_db, thread_id).await, selected);
}

#[tokio::test]
async fn composite_selection_normalizes_to_equivalent_stable_head_before_retirement() {
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
                user_message("first context"),
                turn_completed("turn-1"),
                turn_started("turn-2"),
                user_message("second context"),
                turn_completed("turn-2"),
            ],
        })
        .await
        .expect("append turns");
    let stable_path = store
        .live_rollout_path(thread_id)
        .await
        .expect("source rollout path");
    store
        .shutdown_thread(thread_id)
        .await
        .expect("close source writer");
    reconcile_rollout(&state_db, stable_path.as_path()).await;

    let composite_rollout_id = RolloutId::new();
    let stem = stable_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("rollout stem");
    let composite_path = stable_path.with_file_name(format!("{stem}_{composite_rollout_id}.jsonl"));
    std::fs::rename(stable_path.as_path(), composite_path.as_path())
        .expect("rename composite rollout");
    assert!(
        state_db
            .replace_rollout_path_if_current(
                thread_id,
                stable_path.as_path(),
                composite_path.as_path(),
            )
            .await
            .expect("select composite rollout")
    );

    let source_meta = codex_rollout::read_session_meta_line(composite_path.as_path())
        .await
        .expect("read composite metadata")
        .meta;
    let lineage = store
        .resolve_rollout_lineage(thread_id)
        .await
        .expect("resolve composite lineage");
    normalize_composite_selection(
        &store,
        &state_db,
        thread_id,
        source_meta,
        &lineage,
        composite_path.as_path(),
        stable_path.as_path(),
    )
    .await
    .expect("normalize composite selection");

    assert_eq!(selected_path(&state_db, thread_id).await, stable_path);
    assert!(composite_path.exists(), "normalization keeps the old head");
    assert_eq!(
        head_only_user_messages(stable_path.as_path()).await,
        head_only_user_messages(composite_path.as_path()).await
    );
    let normalized_meta = codex_rollout::read_session_meta_line(stable_path.as_path())
        .await
        .expect("read normalized metadata")
        .meta;
    assert_eq!(normalized_meta.history_base, None);
    assert!(
        normalized_meta
            .rollout_id
            .is_some_and(|rollout_id| rollout_id != composite_rollout_id && rollout_id != thread_id)
    );
}

#[tokio::test]
async fn archived_composite_repair_does_not_mutate_active_head() {
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
                user_message("active context"),
                turn_completed("turn-1"),
            ],
        })
        .await
        .expect("append active turn");
    let active_path = store
        .live_rollout_path(thread_id)
        .await
        .expect("active rollout path");
    store
        .shutdown_thread(thread_id)
        .await
        .expect("close active writer");
    reconcile_rollout(&state_db, active_path.as_path()).await;
    let active_bytes = std::fs::read(active_path.as_path()).expect("read active rollout");

    let archived_dir = home.path().join(codex_rollout::ARCHIVED_SESSIONS_SUBDIR);
    std::fs::create_dir_all(archived_dir.as_path()).expect("create archived directory");
    let stem = active_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("active rollout stem");
    let archived_composite_path = archived_dir.join(format!("{stem}_{}.jsonl", RolloutId::new()));
    std::fs::copy(active_path.as_path(), archived_composite_path.as_path())
        .expect("copy archived composite");
    state_db
        .mark_archived(
            thread_id,
            archived_composite_path.as_path(),
            chrono::Utc::now(),
        )
        .await
        .expect("select archived composite");

    let selected = super::super::thread_rollout_resolver::resolve_current_including_archived(
        &store, thread_id,
    )
    .await
    .expect("resolve archived selection")
    .expect("archived selection");
    let repaired = repair_composite_selection(&store, selected)
        .await
        .expect("repair archived composite");
    let archived_stable_path =
        archived_dir.join(active_path.file_name().expect("active rollout file name"));
    let canonical_archived_stable_path =
        std::fs::canonicalize(archived_stable_path.as_path()).expect("canonical archived path");

    assert_eq!(repaired.path, canonical_archived_stable_path);
    assert_eq!(
        selected_path(&state_db, thread_id).await,
        canonical_archived_stable_path
    );
    assert!(!archived_composite_path.exists());
    assert_eq!(
        std::fs::read(active_path.as_path()).expect("read preserved active rollout"),
        active_bytes
    );
    assert_eq!(
        codex_rollout::find_thread_paths_by_id(home.path(), thread_id)
            .await
            .expect("active rollout inventory"),
        vec![active_path]
    );
    assert_eq!(
        codex_rollout::find_archived_thread_paths_by_id(home.path(), thread_id)
            .await
            .expect("archived rollout inventory"),
        vec![archived_stable_path]
    );
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
        format!(
            "{}\n",
            serde_json::to_string(&meta).expect("serialize metadata")
        ),
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

    assert!(
        err.to_string().contains("canonical rollout filename"),
        "{err}"
    );
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
            turn_id: None,
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

fn write_stable_representations(stable_path: &Path) -> std::path::PathBuf {
    std::fs::write(stable_path, b"original plain stable").expect("write plain stable rollout");
    let compressed_path = stable_path.with_extension("jsonl.zst");
    std::fs::write(&compressed_path, b"original compressed stable")
        .expect("write compressed stable rollout");
    compressed_path
}

fn assert_stable_representations(stable_path: &Path, compressed_path: &Path) {
    assert_eq!(
        std::fs::read(stable_path).expect("read restored plain stable rollout"),
        b"original plain stable"
    );
    assert_eq!(
        std::fs::read(compressed_path).expect("read restored compressed stable rollout"),
        b"original compressed stable"
    );
}

fn assert_no_revert_rollback_artifacts(codex_home: &Path) {
    let staging = codex_home
        .join(codex_rollout::ROLLOUT_REVISIONS_SUBDIR)
        .join(".staging");
    assert!(
        staging
            .read_dir()
            .expect("read revert staging directory")
            .next()
            .is_none()
    );
}

async fn head_only_user_messages(path: &Path) -> Vec<String> {
    let (items, _, parse_errors) = codex_rollout::RolloutRecorder::load_rollout_items(path)
        .await
        .expect("load replacement head directly");
    assert_eq!(parse_errors, 0);
    items
        .into_iter()
        .filter_map(|item| {
            let RolloutItem::ResponseItem(envelope) = item else {
                return None;
            };
            let ResponseItem::Message { role, content, .. } = envelope.item else {
                return None;
            };
            if role != "user" {
                return None;
            }
            content.into_iter().find_map(|content| match content {
                ContentItem::InputText { text } => Some(text),
                _ => None,
            })
        })
        .collect()
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

fn user_message(message: &str) -> RolloutItem {
    RolloutItem::ResponseItem(
        ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: message.to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    )
}

fn compress_rollout(path: &Path) {
    let contents = std::fs::read(path).expect("read rollout");
    let compressed = zstd::stream::encode_all(contents.as_slice(), 3).expect("compress rollout");
    std::fs::write(path.with_extension("jsonl.zst"), compressed).expect("write compressed rollout");
    std::fs::remove_file(path).expect("remove plain rollout");
}
