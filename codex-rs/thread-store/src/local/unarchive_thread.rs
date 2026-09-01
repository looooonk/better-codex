use chrono::DateTime;
use chrono::Utc;
use codex_rollout::find_archived_thread_path_by_id_str;
use codex_rollout::read_thread_item_from_rollout;
use codex_rollout::rollout_date_parts;

use super::LocalThreadStore;
use super::helpers::RolloutCollection;
use super::helpers::rollout_paths_for_thread;
use super::helpers::scoped_rollout_path;
use super::helpers::set_modified_time;
use super::helpers::stored_thread_from_rollout_item;
use super::helpers::touch_modified_time;
use super::rollout_moves::PendingRolloutMoves;
use super::rollout_moves::move_rollouts;
use crate::ArchiveThreadParams;
use crate::StoredThread;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

pub(super) async fn unarchive_thread(
    store: &LocalThreadStore,
    params: ArchiveThreadParams,
) -> ThreadStoreResult<StoredThread> {
    let thread_id = params.thread_id;
    let _lifecycle_guard = store.live_writer_locks.lock_lifecycle(thread_id).await;
    let _live_writer_guard = store.live_writer_locks.lock(thread_id).await;
    store.ensure_live_recorder_absent(thread_id).await?;
    let _writer_lock = store.writer_lock_coordinator.acquire(thread_id)?;
    let state_db_ctx = store.state_db().await;
    let archived_path = find_archived_thread_path_by_id_str(
        store.config.codex_home.as_path(),
        &thread_id.to_string(),
        state_db_ctx.as_deref(),
    )
    .await
    .map_err(|err| ThreadStoreError::InvalidRequest {
        message: format!("failed to locate archived thread id {thread_id}: {err}"),
    })?
    .ok_or_else(|| ThreadStoreError::InvalidRequest {
        message: format!("no archived rollout found for thread id {thread_id}"),
    })?;

    let canonical_archived_path = scoped_rollout_path(
        store
            .config
            .codex_home
            .join(codex_rollout::ARCHIVED_SESSIONS_SUBDIR),
        archived_path.as_path(),
        "archived",
    )?;
    let rollout_paths = rollout_paths_for_thread(
        store.config.codex_home.as_path(),
        canonical_archived_path.as_path(),
        thread_id,
        RolloutCollection::Archived,
    )
    .await?;
    let mut restored_path = None;
    let mut moves = Vec::with_capacity(rollout_paths.len());
    for source in rollout_paths {
        let file_name = source
            .file_name()
            .ok_or_else(|| ThreadStoreError::InvalidRequest {
                message: format!("rollout path `{}` missing file name", source.display()),
            })?;
        let Some((year, month, day)) = rollout_date_parts(file_name) else {
            return Err(ThreadStoreError::InvalidRequest {
                message: format!(
                    "rollout path `{}` missing filename timestamp",
                    source.display()
                ),
            });
        };
        let destination = store
            .config
            .codex_home
            .join(codex_rollout::SESSIONS_SUBDIR)
            .join(year)
            .join(month)
            .join(day)
            .join(file_name);
        if source == canonical_archived_path {
            restored_path = Some(destination.clone());
        }
        moves.push((source, destination));
    }
    let restored_path = restored_path.ok_or_else(|| ThreadStoreError::Internal {
        message: "selected rollout missing from unarchive set".to_string(),
    })?;
    let mut item = read_thread_item_from_rollout(canonical_archived_path.clone())
        .await
        .ok_or_else(|| ThreadStoreError::Internal {
            message: format!(
                "failed to read archived thread {}",
                canonical_archived_path.display()
            ),
        })?;
    item.path = restored_path.clone();
    let mut thread = stored_thread_from_rollout_item(
        item,
        /*archived*/ false,
        store.config.default_model_provider_id.as_str(),
    )
    .ok_or_else(|| ThreadStoreError::Internal {
        message: format!(
            "failed to read archived thread id from {}",
            canonical_archived_path.display()
        ),
    })?;
    if thread.thread_id != thread_id {
        return Err(ThreadStoreError::InvalidRequest {
            message: format!(
                "archived rollout `{}` contains thread id {}, expected {thread_id}",
                archived_path.display(),
                thread.thread_id
            ),
        });
    }

    let original_modified = std::fs::metadata(&canonical_archived_path)
        .and_then(|metadata| metadata.modified())
        .ok();
    let pending_moves = move_rollouts(moves, "unarchive")?;
    if let Err(err) = touch_modified_time(restored_path.as_path()) {
        return Err(fail_unarchive(
            pending_moves,
            restored_path.as_path(),
            original_modified,
            format!("failed to update rollout timestamp: {err}"),
        ));
    }

    if let Some(ctx) = state_db_ctx
        && let Err(err) = ctx
            .mark_unarchived(thread_id, restored_path.as_path())
            .await
    {
        return Err(fail_unarchive(
            pending_moves,
            restored_path.as_path(),
            original_modified,
            err,
        ));
    }
    pending_moves.commit();

    if let Ok(modified) =
        std::fs::metadata(restored_path.as_path()).and_then(|metadata| metadata.modified())
    {
        let modified = DateTime::<Utc>::from(modified);
        thread.updated_at = modified;
        thread.recency_at = modified;
    }
    Ok(thread)
}

