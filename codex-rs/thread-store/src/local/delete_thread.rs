//! Recoverable deletion support for persisted local threads.
//!
//! Rollout JSONL and the main state DB are deleted failure-atomically. The rebuildable paginated
//! history projection is also cleaned while writes for each affected thread are serialized.

use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use codex_protocol::ThreadId;
use codex_rollout::ARCHIVED_SESSIONS_SUBDIR;
use codex_rollout::SESSIONS_SUBDIR;
use codex_rollout::find_archived_thread_path_by_id_str;
use codex_rollout::find_thread_path_by_id_str;
use codex_rollout::remove_thread_name_entries;
use tracing::warn;

use super::LocalThreadStore;
use super::helpers::matching_rollout_file_name;
use super::helpers::scoped_rollout_path;
use crate::DeleteThreadParams;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(0);

enum MissingRolloutPolicy {
    Allow,
    Require(ThreadId),
}

struct StagedRollout {
    original_path: PathBuf,
    staged_path: PathBuf,
}

struct StagedRollouts {
    staging_dir: Option<PathBuf>,
    files: Vec<StagedRollout>,
    disposition: StagingDisposition,
}

#[derive(Clone, Copy)]
enum StagingDisposition {
    Restore,
    Discard,
    Complete,
}

pub(super) async fn delete_thread(
    store: &LocalThreadStore,
    params: DeleteThreadParams,
) -> ThreadStoreResult<()> {
    delete_threads_impl(
        store,
        &[params.thread_id],
        MissingRolloutPolicy::Require(params.thread_id),
    )
    .await
}

pub(super) async fn delete_threads(
    store: &LocalThreadStore,
    thread_ids: &[ThreadId],
) -> ThreadStoreResult<()> {
    delete_threads_impl(store, thread_ids, MissingRolloutPolicy::Allow).await
}

async fn delete_threads_impl(
    store: &LocalThreadStore,
    thread_ids: &[ThreadId],
    missing_rollout_policy: MissingRolloutPolicy,
) -> ThreadStoreResult<()> {
    if thread_ids.is_empty() {
        return Ok(());
    }

    let mut ordered_thread_ids = thread_ids.to_vec();
    ordered_thread_ids.sort_by_key(ToString::to_string);
    ordered_thread_ids.dedup();
    let mut lifecycle_guards = Vec::with_capacity(ordered_thread_ids.len());
    for thread_id in &ordered_thread_ids {
        lifecycle_guards.push(store.live_writer_locks.lock_lifecycle(*thread_id).await);
    }
    let mut live_writer_guards = Vec::with_capacity(ordered_thread_ids.len());
    for thread_id in &ordered_thread_ids {
        live_writer_guards.push(store.live_writer_locks.lock(*thread_id).await);
        store.ensure_live_recorder_absent(*thread_id).await?;
    }
    let mut writer_locks = Vec::with_capacity(ordered_thread_ids.len());
    for thread_id in &ordered_thread_ids {
        writer_locks.push(store.writer_lock_coordinator.acquire(*thread_id)?);
    }

    let state_db = store.state_db().await;
    let (rollout_paths, found_thread_ids) = locate_rollout_paths(store, thread_ids).await?;
    if let MissingRolloutPolicy::Require(thread_id) = missing_rollout_policy
        && !found_thread_ids.contains(&thread_id)
    {
        return Err(ThreadStoreError::ThreadNotFound { thread_id });
    }

    let mut projection_ids = thread_ids.to_vec();
    projection_ids.extend(
        rollout_paths
            .iter()
            .filter_map(|(path, _)| codex_rollout::rollout_id_from_path(path.as_path())),
    );
    projection_ids.sort_by_key(ToString::to_string);
    projection_ids.dedup();
    let mut staged = stage_rollouts(store, rollout_paths)?;
    for rollout_id in projection_ids {
        if let Err(err) = super::thread_history::delete_thread(store, rollout_id).await {
            return match staged.restore() {
                Ok(()) => Err(err),
                Err(restore_err) => Err(ThreadStoreError::Internal {
                    message: format!(
                        "{err}; failed to restore rollout files after history cleanup failure: {restore_err}"
                    ),
                }),
            };
        }
    }
    if let Some(state_db) = state_db.as_ref()
        && let Err(err) = state_db.delete_threads_strict(thread_ids).await
    {
        return match staged.restore() {
            Ok(()) => Err(ThreadStoreError::Internal {
                message: format!("failed to delete thread state: {err}"),
            }),
            Err(restore_err) => Err(ThreadStoreError::Internal {
                message: format!(
                    "failed to delete thread state: {err}; failed to restore rollout files: {restore_err}"
                ),
            }),
        };
    }
    staged.commit();

    for thread_id in thread_ids {
        if let Err(err) =
            remove_thread_name_entries(store.config.codex_home.as_path(), *thread_id).await
        {
            warn!("failed to delete thread name index entries for {thread_id}: {err}");
        }
    }
    staged.discard();
    Ok(())
}

