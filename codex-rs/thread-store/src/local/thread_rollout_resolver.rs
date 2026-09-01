use std::path::Path;
use std::path::PathBuf;

use codex_protocol::RolloutId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_rollout::find_archived_thread_path_by_id_str;
use codex_rollout::find_thread_path_by_id_str;

use super::LocalThreadStore;
use super::helpers::rollout_path_is_archived;
use super::live_writer;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ResolvedThreadRollout {
    pub(super) thread_id: ThreadId,
    pub(super) rollout_id: RolloutId,
    pub(super) path: PathBuf,
    pub(super) location: RolloutLocation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RolloutLocation {
    Unarchived,
    Archived,
}

#[cfg(test)]
#[path = "thread_rollout_resolver_tests.rs"]
mod tests;

pub(super) async fn resolve_current(
    store: &LocalThreadStore,
    thread_id: ThreadId,
) -> ThreadStoreResult<Option<ResolvedThreadRollout>> {
    resolve(store, thread_id, LookupScope::ExcludeArchived).await
}

pub(super) async fn resolve_current_including_archived(
    store: &LocalThreadStore,
    thread_id: ThreadId,
) -> ThreadStoreResult<Option<ResolvedThreadRollout>> {
    resolve(store, thread_id, LookupScope::IncludeArchived).await
}

#[derive(Clone, Copy)]
enum LookupScope {
    ExcludeArchived,
    IncludeArchived,
}

impl LookupScope {
    fn accepts(self, location: RolloutLocation) -> bool {
        match self {
            Self::ExcludeArchived => location == RolloutLocation::Unarchived,
            Self::IncludeArchived => true,
        }
    }
}

async fn resolve(
    store: &LocalThreadStore,
    thread_id: ThreadId,
    scope: LookupScope,
) -> ThreadStoreResult<Option<ResolvedThreadRollout>> {
    if let Ok((path, rollout_id, history_mode)) =
        live_writer::rollout_identity(store, thread_id).await
        && let Some(path) = codex_rollout::existing_rollout_path(path.as_path()).await
    {
        let location = location_for_path(store, path.as_path());
        if !scope.accepts(location) {
            return Ok(None);
        }
        validate_thread_path(path.as_path(), thread_id, history_mode).await?;
        return Ok(Some(ResolvedThreadRollout {
            thread_id,
            rollout_id,
            path,
            location,
        }));
    }

    let state_db = store.state_db().await;
    let mut recovery_only = false;
    let mut recovery_error = None;
    if let Some(state_db) = state_db.as_deref() {
        match state_db.get_thread(thread_id).await {
            Ok(Some(metadata)) => {
                if let Some(path) =
                    codex_rollout::existing_rollout_path(metadata.rollout_path.as_path()).await
                {
                    let location = if metadata.archived_at.is_some() {
                        RolloutLocation::Archived
                    } else {
                        location_for_path(store, path.as_path())
                    };
                    if !scope.accepts(location) {
                        return Ok(None);
                    }
                    if metadata.history_mode == ThreadHistoryMode::Legacy
                        && matches!(
                            codex_rollout::read_session_meta_line(path.as_path()).await,
                            Ok(meta) if meta.meta.id != thread_id
                        )
                    {
                        tracing::warn!(
                            thread_id = %thread_id,
                            rollout_path = %path.display(),
                            "ignoring stale legacy rollout selection owned by another thread"
                        );
                    } else {
                        match rollout_id_for_thread_path(
                            path.as_path(),
                            thread_id,
                            metadata.history_mode,
                        )
                        .await
                        {
                            Ok(rollout_id) => {
                                return Ok(Some(ResolvedThreadRollout {
                                    thread_id,
                                    rollout_id,
                                    path,
                                    location,
                                }));
                            }
                            Err(err) if metadata.history_mode == ThreadHistoryMode::Paginated => {
                                recovery_only = true;
                                recovery_error = Some(err);
                            }
                            Err(err) => return Err(err),
                        }
                    }
                }
                if metadata.history_mode == ThreadHistoryMode::Paginated {
                    recovery_only = true;
                }
            }
            Ok(None) => {}
            Err(err) => {
                recovery_only = true;
                recovery_error = Some(ThreadStoreError::Internal {
                    message: format!("failed to read thread metadata for {thread_id}: {err}"),
                });
            }
        }
    }

    if let Some(path) = find_thread_path_by_id_str(
        store.config.codex_home.as_path(),
        &thread_id.to_string(),
        if recovery_only {
            None
        } else {
            state_db.as_deref()
        },
    )
    .await
    .map_err(|err| lookup_error(thread_id, err))?
    {
        let resolved = resolve_fallback_path(store, thread_id, path, scope).await?;
        if !recovery_only
            || resolved
                .as_ref()
                .is_some_and(|resolved| resolved.rollout_id != thread_id)
        {
            return Ok(resolved);
        }
    }
    if !scope.accepts(RolloutLocation::Archived) {
        return finish_recovery(recovery_error);
    }
    let path = find_archived_thread_path_by_id_str(
        store.config.codex_home.as_path(),
        &thread_id.to_string(),
        if recovery_only {
            None
        } else {
            state_db.as_deref()
        },
    )
    .await
    .map_err(|err| lookup_error(thread_id, err))?;
    match path {
        Some(path) => {
            let resolved = resolve_fallback_path(store, thread_id, path, scope).await?;
            if !recovery_only
                || resolved
                    .as_ref()
                    .is_some_and(|resolved| resolved.rollout_id != thread_id)
            {
                Ok(resolved)
            } else {
                finish_recovery(recovery_error)
            }
        }
        None => finish_recovery(recovery_error),
    }
}

fn finish_recovery(
    recovery_error: Option<ThreadStoreError>,
) -> ThreadStoreResult<Option<ResolvedThreadRollout>> {
    match recovery_error {
        Some(err) => Err(err),
        None => Ok(None),
    }
}

async fn resolve_fallback_path(
    store: &LocalThreadStore,
    thread_id: ThreadId,
    path: PathBuf,
    scope: LookupScope,
) -> ThreadStoreResult<Option<ResolvedThreadRollout>> {
    let location = location_for_path(store, path.as_path());
    if !scope.accepts(location) {
        return Ok(None);
    }
    let rollout_id = match codex_rollout::read_session_meta_line(path.as_path()).await {
        Ok(meta) => {
            validate_session_meta(
                path.as_path(),
                thread_id,
                meta.meta.id,
                None,
                meta.meta.history_mode,
            )?;
            rollout_id_from_path_or_legacy_thread_id(
                path.as_path(),
                thread_id,
                meta.meta.history_mode,
                meta.meta.rollout_id,
            )?
        }
        Err(_) => rollout_id_from_unreadable_legacy_path(path.as_path(), thread_id)?,
    };
    Ok(Some(ResolvedThreadRollout {
        thread_id,
        rollout_id,
        path,
        location,
    }))
}

fn location_for_path(store: &LocalThreadStore, path: &Path) -> RolloutLocation {
    if rollout_path_is_archived(store.config.codex_home.as_path(), path) {
        RolloutLocation::Archived
    } else {
        RolloutLocation::Unarchived
    }
}

pub(super) async fn rollout_id_for_thread_path(
    path: &Path,
    thread_id: ThreadId,
    history_mode: ThreadHistoryMode,
) -> ThreadStoreResult<RolloutId> {
    match codex_rollout::read_session_meta_line(path).await {
        Ok(meta) => {
            validate_session_meta(
                path,
                thread_id,
                meta.meta.id,
                Some(history_mode),
                meta.meta.history_mode,
            )?;
            rollout_id_from_path_or_legacy_thread_id(
                path,
                thread_id,
                history_mode,
                meta.meta.rollout_id,
            )
        }
        Err(_) if history_mode == ThreadHistoryMode::Legacy => {
            rollout_id_from_unreadable_legacy_path(path, thread_id)
        }
        Err(err) => Err(ThreadStoreError::InvalidRequest {
            message: format!(
                "failed to read paginated rollout metadata {}: {err}",
                path.display()
            ),
        }),
    }
}

async fn validate_thread_path(
    path: &Path,
    thread_id: ThreadId,
    history_mode: ThreadHistoryMode,
) -> ThreadStoreResult<()> {
    rollout_id_for_thread_path(path, thread_id, history_mode)
        .await
        .map(|_| ())
}

fn validate_session_meta(
    path: &Path,
    thread_id: ThreadId,
    actual_thread_id: ThreadId,
    expected_history_mode: Option<ThreadHistoryMode>,
    actual_history_mode: ThreadHistoryMode,
) -> ThreadStoreResult<()> {
    if actual_thread_id != thread_id {
        return Err(ThreadStoreError::InvalidRequest {
            message: format!(
                "rollout at {} belongs to thread {actual_thread_id}, not {thread_id}",
                path.display()
            ),
        });
    }
    if expected_history_mode.is_some_and(|expected| expected != actual_history_mode) {
        return Err(ThreadStoreError::InvalidRequest {
            message: format!(
                "rollout history mode disagrees with metadata: {}",
                path.display()
            ),
        });
    }
    Ok(())
}

fn rollout_id_from_unreadable_legacy_path(
    path: &Path,
    thread_id: ThreadId,
) -> ThreadStoreResult<RolloutId> {
    match codex_rollout::rollout_id_from_path(path) {
        Some(rollout_id) if rollout_id != thread_id => Err(ThreadStoreError::InvalidRequest {
            message: format!(
                "replacement rollout at {} does not have readable session metadata",
                path.display()
            ),
        }),
        Some(rollout_id) => Ok(rollout_id),
        None => Ok(thread_id),
    }
}

pub(super) fn rollout_id_from_path_or_legacy_thread_id(
    path: &Path,
    thread_id: ThreadId,
    history_mode: ThreadHistoryMode,
    metadata_rollout_id: Option<RolloutId>,
) -> ThreadStoreResult<RolloutId> {
    match (
        codex_rollout::rollout_id_from_path(path),
        metadata_rollout_id,
    ) {
        (Some(path_rollout_id), Some(metadata_rollout_id))
            if path_rollout_id != thread_id && path_rollout_id != metadata_rollout_id =>
        {
            Err(ThreadStoreError::InvalidRequest {
                message: format!(
                    "rollout identity disagrees with canonical filename: {}",
                    path.display()
                ),
            })
        }
        (Some(_), Some(metadata_rollout_id)) => Ok(metadata_rollout_id),
        (Some(path_rollout_id), None) => Ok(path_rollout_id),
        (None, Some(metadata_rollout_id)) if history_mode == ThreadHistoryMode::Legacy => {
            Ok(metadata_rollout_id)
        }
        (None, None) if history_mode == ThreadHistoryMode::Legacy => Ok(thread_id),
        (None, _) => Err(ThreadStoreError::InvalidRequest {
            message: format!(
                "paginated rollout path `{}` does not have a canonical rollout filename",
                path.display()
            ),
        }),
    }
}

fn lookup_error(thread_id: ThreadId, err: std::io::Error) -> ThreadStoreError {
    ThreadStoreError::InvalidRequest {
        message: format!("failed to locate thread id {thread_id}: {err}"),
    }
}
