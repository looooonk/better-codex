use std::fs;

use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_state::ThreadMetadataBuilder;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use uuid::Uuid;

use super::*;
use crate::local::test_support::test_config;
use crate::local::test_support::write_session_file_with_history_mode;

fn id(value: u128) -> ThreadId {
    ThreadId::from_string(&Uuid::from_u128(value).to_string()).expect("valid thread id")
}

#[tokio::test]
async fn selected_replacement_rollout_is_authoritative() {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let thread_uuid = Uuid::from_u128(401);
    let thread_id = id(401);
    let rollout_id = id(402);
    let old_path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T12-00-00",
        thread_uuid,
        ThreadHistoryMode::Paginated,
    )
    .expect("old rollout");
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
    fs::rename(&temporary_path, &selected_path).expect("rename replacement rollout");

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

    assert_eq!(
        resolve_current(&store, thread_id).await.expect("resolve"),
        Some(ResolvedThreadRollout {
            thread_id,
            rollout_id,
            path: selected_path.clone(),
            location: RolloutLocation::Unarchived,
        })
    );

    fs::remove_file(selected_path).expect("remove selected rollout");
    assert_eq!(
        resolve_current(&store, thread_id)
            .await
            .expect("missing selected rollout"),
        None
    );
    assert!(old_path.exists());
}

#[tokio::test]
async fn unreadable_replacement_rollout_cannot_claim_a_thread() {
    let home = TempDir::new().expect("temp dir");
    let thread_id = id(403);
    let rollout_id = id(404);
    let path = home.path().join(format!(
        "rollout-2025-01-03T13-00-00-{thread_id}_{rollout_id}.jsonl"
    ));
    fs::write(&path, b"not session metadata\n").expect("write invalid rollout");

    let error = rollout_id_for_thread_path(&path, thread_id, ThreadHistoryMode::Legacy)
        .await
        .expect_err("reject unreadable replacement rollout");
    let ThreadStoreError::InvalidRequest { message } = error else {
        panic!("expected invalid request");
    };
    assert_eq!(
        message,
        format!(
            "replacement rollout at {} does not have readable session metadata",
            path.display()
        )
    );
}