async fn locate_rollout_paths(
    store: &LocalThreadStore,
    thread_ids: &[ThreadId],
) -> ThreadStoreResult<(Vec<(PathBuf, ThreadId)>, Vec<ThreadId>)> {
    let state_db = store.state_db().await;
    let mut rollout_paths = Vec::new();
    let mut found_thread_ids = Vec::new();

    for thread_id in thread_ids {
        let thread_id_str = thread_id.to_string();
        let mut thread_paths = codex_rollout::find_thread_paths_by_id(
            store.config.codex_home.as_path(),
            *thread_id,
        )
        .await
        .map_err(|err| ThreadStoreError::InvalidRequest {
            message: format!("failed to locate thread id {thread_id}: {err}"),
        })?;
        thread_paths.extend(
            codex_rollout::find_archived_thread_paths_by_id(
                store.config.codex_home.as_path(),
                *thread_id,
            )
            .await
            .map_err(|err| ThreadStoreError::InvalidRequest {
                message: format!("failed to locate archived thread id {thread_id}: {err}"),
            })?,
        );
        match find_thread_path_by_id_str(
            store.config.codex_home.as_path(),
            thread_id_str.as_str(),
            state_db.as_deref(),
        )
        .await
        {
            Ok(Some(path)) if !thread_paths.contains(&path) => thread_paths.push(path),
            Ok(Some(_)) => {}
            Ok(None) => {}
            Err(err) => {
                return Err(ThreadStoreError::InvalidRequest {
                    message: format!("failed to locate thread id {thread_id}: {err}"),
                });
            }
        }
        match find_archived_thread_path_by_id_str(
            store.config.codex_home.as_path(),
            thread_id_str.as_str(),
            state_db.as_deref(),
        )
        .await
        {
            Ok(Some(path)) if !thread_paths.contains(&path) => thread_paths.push(path),
            Ok(Some(_)) | Ok(None) => {}
            Err(err) => {
                return Err(ThreadStoreError::InvalidRequest {
                    message: format!("failed to locate archived thread id {thread_id}: {err}"),
                });
            }
        }
        thread_paths.sort();
        thread_paths.dedup();
        if !thread_paths.is_empty() {
            found_thread_ids.push(*thread_id);
        }
        rollout_paths.extend(thread_paths.into_iter().map(|path| (path, *thread_id)));
    }

    Ok((rollout_paths, found_thread_ids))
}

fn stage_rollouts(
    store: &LocalThreadStore,
    rollout_paths: Vec<(PathBuf, ThreadId)>,
) -> ThreadStoreResult<StagedRollouts> {
    let mut candidates = Vec::with_capacity(rollout_paths.len() * 2);
    for (rollout_path, thread_id) in rollout_paths {
        let plain_path = codex_rollout::plain_rollout_path(&rollout_path);
        for path in [plain_path.clone(), plain_path.with_extension("jsonl.zst")] {
            candidates.push((
                validated_rollout_path(store, path.as_path(), thread_id)?,
                thread_id,
            ));
        }
    }
    candidates.sort_by(|(left, _), (right, _)| left.cmp(right));
    candidates.dedup_by(|(left, _), (right, _)| left == right);
    if candidates.is_empty() {
        return Ok(StagedRollouts {
            staging_dir: None,
            files: Vec::new(),
            disposition: StagingDisposition::Restore,
        });
    }

    let staging_dir = create_staging_dir(store.config.codex_home.as_path())?;
    let mut staged = StagedRollouts {
        staging_dir: Some(staging_dir.clone()),
        files: Vec::new(),
        disposition: StagingDisposition::Restore,
    };
    for (index, (original_path, thread_id)) in candidates.into_iter().enumerate() {
        let staged_path = staging_dir.join(index.to_string());
        match std::fs::rename(&original_path, &staged_path) {
            Ok(()) => staged.files.push(StagedRollout {
                original_path,
                staged_path,
            }),
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                let stage_err = ThreadStoreError::Internal {
                    message: format!(
                        "failed to stage rollout file `{}` for thread {thread_id}: {err}",
                        original_path.display()
                    ),
                };
                return match staged.restore() {
                    Ok(()) => Err(stage_err),
                    Err(restore_err) => Err(ThreadStoreError::Internal {
                        message: format!(
                            "{stage_err}; failed to restore staged rollout files: {restore_err}"
                        ),
                    }),
                };
            }
        }
    }

    Ok(staged)
}

