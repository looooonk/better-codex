use std::path::Path;

use codex_protocol::RolloutId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::HistoryPosition;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_rollout::RolloutConfig;
use codex_rollout::RolloutRecorder;
use codex_rollout::RolloutRecorderParams;

use super::LocalThreadStore;
use super::rollout_lineage::RolloutLineage;
use super::thread_rollout_resolver;
use crate::RevertThreadParams;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

/// Revert an idle paginated thread by selecting a new immutable rollout head.
pub(super) async fn revert(
    store: &LocalThreadStore,
    params: RevertThreadParams,
) -> ThreadStoreResult<()> {
    let RevertThreadParams {
        thread_id,
        before_turn_id,
    } = params;
    let state_db = store
        .state_db()
        .await
        .ok_or(ThreadStoreError::Unsupported {
            operation: "revert_thread",
        })?;
    let _lifecycle_guard = store.live_writer_locks.lock_lifecycle(thread_id).await;
    let _live_writer_guard = store.live_writer_locks.lock(thread_id).await;
    store.ensure_live_recorder_absent(thread_id).await?;
    let _writer_lock = store.writer_lock_coordinator.acquire(thread_id)?;

    let expected_sqlite_path = state_db
        .get_thread(thread_id)
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!("failed to read thread metadata for {thread_id}: {err}"),
        })?
        .ok_or(ThreadStoreError::ThreadNotFound { thread_id })?
        .rollout_path;
    let current_rollout = thread_rollout_resolver::resolve_current(store, thread_id)
        .await?
        .ok_or(ThreadStoreError::ThreadNotFound { thread_id })?;
    let source_path = super::helpers::scoped_rollout_path(
        store.config.codex_home.clone(),
        current_rollout.path.as_path(),
        "Codex home",
    )?;
    let source_meta = codex_rollout::read_session_meta_line(source_path.as_path())
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!(
                "failed to read current paginated rollout {}: {err}",
                source_path.display()
            ),
        })?
        .meta;
    if source_meta.id != thread_id {
        return Err(ThreadStoreError::InvalidRequest {
            message: format!("current rollout for {thread_id} belongs to another thread"),
        });
    }
    if source_meta.history_mode != ThreadHistoryMode::Paginated {
        return Err(ThreadStoreError::InvalidRequest {
            message: format!("thread {thread_id} does not use paginated history"),
        });
    }

    let mut lineage = store.resolve_rollout_lineage(thread_id).await?;
    for segment in &mut lineage.segments {
        let path = super::helpers::scoped_rollout_path(
            store.config.codex_home.clone(),
            segment.rollout_path.as_path(),
            "Codex home",
        )?;
        segment.rollout_path = codex_rollout::materialize_rollout_for_reference(path.as_path())
            .await
            .map_err(|err| ThreadStoreError::Internal {
                message: format!("failed to materialize rollout {} for revert: {err}", path.display()),
            })?;
        super::thread_history_materialization::materialize_to_sqlite(
            store,
            segment.rollout_id(),
            segment.rollout_path.as_path(),
        )
        .await?;
    }
    let history_base = history_base_before_turn(store, &lineage, before_turn_id.as_str()).await?;

    let rollout_id = RolloutId::new();
    let recorder = create_replacement_recorder(store, source_meta, rollout_id, history_base).await?;
    let replacement_path = recorder.rollout_path().to_path_buf();
    persist_replacement(&recorder, replacement_path.as_path()).await?;
    publish_replacement(
        &state_db,
        thread_id,
        expected_sqlite_path.as_path(),
        replacement_path.as_path(),
    )
    .await
}

