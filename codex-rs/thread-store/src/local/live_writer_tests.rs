use std::fs;

use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::ThreadMemoryMode;
use codex_state::ThreadMetadataBuilder;
use tempfile::TempDir;
use uuid::Uuid;

use super::super::LocalThreadStore;
use super::super::test_support::test_config;
use super::super::test_support::write_session_file_with_history_mode;
use crate::ResumeThreadParams;
use crate::ThreadPersistenceMetadata;

#[tokio::test]
async fn resume_uses_selected_paginated_rollout_instead_of_stale_requested_path() {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let thread_uuid = Uuid::from_u128(/*v*/ 501);
    let thread_id = ThreadId::from_string(thread_uuid.to_string().as_str()).expect("thread id");
    let rollout_id = ThreadId::new();
    let stale_path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T12-00-00",
        thread_uuid,
        ThreadHistoryMode::Paginated,
    )
    .expect("stale rollout");
    let temporary_path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T13-00-00",
        thread_uuid,
        ThreadHistoryMode::Paginated,
    )
    .expect("replacement rollout");
    let selected_path = temporary_path.with_file_name(format!(
        "rollout-2025-01-03T13-00-00-{thread_id}_{rollout_id}.jsonl"
    ));
    fs::rename(temporary_path, selected_path.as_path()).expect("rename replacement rollout");
    let runtime = codex_state::StateRuntime::init(
        config.sqlite_home.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .expect("state runtime");
    let mut builder = ThreadMetadataBuilder::new(
        thread_id,
        selected_path.clone(),
        Utc::now(),
        SessionSource::Cli,
    );
    builder.history_mode = ThreadHistoryMode::Paginated;
    runtime
        .upsert_thread(&builder.build(config.default_model_provider_id.as_str()))
        .await
        .expect("seed selected rollout");
    let store = LocalThreadStore::new(config, Some(runtime));

    super::resume_thread(
        &store,
        ResumeThreadParams {
            thread_id,
            rollout_path: Some(stale_path),
            history: None,
            include_archived: false,
            metadata: ThreadPersistenceMetadata {
                cwd: Some(home.path().to_path_buf()),
                model_provider: "test-provider".to_string(),
                memory_mode: ThreadMemoryMode::Enabled,
            },
        },
    )
    .await
    .expect("resume selected rollout");

    assert_eq!(
        store
            .live_rollout_path(thread_id)
            .await
            .expect("live rollout path"),
        selected_path
    );
    super::shutdown_thread(&store, thread_id)
        .await
        .expect("shutdown resumed writer");
}