fn validated_rollout_path(
    store: &LocalThreadStore,
    rollout_path: &Path,
    thread_id: ThreadId,
) -> ThreadStoreResult<PathBuf> {
    let canonical_rollout_path = scoped_rollout_path(
        store.config.codex_home.join(SESSIONS_SUBDIR),
        rollout_path,
        "sessions",
    )
    .or_else(|_| {
        scoped_rollout_path(
            store.config.codex_home.join(ARCHIVED_SESSIONS_SUBDIR),
            rollout_path,
            "archived sessions",
        )
    })
    .or_else(|err| match rollout_path.try_exists() {
        Ok(false) => Ok(rollout_path.to_path_buf()),
        Ok(true) | Err(_) => Err(err),
    })?;
    matching_rollout_file_name(&canonical_rollout_path, thread_id, rollout_path)?;
    Ok(canonical_rollout_path)
}

fn create_staging_dir(codex_home: &Path) -> ThreadStoreResult<PathBuf> {
    let staging_root = codex_home.join(".thread-delete-staging");
    std::fs::create_dir_all(&staging_root).map_err(|err| ThreadStoreError::Internal {
        message: format!(
            "failed to create thread deletion staging root `{}`: {err}",
            staging_root.display()
        ),
    })?;
    loop {
        let id = NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed);
        let staging_dir = staging_root.join(format!("{}-{id}", std::process::id()));
        match std::fs::create_dir(&staging_dir) {
            Ok(()) => return Ok(staging_dir),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
            Err(err) => {
                return Err(ThreadStoreError::Internal {
                    message: format!(
                        "failed to create thread deletion staging directory `{}`: {err}",
                        staging_dir.display()
                    ),
                });
            }
        }
    }
}

impl StagedRollouts {
    fn commit(&mut self) {
        self.disposition = StagingDisposition::Discard;
    }

    fn restore(mut self) -> ThreadStoreResult<()> {
        let result = self.restore_inner();
        self.disposition = StagingDisposition::Complete;
        result
    }

    fn restore_inner(&mut self) -> ThreadStoreResult<()> {
        let mut failures = Vec::new();
        for rollout in self.files.drain(..).rev() {
            match rollout.original_path.try_exists() {
                Ok(false) => {
                    if let Err(err) = std::fs::rename(&rollout.staged_path, &rollout.original_path)
                    {
                        failures.push(format!(
                            "`{}` to `{}`: {err}",
                            rollout.staged_path.display(),
                            rollout.original_path.display()
                        ));
                    }
                }
                Ok(true) => failures.push(format!(
                    "refusing to overwrite restored rollout `{}`",
                    rollout.original_path.display()
                )),
                Err(err) => failures.push(format!(
                    "check restored rollout `{}`: {err}",
                    rollout.original_path.display()
                )),
            }
        }
        if let Some(staging_dir) = self.staging_dir.take()
            && let Err(err) = std::fs::remove_dir(&staging_dir)
            && err.kind() != ErrorKind::NotFound
        {
            failures.push(format!("remove `{}`: {err}", staging_dir.display()));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(ThreadStoreError::Internal {
                message: failures.join(", "),
            })
        }
    }

    fn discard(mut self) {
        self.discard_inner();
        self.disposition = StagingDisposition::Complete;
    }