fn fail_unarchive(
    pending_moves: PendingRolloutMoves,
    restored_path: &std::path::Path,
    original_modified: Option<std::time::SystemTime>,
    cause: impl std::fmt::Display,
) -> ThreadStoreError {
    let cause = match original_modified.map(|modified| set_modified_time(restored_path, modified)) {
        Some(Err(restore_err)) => {
            format!("{cause}; failed to restore rollout timestamp: {restore_err}")
        }
        Some(Ok(())) | None => cause.to_string(),
    };
    pending_moves.fail(format!("failed to unarchive thread: {cause}"))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use codex_protocol::ThreadId;
    use codex_protocol::protocol::SessionSource;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;
    use uuid::Uuid;

    use super::*;
    use crate::ThreadStore;
    use crate::local::LocalThreadStore;
    use crate::local::test_support::test_config;
    use crate::local::test_support::write_archived_session_file;

    #[tokio::test]
    async fn unarchive_thread_restores_rollout_and_returns_updated_thread() {
        let home = TempDir::new().expect("temp dir");
        let store = LocalThreadStore::new(test_config(home.path()), /*state_db*/ None);
        let uuid = Uuid::from_u128(203);
        let thread_id = ThreadId::from_string(&uuid.to_string()).expect("valid thread id");
        let archived_path = write_archived_session_file(home.path(), "2025-01-03T13-00-00", uuid)
            .expect("archived session file");
        let replacement_source =
            write_archived_session_file(home.path(), "2025-01-03T14-00-00", uuid)
                .expect("replacement source");
        let rollout_id = Uuid::from_u128(209);
        let selected_archived_path = replacement_source.with_file_name(format!(
            "rollout-2025-01-03T14-00-00-{thread_id}_{rollout_id}.jsonl"
        ));
        std::fs::rename(replacement_source, &selected_archived_path).expect("replacement rollout");

        let thread = store
            .unarchive_thread(ArchiveThreadParams { thread_id })
            .await
            .expect("unarchive thread");

        assert!(!archived_path.exists());
        assert!(!selected_archived_path.exists());
        let restored_path = home
            .path()
            .join("sessions/2025/01/03")
            .join(selected_archived_path.file_name().expect("file name"));
        let restored_original_path = home
            .path()
            .join("sessions/2025/01/03")
            .join(archived_path.file_name().expect("file name"));
        assert!(restored_path.exists());
        assert!(restored_original_path.exists());
        assert_eq!(thread.thread_id, thread_id);
        assert_eq!(thread.rollout_path, Some(restored_path));
        assert_eq!(thread.archived_at, None);
        assert_eq!(thread.preview, "Archived user message");
        assert_eq!(
            thread.first_user_message.as_deref(),
            Some("Archived user message")
        );
    }

    #[tokio::test]
    async fn unarchive_thread_leaves_malformed_rollout_archived() {
        let home = TempDir::new().expect("temp dir");
        let store = LocalThreadStore::new(test_config(home.path()), /*state_db*/ None);
        let uuid = Uuid::from_u128(205);
        let thread_id = ThreadId::from_string(&uuid.to_string()).expect("valid thread id");
        let archived_path = write_archived_session_file(home.path(), "2025-01-03T13-00-00", uuid)
            .expect("archived session file");
        std::fs::write(&archived_path, "not a rollout").expect("malformed rollout");

        let error = store
            .unarchive_thread(ArchiveThreadParams { thread_id })
            .await
            .expect_err("malformed rollout should fail");

        assert!(matches!(error, ThreadStoreError::Internal { .. }));
        assert!(archived_path.exists());
        assert!(
            !home
                .path()
                .join("sessions/2025/01/03")
                .join(archived_path.file_name().expect("file name"))
                .exists()
        );
    }

    #[tokio::test]
    async fn unarchive_thread_updates_sqlite_metadata_when_present() {
        let home = TempDir::new().expect("temp dir");
        let config = test_config(home.path());
        let uuid = Uuid::from_u128(204);
        let thread_id = ThreadId::from_string(&uuid.to_string()).expect("valid thread id");
        let archived_path = write_archived_session_file(home.path(), "2025-01-03T13-00-00", uuid)
            .expect("archived session file");
        let runtime = codex_state::StateRuntime::init(
            home.path().to_path_buf(),
            config.default_model_provider_id.clone(),
        )
        .await
        .expect("state db should initialize");
        let store = LocalThreadStore::new(config.clone(), Some(runtime.clone()));
        runtime
            .mark_backfill_complete(/*last_watermark*/ None)
            .await
            .expect("backfill should be complete");
        let mut builder = codex_state::ThreadMetadataBuilder::new(
            thread_id,
            archived_path.clone(),
            Utc::now(),
            SessionSource::Cli,
        );
        builder.model_provider = Some(config.default_model_provider_id.clone());
        builder.cwd = home.path().to_path_buf();
        builder.cli_version = Some("test_version".to_string());
        let mut metadata = builder.build(config.default_model_provider_id.as_str());
        metadata.archived_at = Some(metadata.updated_at);
        runtime
            .upsert_thread(&metadata)
            .await
            .expect("state db upsert should succeed");

        store
            .unarchive_thread(ArchiveThreadParams { thread_id })
            .await
            .expect("unarchive thread");

        let restored_path = home
            .path()
            .join("sessions/2025/01/03")
            .join(archived_path.file_name().expect("file name"));
        let updated = runtime
            .get_thread(thread_id)
            .await
            .expect("state db read should succeed")
            .expect("thread metadata should exist");
        assert_eq!(updated.rollout_path, restored_path);
        assert_eq!(updated.archived_at, None);
        assert_eq!(updated.recency_at, metadata.recency_at);
    }

    #[tokio::test]
    async fn unarchive_thread_restores_rollout_when_sqlite_update_fails() {
        let home = TempDir::new().expect("temp dir");
        let config = test_config(home.path());
        let uuid = Uuid::from_u128(207);
        let thread_id = ThreadId::from_string(&uuid.to_string()).expect("valid thread id");
        let archived_path = write_archived_session_file(home.path(), "2025-01-03T13-00-00", uuid)
            .expect("archived session file");
        let replacement_source =
            write_archived_session_file(home.path(), "2025-01-03T14-00-00", uuid)
                .expect("replacement source");
        let selected_archived_path = replacement_source.with_file_name(format!(
            "rollout-2025-01-03T14-00-00-{thread_id}_{}.jsonl",
            Uuid::from_u128(211)
        ));
        std::fs::rename(replacement_source, &selected_archived_path).expect("replacement rollout");
        let runtime = codex_state::StateRuntime::init(
            home.path().to_path_buf(),
            config.default_model_provider_id.clone(),
        )
        .await
        .expect("state db should initialize");
        let store = LocalThreadStore::new(config.clone(), Some(runtime.clone()));
        let mut builder = codex_state::ThreadMetadataBuilder::new(
            thread_id,
            selected_archived_path.clone(),
            Utc::now(),
            SessionSource::Cli,
        );
        builder.model_provider = Some(config.default_model_provider_id.clone());
        builder.cwd = home.path().to_path_buf();
        let mut metadata = builder.build(config.default_model_provider_id.as_str());
        metadata.archived_at = Some(metadata.updated_at);
        runtime
            .upsert_thread(&metadata)
            .await
            .expect("state db upsert should succeed");
        runtime.close().await;

        let error = store
            .unarchive_thread(ArchiveThreadParams { thread_id })
            .await
            .expect_err("closed state db should fail unarchive");

        assert!(matches!(error, ThreadStoreError::Internal { .. }));
        assert!(archived_path.exists());
        assert!(selected_archived_path.exists());
        assert!(
            !home
                .path()
                .join("sessions/2025/01/03")
                .join(archived_path.file_name().expect("file name"))
                .exists()
        );
        assert!(
            !home
                .path()
                .join("sessions/2025/01/03")
                .join(selected_archived_path.file_name().expect("file name"))
                .exists()
        );
    }
}
