use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use codex_protocol::RolloutId;
use codex_protocol::ThreadId;
use codex_rollout::RolloutRecorder;

use super::super::LocalThreadStore;
use super::super::thread_rollout_resolver::RolloutLocation;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

pub(super) async fn visible_rollout_paths(
    store: &LocalThreadStore,
    thread_id: ThreadId,
    selected_path: &Path,
    location: RolloutLocation,
) -> ThreadStoreResult<Vec<PathBuf>> {
    let paths = match location {
        RolloutLocation::Unarchived => {
            codex_rollout::find_thread_paths_by_id(store.config.codex_home.as_path(), thread_id)
                .await
        }
        RolloutLocation::Archived => {
            codex_rollout::find_archived_thread_paths_by_id(
                store.config.codex_home.as_path(),
                thread_id,
            )
            .await
        }
    };
    let mut paths = paths.map_err(|err| ThreadStoreError::Internal {
        message: format!("failed to inventory rollout heads for {thread_id}: {err}"),
    })?;
    let plain_selected = codex_rollout::plain_rollout_path(selected_path);
    for path in [
        selected_path.to_path_buf(),
        plain_selected.clone(),
        plain_selected.with_extension("jsonl.zst"),
    ] {
        match path.try_exists() {
            Ok(true) => paths.push(path),
            Ok(false) => {}
            Err(err) => {
                return Err(ThreadStoreError::Internal {
                    message: format!("failed to inspect rollout head {}: {err}", path.display()),
                });
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub(super) fn stable_rollout_path(
    thread_id: ThreadId,
    expected_path: &Path,
    source_path: &Path,
    visible_paths: &[PathBuf],
) -> ThreadStoreResult<PathBuf> {
    let expected_plain = codex_rollout::plain_rollout_path(expected_path);
    if codex_rollout::rollout_id_from_path(expected_plain.as_path()) == Some(thread_id) {
        return Ok(expected_plain);
    }
    if let Some(path) = visible_paths
        .iter()
        .map(|path| codex_rollout::plain_rollout_path(path.as_path()))
        .filter(|path| codex_rollout::rollout_id_from_path(path.as_path()) == Some(thread_id))
        .min()
    {
        return Ok(path);
    }
    codex_rollout::stable_rollout_path(source_path, thread_id).ok_or_else(|| {
        ThreadStoreError::InvalidRequest {
            message: format!(
                "paginated rollout path `{}` does not have a canonical rollout filename",
                source_path.display()
            ),
        }
    })
}

pub(super) async fn materialize_existing_stable_head(path: &Path) -> ThreadStoreResult<()> {
    let Some(existing) = codex_rollout::existing_rollout_path(path).await else {
        return Ok(());
    };
    if existing == path {
        return Ok(());
    }
    codex_rollout::materialize_rollout_for_reference(existing.as_path())
        .await
        .map(|_| ())
        .map_err(|err| ThreadStoreError::Internal {
            message: format!(
                "failed to materialize stable rollout head {}: {err}",
                existing.display()
            ),
        })
}

pub(super) async fn preserve_visible_rollouts(
    store: &LocalThreadStore,
    thread_id: ThreadId,
    paths: &[PathBuf],
) -> ThreadStoreResult<()> {
    let revision_dir = store
        .config
        .codex_home
        .join(codex_rollout::ROLLOUT_REVISIONS_SUBDIR)
        .join(thread_id.to_string());
    std::fs::create_dir_all(revision_dir.as_path()).map_err(|err| ThreadStoreError::Internal {
        message: format!(
            "failed to create rollout revision directory {}: {err}",
            revision_dir.display()
        ),
    })?;
    for path in paths {
        let meta = codex_rollout::read_session_meta_line(path.as_path())
            .await
            .map_err(|err| ThreadStoreError::Internal {
                message: format!("failed to read rollout revision {}: {err}", path.display()),
            })?;
        if meta.meta.id != thread_id {
            return Err(ThreadStoreError::InvalidRequest {
                message: format!(
                    "rollout at {} belongs to thread {}, not {thread_id}",
                    path.display(),
                    meta.meta.id
                ),
            });
        }
        let rollout_id = super::super::thread_rollout_resolver::rollout_id_for_thread_path(
            path.as_path(),
            thread_id,
            meta.meta.history_mode,
        )
        .await?;
        let extension = if path.extension().is_some_and(|extension| extension == "zst") {
            "jsonl.zst"
        } else {
            "jsonl"
        };
        let destination = revision_dir.join(format!("{rollout_id}.{extension}"));
        replace_with_snapshot(path.as_path(), destination.as_path())?;
    }
    Ok(())
}

fn replace_with_snapshot(source: &Path, destination: &Path) -> ThreadStoreResult<()> {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ThreadStoreError::Internal {
            message: format!(
                "rollout revision path has no file name: {}",
                destination.display()
            ),
        })?;
    let temporary = destination.with_file_name(format!(".{file_name}.{}.tmp", RolloutId::new()));
    match std::fs::hard_link(source, temporary.as_path()) {
        Ok(()) => {}
        Err(_) => {
            std::fs::copy(source, temporary.as_path()).map_err(|err| {
                ThreadStoreError::Internal {
                    message: format!(
                        "failed to preserve rollout revision {}: {err}",
                        source.display()
                    ),
                }
            })?;
            std::fs::File::open(temporary.as_path())
                .and_then(|file| file.sync_all())
                .map_err(|err| ThreadStoreError::Internal {
                    message: format!(
                        "failed to sync rollout revision {}: {err}",
                        temporary.display()
                    ),
                })?;
        }
    }
    if let Err(err) = std::fs::rename(temporary.as_path(), destination) {
        let _ = std::fs::remove_file(temporary.as_path());
        return Err(ThreadStoreError::Internal {
            message: format!(
                "failed to publish rollout revision {}: {err}",
                destination.display()
            ),
        });
    }
    Ok(())
}

pub(super) async fn abandon_replacement(
    recorder: &RolloutRecorder,
    path: &Path,
    err: ThreadStoreError,
) -> ThreadStoreError {
    let mut message = err.to_string();
    if let Err(shutdown_error) = recorder.shutdown().await {
        message.push_str(format!("; shutdown failed: {shutdown_error}").as_str());
    }
    if let Err(cleanup_error) = remove_failed_replacement(path).await {
        message.push_str(format!("; cleanup failed: {cleanup_error}").as_str());
    }
    ThreadStoreError::Internal { message }
}

pub(super) async fn persist_replacement(
    recorder: &RolloutRecorder,
    path: &Path,
) -> ThreadStoreResult<()> {
    let persist_error = recorder.persist().await.err();
    let shutdown_error = recorder.shutdown().await.err();
    let Some(message) = replacement_write_error(persist_error, shutdown_error) else {
        return Ok(());
    };
    match remove_failed_replacement(path).await {
        Ok(()) => Err(ThreadStoreError::Internal { message }),
        Err(cleanup_error) => Err(ThreadStoreError::Internal {
            message: format!("{message}; cleanup failed: {cleanup_error}"),
        }),
    }
}

pub(super) fn replacement_write_error(
    persist_error: Option<std::io::Error>,
    shutdown_error: Option<std::io::Error>,
) -> Option<String> {
    match (persist_error, shutdown_error) {
        (None, None) => None,
        (Some(persist_error), None) => Some(format!(
            "failed to persist replacement rollout: {persist_error}"
        )),
        (None, Some(shutdown_error)) => Some(format!(
            "failed to shut down replacement rollout: {shutdown_error}"
        )),
        (Some(persist_error), Some(shutdown_error)) => Some(format!(
            "failed to persist replacement rollout: {persist_error}; shutdown failed: {shutdown_error}"
        )),
    }
}

pub(super) async fn publish_replacement(
    store: &LocalThreadStore,
    state_db: &codex_rollout::StateDbHandle,
    thread_id: ThreadId,
    expected_path: &Path,
    replacement_path: &Path,
    stable_path: &Path,
    visible_paths: &[PathBuf],
) -> ThreadStoreResult<()> {
    publish_head(
        store,
        state_db,
        thread_id,
        expected_path,
        replacement_path,
        stable_path,
    )
    .await?;
    retire_visible_rollouts(visible_paths, stable_path)
}

pub(super) async fn publish_head(
    store: &LocalThreadStore,
    state_db: &codex_rollout::StateDbHandle,
    thread_id: ThreadId,
    expected_path: &Path,
    replacement_path: &Path,
    stable_path: &Path,
) -> ThreadStoreResult<()> {
    let rollback = replace_stable_head(store, replacement_path, stable_path)?;

    let selection = state_db
        .replace_rollout_path_if_current(thread_id, expected_path, stable_path)
        .await;
    match selection {
        Ok(true) => {
            rollback.commit();
            Ok(())
        }
        Ok(false) => Err(rollback.fail(ThreadStoreError::Conflict {
            message: format!("thread {thread_id} changed while it was being reverted"),
        })),
        Err(err) => Err(rollback.fail(ThreadStoreError::Internal {
            message: format!("failed to switch thread {thread_id} to reverted rollout: {err}"),
        })),
    }
}

pub(super) fn publish_head_without_state_db(
    store: &LocalThreadStore,
    replacement_path: &Path,
    stable_path: &Path,
) -> ThreadStoreResult<()> {
    replace_stable_head(store, replacement_path, stable_path)?.commit();
    Ok(())
}

fn replace_stable_head(
    store: &LocalThreadStore,
    replacement_path: &Path,
    stable_path: &Path,
) -> ThreadStoreResult<StableHeadRollback> {
    let compressed_stable_path = stable_path.with_extension("jsonl.zst");
    let rollback = StableHeadRollback::capture(
        store.config.codex_home.as_path(),
        [stable_path, compressed_stable_path.as_path()],
    )?;
    if let Err(err) = std::fs::remove_file(compressed_stable_path.as_path())
        && err.kind() != ErrorKind::NotFound
    {
        return Err(rollback.fail(ThreadStoreError::Internal {
            message: format!(
                "failed to retire compressed stable rollout {}: {err}",
                compressed_stable_path.display()
            ),
        }));
    }
    if let Err(err) = std::fs::rename(replacement_path, stable_path) {
        return Err(rollback.fail(ThreadStoreError::Internal {
            message: format!(
                "failed to publish stable reverted rollout {}: {err}",
                stable_path.display()
            ),
        }));
    }
    Ok(rollback)
}

pub(super) fn retire_visible_rollouts(
    paths: &[PathBuf],
    stable_path: &Path,
) -> ThreadStoreResult<()> {
    let mut failures = Vec::new();
    let canonical_stable_path =
        std::fs::canonicalize(stable_path).map_err(|err| ThreadStoreError::Internal {
            message: format!(
                "failed to resolve published rollout {}: {err}",
                stable_path.display()
            ),
        })?;
    for path in paths {
        if path == stable_path
            || std::fs::canonicalize(path)
                .is_ok_and(|canonical_path| canonical_path == canonical_stable_path)
        {
            continue;
        }
        if let Err(err) = std::fs::remove_file(path)
            && err.kind() != ErrorKind::NotFound
        {
            failures.push(format!("{}: {err}", path.display()));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ThreadStoreError::Internal {
            message: format!(
                "reverted rollout was published, but old heads could not be retired: {}",
                failures.join(", ")
            ),
        })
    }
}

struct StableHeadRollback {
    directory: PathBuf,
    files: Vec<RollbackFile>,
}

struct RollbackFile {
    original: PathBuf,
    backup: Option<PathBuf>,
}

impl StableHeadRollback {
    fn capture<'a>(
        codex_home: &Path,
        paths: impl IntoIterator<Item = &'a Path>,
    ) -> ThreadStoreResult<Self> {
        let directory = codex_home
            .join(codex_rollout::ROLLOUT_REVISIONS_SUBDIR)
            .join(".staging")
            .join(format!("rollback-{}", RolloutId::new()));
        std::fs::create_dir_all(directory.as_path()).map_err(|err| ThreadStoreError::Internal {
            message: format!(
                "failed to create revert rollback directory {}: {err}",
                directory.display()
            ),
        })?;
        let mut files = Vec::new();
        for (index, original) in paths.into_iter().enumerate() {
            let backup = match original.try_exists() {
                Ok(true) => {
                    let backup = directory.join(index.to_string());
                    match std::fs::hard_link(original, backup.as_path()) {
                        Ok(()) => {}
                        Err(_) => {
                            std::fs::copy(original, backup.as_path()).map_err(|err| {
                                ThreadStoreError::Internal {
                                    message: format!(
                                        "failed to stage revert rollback for {}: {err}",
                                        original.display()
                                    ),
                                }
                            })?;
                        }
                    }
                    Some(backup)
                }
                Ok(false) => None,
                Err(err) => {
                    return Err(ThreadStoreError::Internal {
                        message: format!(
                            "failed to inspect stable rollout {}: {err}",
                            original.display()
                        ),
                    });
                }
            };
            files.push(RollbackFile {
                original: original.to_path_buf(),
                backup,
            });
        }
        Ok(Self { directory, files })
    }

    fn fail(mut self, error: ThreadStoreError) -> ThreadStoreError {
        match self.restore() {
            Ok(()) => error,
            Err(restore_error) => ThreadStoreError::Internal {
                message: format!("{error}; failed to restore prior rollout head: {restore_error}"),
            },
        }
    }

    fn restore(&mut self) -> ThreadStoreResult<()> {
        let mut failures = Vec::new();
        for file in &self.files {
            match file.backup.as_ref() {
                Some(backup) => {
                    if let Err(err) = std::fs::rename(backup, file.original.as_path()) {
                        failures.push(format!(
                            "{} to {}: {err}",
                            backup.display(),
                            file.original.display()
                        ));
                    }
                }
                None => {
                    if let Err(err) = std::fs::remove_file(file.original.as_path())
                        && err.kind() != ErrorKind::NotFound
                    {
                        failures.push(format!("{}: {err}", file.original.display()));
                    }
                }
            }
        }
        if let Err(err) = std::fs::remove_dir_all(self.directory.as_path())
            && err.kind() != ErrorKind::NotFound
        {
            failures.push(format!("{}: {err}", self.directory.display()));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(ThreadStoreError::Internal {
                message: failures.join(", "),
            })
        }
    }

    fn commit(self) {
        if let Err(err) = std::fs::remove_dir_all(self.directory.as_path())
            && err.kind() != ErrorKind::NotFound
        {
            tracing::warn!(
                "failed to discard revert rollback directory {}: {err}",
                self.directory.display()
            );
        }
    }
}

pub(super) async fn remove_failed_replacement(path: &Path) -> ThreadStoreResult<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(ThreadStoreError::Internal {
            message: format!(
                "failed to remove unpublished replacement rollout {}: {err}",
                path.display()
            ),
        }),
    }
}
