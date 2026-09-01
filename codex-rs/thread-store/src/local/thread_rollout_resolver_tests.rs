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
async fn db_loss_prefers_newer_composite_head_to_older_stable_head() {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let thread_id = id(412);
    let rollout_id = id(413);
    let stable_path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T12-00-00",
        Uuid::from_u128(412),
        ThreadHistoryMode::Paginated,
    )
    .expect("stable rollout");
    let source = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T13-00-00",
        Uuid::from_u128(412),
        ThreadHistoryMode::Paginated,
    )
    .expect("composite rollout");
    let composite_path = source.with_file_name(format!(
        "rollout-2025-01-03T13-00-00-{thread_id}_{rollout_id}.jsonl"
    ));
    fs::rename(source, composite_path.as_path()).expect("rename composite rollout");
    let store = LocalThreadStore::new(config, /*state_db*/ None);

    assert_eq!(
        resolve_current(&store, thread_id).await.expect("resolve"),
        Some(ResolvedThreadRollout {
            thread_id,
            rollout_id,
            path: composite_path,
            location: RolloutLocation::Unarchived,
        })
    );
    assert!(stable_path.exists());
}

#[tokio::test]
async fn corrupt_paginated_selection_recovers_newer_composite_head() {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let thread_id = id(414);
    let rollout_id = id(415);
    let stable_path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T12-00-00",
        Uuid::from_u128(414),
        ThreadHistoryMode::Paginated,
    )
    .expect("stable rollout");
    let mismatched_path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T13-00-00",
        Uuid::from_u128(416),
        ThreadHistoryMode::Paginated,
    )
    .expect("mismatched rollout");
    let source = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T14-00-00",
        Uuid::from_u128(414),
        ThreadHistoryMode::Paginated,
    )
    .expect("composite rollout");
    let composite_path = source.with_file_name(format!(
        "rollout-2025-01-03T14-00-00-{thread_id}_{rollout_id}.jsonl"
    ));
    fs::rename(source, composite_path.as_path()).expect("rename composite rollout");
    let runtime = codex_state::StateRuntime::init(
        config.sqlite_home.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .expect("state runtime");
    let mut builder =
        ThreadMetadataBuilder::new(thread_id, mismatched_path, Utc::now(), SessionSource::Cli);
    builder.history_mode = ThreadHistoryMode::Paginated;
    runtime
        .upsert_thread(&builder.build(config.default_model_provider_id.as_str()))
        .await
        .expect("seed corrupt selection");
    let store = LocalThreadStore::new(config, Some(runtime));

    assert_eq!(
        resolve_current(&store, thread_id).await.expect("recover"),
        Some(ResolvedThreadRollout {
            thread_id,
            rollout_id,
            path: composite_path,
            location: RolloutLocation::Unarchived,
        })
    );
    assert!(stable_path.exists());
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

#[tokio::test]
async fn stable_rollout_basename_uses_metadata_rollout_identity() {
    let home = TempDir::new().expect("temp dir");
    let thread_id = id(407);
    let rollout_id = id(408);
    let path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T13-00-00",
        Uuid::from_u128(407),
        ThreadHistoryMode::Paginated,
    )
    .expect("stable rollout");
    set_rollout_id(path.as_path(), rollout_id);

    assert_eq!(
        rollout_id_for_thread_path(path.as_path(), thread_id, ThreadHistoryMode::Paginated)
            .await
            .expect("resolve metadata identity"),
        rollout_id
    );
}

#[tokio::test]
async fn composite_basename_must_match_metadata_rollout_identity() {
    let home = TempDir::new().expect("temp dir");
    let thread_id = id(409);
    let file_rollout_id = id(410);
    let metadata_rollout_id = id(411);
    let source = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T13-00-00",
        Uuid::from_u128(409),
        ThreadHistoryMode::Paginated,
    )
    .expect("rollout");
    let path = source.with_file_name(format!(
        "rollout-2025-01-03T13-00-00-{thread_id}_{file_rollout_id}.jsonl"
    ));
    fs::rename(source, path.as_path()).expect("composite rollout");
    set_rollout_id(path.as_path(), metadata_rollout_id);

    let error = rollout_id_for_thread_path(path.as_path(), thread_id, ThreadHistoryMode::Paginated)
        .await
        .expect_err("reject mismatched composite identity");
    assert!(
        error
            .to_string()
            .contains("rollout identity disagrees with canonical filename")
    );
}

#[tokio::test]
async fn mismatched_paginated_selection_does_not_fall_back_to_an_ordinary_sibling() {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let thread_id = id(405);
    let other_thread_id = id(406);
    let ordinary_path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T12-00-00",
        Uuid::from_u128(405),
        ThreadHistoryMode::Paginated,
    )
    .expect("ordinary rollout");
    let mismatched_path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T13-00-00",
        Uuid::from_u128(406),
        ThreadHistoryMode::Paginated,
    )
    .expect("mismatched rollout");
    let runtime = codex_state::StateRuntime::init(
        config.sqlite_home.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .expect("state runtime");
    let mut builder = ThreadMetadataBuilder::new(
        thread_id,
        mismatched_path.clone(),
        Utc::now(),
        SessionSource::Cli,
    );
    builder.history_mode = ThreadHistoryMode::Paginated;
    runtime
        .upsert_thread(&builder.build(config.default_model_provider_id.as_str()))
        .await
        .expect("seed mismatched selection");
    let store = LocalThreadStore::new(config, Some(runtime));

    let error = resolve_current(&store, thread_id)
        .await
        .expect_err("paginated selection must fail closed");

    assert!(matches!(error, ThreadStoreError::InvalidRequest { .. }));
    assert!(error.to_string().contains(&other_thread_id.to_string()));
    assert!(ordinary_path.exists());
}

fn set_rollout_id(path: &std::path::Path, rollout_id: codex_protocol::RolloutId) {
    let contents = fs::read_to_string(path).expect("read rollout");
    let mut lines = contents.lines();
    let mut meta: serde_json::Value =
        serde_json::from_str(lines.next().expect("session metadata line"))
            .expect("parse session metadata");
    meta["payload"]["rollout_id"] = serde_json::json!(rollout_id);
    let mut updated = serde_json::to_string(&meta).expect("serialize session metadata");
    updated.push('\n');
    for line in lines {
        updated.push_str(line);
        updated.push('\n');
    }
    fs::write(path, updated).expect("write rollout identity");
}