async fn persist_replacement(recorder: &RolloutRecorder, path: &Path) -> ThreadStoreResult<()> {
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

fn replacement_write_error(
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

async fn history_base_before_turn(
    store: &LocalThreadStore,
    lineage: &RolloutLineage,
    turn_id: &str,
) -> ThreadStoreResult<Option<HistoryPosition>> {
    let pool = store.thread_history_db().await?;
    let row = super::thread_history::find_source_turn(pool, lineage, turn_id).await?;
    if row.rollout_end_ordinal == Some(row.rollout_ordinal) {
        return Err(ThreadStoreError::InvalidRequest {
            message: format!("turn {turn_id} does not have a persisted start boundary"),
        });
    }
    let position = HistoryPosition {
        thread_id: row.rollout_id,
        end_ordinal_exclusive: u64::try_from(row.rollout_ordinal)
            .map_err(|_| invalid_turn_position(turn_id))?,
        end_byte_offset: u64::try_from(
            row.rollout_byte_offset
                .ok_or_else(|| missing_turn_position(turn_id))?,
        )
        .map_err(|_| invalid_turn_position(turn_id))?,
    };
    let segment_index = lineage
        .segments()
        .iter()
        .position(|segment| segment.rollout_id() == position.thread_id)
        .ok_or_else(|| ThreadStoreError::Internal {
            message: "revert position is outside the selected rollout lineage".to_string(),
        })?;
    if lineage.segments()[segment_index].end.is_some_and(|end| {
        position.end_ordinal_exclusive > end.end_ordinal_exclusive
            || position.end_byte_offset > end.end_byte_offset
    }) {
        return Err(ThreadStoreError::InvalidRequest {
            message: "revert boundary exceeds inherited source history".to_string(),
        });
    }
    if position.end_ordinal_exclusive == lineage.segments()[segment_index].start_ordinal() {
        return Ok(segment_index
            .checked_sub(1)
            .and_then(|index| lineage.segments()[index].end));
    }
    Ok(Some(position))
}

async fn create_replacement_recorder(
    store: &LocalThreadStore,
    source_meta: codex_rollout::SessionMeta,
    rollout_id: RolloutId,
    history_base: Option<HistoryPosition>,
) -> ThreadStoreResult<RolloutRecorder> {
    let config = RolloutConfig {
        codex_home: store.config.codex_home.clone(),
        sqlite_home: store.config.sqlite_home.clone(),
        cwd: source_meta.cwd.clone(),
        model_provider_id: source_meta
            .model_provider
            .clone()
            .unwrap_or_else(|| store.config.default_model_provider_id.clone()),
        generate_memories: source_meta.memory_mode.as_deref() != Some("disabled"),
    };
    let mut params = RolloutRecorderParams::new(
        source_meta.id,
        source_meta.forked_from_id,
        source_meta.parent_thread_id,
        source_meta.source,
        source_meta.thread_source,
        source_meta.originator,
        source_meta.base_instructions.unwrap_or_default(),
        source_meta.dynamic_tools.unwrap_or_default(),
    )
    .with_session_id(source_meta.session_id)
    .with_rollout_id(rollout_id)
    .with_selected_capability_roots(source_meta.selected_capability_roots)
    .with_multi_agent_version(source_meta.multi_agent_version)
    .with_history_mode(ThreadHistoryMode::Paginated)
    .with_history_base(history_base)
    .with_subagent_history_start_ordinal(source_meta.subagent_history_start_ordinal);
    if let Some(context_window) = source_meta.context_window {
        params = params.with_initial_window_id(context_window.window_id);
    }
    RolloutRecorder::new(&config, params)
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!("failed to create reverted rollout: {err}"),
        })
}

async fn publish_replacement(
    state_db: &codex_rollout::StateDbHandle,
    thread_id: ThreadId,
    expected_path: &Path,
    replacement_path: &Path,
) -> ThreadStoreResult<()> {
    match state_db
        .replace_rollout_path_if_current(thread_id, expected_path, replacement_path)
        .await
    {
        Ok(true) => Ok(()),
        Ok(false) => {
            remove_failed_replacement(replacement_path).await?;
            Err(ThreadStoreError::Conflict {
                message: format!("thread {thread_id} changed while it was being reverted"),
            })
        }
        Err(err) => {
            let message = format!("failed to switch thread {thread_id} to reverted rollout: {err}");
            match remove_failed_replacement(replacement_path).await {
                Ok(()) => Err(ThreadStoreError::Internal { message }),
                Err(cleanup_error) => Err(ThreadStoreError::Internal {
                    message: format!("{message}; cleanup failed: {cleanup_error}"),
                }),
            }
        }
    }
}

async fn remove_failed_replacement(path: &Path) -> ThreadStoreResult<()> {
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

fn missing_turn_position(turn_id: &str) -> ThreadStoreError {
    ThreadStoreError::InvalidRequest {
        message: format!("turn {turn_id} does not have persisted rollout positions"),
    }
}

fn invalid_turn_position(turn_id: &str) -> ThreadStoreError {
    ThreadStoreError::Internal {
        message: format!("invalid rollout position for turn {turn_id}"),
    }
}

#[cfg(test)]
#[path = "revert_thread_tests.rs"]
mod tests;