    fn discard_inner(&mut self) {
        self.files.clear();
        if let Some(staging_dir) = self.staging_dir.take()
            && let Err(err) = std::fs::remove_dir_all(&staging_dir)
            && err.kind() != ErrorKind::NotFound
        {
            warn!(
                "failed to discard deleted rollout files from `{}`: {err}",
                staging_dir.display()
            );
        }
    }
}

impl Drop for StagedRollouts {
    fn drop(&mut self) {
        match self.disposition {
            StagingDisposition::Restore => {
                if let Err(err) = self.restore_inner() {
                    warn!("failed to restore rollout files after interrupted deletion: {err}");
                }
            }
            StagingDisposition::Discard => self.discard_inner(),
            StagingDisposition::Complete => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use codex_protocol::ThreadId;
    use codex_protocol::protocol::ThreadHistoryMode;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;
    use uuid::Uuid;

    use super::*;
    use crate::ThreadStore;
    use crate::local::LocalThreadStore;
    use crate::local::test_support::test_config;
    use crate::local::test_support::write_archived_session_file;
    use crate::local::test_support::write_session_file;
    use crate::local::test_support::write_session_file_with_history_mode;

    #[tokio::test]
    async fn delete_thread_removes_active_and_archived_rollouts() {
        let home = TempDir::new().expect("temp dir");
        let store = LocalThreadStore::new(test_config(home.path()), /*state_db*/ None);
        let active_path =
            write_session_file(home.path(), "2025-01-03T12-00-00", Uuid::from_u128(301))
                .expect("session file");
        let compressed_path = active_path.with_extension("jsonl.zst");
        std::fs::write(&compressed_path, b"compressed sibling").expect("compressed sibling");
        let cases = [
            (Uuid::from_u128(301), active_path),
            (
                Uuid::from_u128(302),
                write_archived_session_file(
                    home.path(),
                    "2025-01-03T12-00-00",
                    Uuid::from_u128(302),
                )
                .expect("archived session file"),
            ),
        ];

        for (uuid, path) in cases {
            let thread_id = ThreadId::from_string(&uuid.to_string()).expect("valid thread id");
            store
                .delete_thread(DeleteThreadParams { thread_id })
                .await
                .expect("delete thread");

            assert!(!path.exists());
        }
        assert!(!compressed_path.exists());
    }

    #[tokio::test]
    async fn failed_state_delete_restores_all_rollouts() {
        let home = TempDir::new().expect("temp dir");
        let config = test_config(home.path());
        let runtime = codex_state::StateRuntime::init(
            config.sqlite_home.clone(),
            config.default_model_provider_id.clone(),
        )
        .await
        .expect("state db should initialize");
        let store = LocalThreadStore::new(config, Some(runtime.clone()));
        let thread_ids = [Uuid::from_u128(306), Uuid::from_u128(307)]
            .map(|uuid| ThreadId::from_string(&uuid.to_string()).expect("valid thread id"));
        let rollout_paths = [
            write_session_file(home.path(), "2025-01-03T12-00-00", Uuid::from_u128(306))
                .expect("first session file"),
            write_session_file(home.path(), "2025-01-03T12-00-01", Uuid::from_u128(307))
                .expect("second session file"),
        ];
        runtime.close().await;

        store
            .delete_threads(&thread_ids)
            .await
            .expect_err("closed state db should fail deletion");

        assert!(rollout_paths.into_iter().all(|path| path.exists()));
        assert!(
            !home
                .path()
                .join(".thread-delete-staging")
                .read_dir()
                .expect("staging root")
                .any(|_| true)
        );
    }

    #[tokio::test]
    async fn name_index_failure_does_not_fail_committed_delete() {
        let home = TempDir::new().expect("temp dir");
        let store = LocalThreadStore::new(test_config(home.path()), /*state_db*/ None);
        let uuid = Uuid::from_u128(308);
        let thread_id = ThreadId::from_string(&uuid.to_string()).expect("valid thread id");
        let rollout_path =
            write_session_file(home.path(), "2025-01-03T12-00-00", uuid).expect("session file");
        std::fs::create_dir(home.path().join("session_index.jsonl"))
            .expect("unreadable session index");

        store
            .delete_thread(DeleteThreadParams { thread_id })
            .await
            .expect("name index is derived state");

        assert!(!rollout_path.exists());
    }

    #[test]
    fn staging_guard_restores_uncommitted_rollout_on_drop() {
        let home = TempDir::new().expect("temp dir");
        let staging_dir = home.path().join("staging");
        let staged_path = staging_dir.join("0");
        let original_path = home.path().join("rollout.jsonl");
        std::fs::create_dir(&staging_dir).expect("staging dir");
        std::fs::write(&staged_path, b"rollout").expect("staged rollout");

        drop(StagedRollouts {
            staging_dir: Some(staging_dir.clone()),
            files: vec![StagedRollout {
                original_path: original_path.clone(),
                staged_path,
            }],
            disposition: StagingDisposition::Restore,
        });

        assert_eq!(
            std::fs::read(original_path).expect("restored rollout"),
            b"rollout"
        );
        assert!(!staging_dir.exists());
    }

    #[tokio::test]
    async fn delete_thread_removes_materialized_thread_history() {
        let home = TempDir::new().expect("temp dir");
        let store = LocalThreadStore::new(test_config(home.path()), /*state_db*/ None);
        let uuid = Uuid::from_u128(306);
        let thread_id = ThreadId::from_string(&uuid.to_string()).expect("valid thread id");
        let rollout_id = ThreadId::from_string(&Uuid::from_u128(309).to_string())
            .expect("valid rollout id");
        let ordinary_path = write_session_file_with_history_mode(
            home.path(),
            "2025-01-03T12-00-00",
            uuid,
            ThreadHistoryMode::Paginated,
        )
        .expect("session file");
        let rollout_path = ordinary_path.with_file_name(format!(
            "rollout-2025-01-03T12-00-00-{thread_id}_{rollout_id}.jsonl"
        ));
        std::fs::copy(&ordinary_path, &rollout_path).expect("replacement rollout");
        let pool = codex_state::open_thread_history_db(home.path())
            .await
            .expect("open thread history db");
        let rollout_id_string = rollout_id.to_string();
        sqlx::query(
            "INSERT INTO thread_turns (thread_id, turn_id, rollout_ordinal, status) VALUES (?, 'turn-1', 1, 'completed')",
        )
        .bind(rollout_id_string.as_str())
        .execute(&pool)
        .await
        .expect("insert turn");
        sqlx::query(
            "INSERT INTO thread_items (thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json) VALUES (?, 'turn-1', 'item-1', 2, 1, '{}')",
        )
        .bind(rollout_id_string.as_str())
        .execute(&pool)
        .await
        .expect("insert item");
        sqlx::query(
            "INSERT INTO thread_history_projection_state (thread_id, next_rollout_byte_offset, next_rollout_ordinal) VALUES (?, 3, 3)",
        )
        .bind(rollout_id_string.as_str())
        .execute(&pool)
        .await
        .expect("insert projection state");

        store
            .delete_thread(DeleteThreadParams { thread_id })
            .await
            .expect("delete thread");

        assert!(!ordinary_path.exists());
        assert!(!rollout_path.exists());

        let counts = sqlx::query_as::<_, (i64, i64, i64)>(
            r#"
SELECT
    (SELECT COUNT(*) FROM thread_turns WHERE thread_id = ?),
    (SELECT COUNT(*) FROM thread_items WHERE thread_id = ?),
    (SELECT COUNT(*) FROM thread_history_projection_state WHERE thread_id = ?)
            "#,
        )
        .bind(rollout_id_string.as_str())
        .bind(rollout_id_string.as_str())
        .bind(rollout_id_string.as_str())
        .fetch_one(&pool)
        .await
        .expect("read remaining history rows");
        assert_eq!(counts, (0, 0, 0));
    }

    #[tokio::test]
    async fn delete_thread_reports_missing_thread() {
        let home = TempDir::new().expect("temp dir");
        let store = LocalThreadStore::new(test_config(home.path()), /*state_db*/ None);
        let thread_id =
            ThreadId::from_string("00000000-0000-0000-0000-000000000304").expect("valid thread id");

        let err = store
            .delete_thread(DeleteThreadParams { thread_id })
            .await
            .expect_err("missing thread should fail");
        assert_eq!(
            err.to_string(),
            "thread 00000000-0000-0000-0000-000000000304 not found"
        );
    }
}
