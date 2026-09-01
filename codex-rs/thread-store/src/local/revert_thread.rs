use std::path::Path;
use std::path::PathBuf;

use codex_protocol::RolloutId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_rollout::RolloutConfig;
use codex_rollout::RolloutRecorder;
use codex_rollout::RolloutRecorderParams;

use self::history::copy_history_prefix;
use self::history::full_history_end;
use self::history::history_end_before_turn;
use self::publication::abandon_replacement;
use self::publication::materialize_existing_stable_head;
use self::publication::persist_replacement;
use self::publication::preserve_visible_rollouts;
use self::publication::publish_head;
use self::publication::publish_head_without_state_db;
use self::publication::publish_replacement;
use self::publication::remove_failed_replacement;
use self::publication::retire_visible_rollouts;
use self::publication::stable_rollout_path;
use self::publication::visible_rollout_paths;
use super::LocalThreadStore;
use super::rollout_lineage::RolloutLineage;
use super::thread_rollout_resolver;
use super::thread_rollout_resolver::RolloutLocation;
use crate::RevertThreadParams;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

mod history;
mod publication;

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
                message: format!(
                    "failed to materialize rollout {} for revert: {err}",
                    path.display()
                ),
            })?;
        super::thread_history_materialization::materialize_to_sqlite(
            store,
            segment.rollout_id(),
            segment.rollout_path.as_path(),
        )
        .await?;
    }
    let history_end = history_end_before_turn(store, &lineage, before_turn_id.as_str()).await?;

    let rollout_id = RolloutId::new();
    let recorder = create_replacement_recorder(store, source_meta.clone(), rollout_id).await?;
    let replacement_path = recorder.rollout_path().to_path_buf();
    // Older readers only inspect the selected head, so keep every retained item in that file.
    if let Err(err) = copy_history_prefix(&recorder, &lineage, history_end).await {
        return Err(abandon_replacement(&recorder, replacement_path.as_path(), err).await);
    }
    persist_replacement(&recorder, replacement_path.as_path()).await?;
    let result = async {
        let visible_paths = visible_rollout_paths(
            store,
            thread_id,
            source_path.as_path(),
            RolloutLocation::Unarchived,
        )
        .await?;
        let stable_path = stable_rollout_path(
            thread_id,
            expected_sqlite_path.as_path(),
            source_path.as_path(),
            visible_paths.as_slice(),
        )?;
        materialize_existing_stable_head(stable_path.as_path()).await?;
        let visible_paths = visible_rollout_paths(
            store,
            thread_id,
            source_path.as_path(),
            RolloutLocation::Unarchived,
        )
        .await?;
        preserve_visible_rollouts(store, thread_id, visible_paths.as_slice()).await?;
        let final_expected_path =
            if is_composite_selection(expected_sqlite_path.as_path(), thread_id) {
                normalize_composite_selection(
                    store,
                    &state_db,
                    thread_id,
                    source_meta,
                    &lineage,
                    expected_sqlite_path.as_path(),
                    stable_path.as_path(),
                )
                .await?;
                stable_path.clone()
            } else {
                expected_sqlite_path.clone()
            };
        publish_replacement(
            store,
            &state_db,
            thread_id,
            final_expected_path.as_path(),
            replacement_path.as_path(),
            stable_path.as_path(),
            visible_paths.as_slice(),
        )
        .await
    }
    .await;
    if result.is_err() {
        remove_failed_replacement(replacement_path.as_path()).await?;
    }
    result
}

fn is_composite_selection(path: &Path, thread_id: ThreadId) -> bool {
    codex_rollout::thread_id_from_rollout_path(path) == Some(thread_id)
        && codex_rollout::rollout_id_from_path(path)
            .is_some_and(|rollout_id| rollout_id != thread_id)
}

pub(super) async fn repair_composite_selection(
    store: &LocalThreadStore,
    selected: thread_rollout_resolver::ResolvedThreadRollout,
) -> ThreadStoreResult<thread_rollout_resolver::ResolvedThreadRollout> {
    let thread_id = selected.thread_id;
    if !is_composite_selection(selected.path.as_path(), thread_id) {
        return Ok(selected);
    }
    let source_path = super::helpers::scoped_rollout_path(
        store.config.codex_home.clone(),
        selected.path.as_path(),
        "Codex home",
    )?;
    let source_meta = codex_rollout::read_session_meta_line(source_path.as_path())
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!(
                "failed to read composite rollout {}: {err}",
                source_path.display()
            ),
        })?
        .meta;
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
                message: format!(
                    "failed to materialize rollout {} for repair: {err}",
                    path.display()
                ),
            })?;
    }
    let visible_paths =
        visible_rollout_paths(store, thread_id, source_path.as_path(), selected.location).await?;
    let stable_path = stable_rollout_path(
        thread_id,
        source_path.as_path(),
        source_path.as_path(),
        visible_paths.as_slice(),
    )?;
    materialize_existing_stable_head(stable_path.as_path()).await?;
    let visible_paths =
        visible_rollout_paths(store, thread_id, source_path.as_path(), selected.location).await?;
    preserve_visible_rollouts(store, thread_id, visible_paths.as_slice()).await?;

    let normalized_path = create_normalized_replacement(store, source_meta, &lineage).await?;
    let state_db = store.state_db().await;
    let expected_path = if let Some(state_db) = state_db.as_ref() {
        match state_db.get_thread(thread_id).await {
            Ok(Some(metadata)) => Some(metadata.rollout_path),
            Ok(None) => None,
            Err(err) => {
                tracing::warn!(
                    "failed to read state selection for composite rollout repair {thread_id}: {err}"
                );
                None
            }
        }
    } else {
        None
    };
    let publish_result = match (state_db.as_ref(), expected_path.as_deref()) {
        (Some(state_db), Some(expected_path)) => {
            publish_head(
                store,
                state_db,
                thread_id,
                expected_path,
                normalized_path.as_path(),
                stable_path.as_path(),
            )
            .await
        }
        _ => publish_head_without_state_db(store, normalized_path.as_path(), stable_path.as_path()),
    };
    if publish_result.is_err() {
        remove_failed_replacement(normalized_path.as_path()).await?;
    }
    publish_result?;
    retire_visible_rollouts(visible_paths.as_slice(), stable_path.as_path())?;

    let rollout_id = thread_rollout_resolver::rollout_id_for_thread_path(
        stable_path.as_path(),
        thread_id,
        ThreadHistoryMode::Paginated,
    )
    .await?;
    Ok(thread_rollout_resolver::ResolvedThreadRollout {
        thread_id,
        rollout_id,
        path: stable_path,
        location: selected.location,
    })
}

async fn normalize_composite_selection(
    store: &LocalThreadStore,
    state_db: &codex_rollout::StateDbHandle,
    thread_id: ThreadId,
    source_meta: codex_rollout::SessionMeta,
    lineage: &RolloutLineage,
    expected_path: &Path,
    stable_path: &Path,
) -> ThreadStoreResult<()> {
    let normalized_path = create_normalized_replacement(store, source_meta, lineage).await?;
    let result = publish_head(
        store,
        state_db,
        thread_id,
        expected_path,
        normalized_path.as_path(),
        stable_path,
    )
    .await;
    if result.is_err() {
        remove_failed_replacement(normalized_path.as_path()).await?;
    }
    result
}

async fn create_normalized_replacement(
    store: &LocalThreadStore,
    source_meta: codex_rollout::SessionMeta,
    lineage: &RolloutLineage,
) -> ThreadStoreResult<PathBuf> {
    let history_end = full_history_end(lineage).await?;
    let recorder = create_replacement_recorder(store, source_meta, RolloutId::new()).await?;
    let normalized_path = recorder.rollout_path().to_path_buf();
    if let Err(err) = copy_history_prefix(&recorder, lineage, Some(history_end)).await {
        return Err(abandon_replacement(&recorder, normalized_path.as_path(), err).await);
    }
    persist_replacement(&recorder, normalized_path.as_path()).await?;
    Ok(normalized_path)
}

async fn create_replacement_recorder(
    store: &LocalThreadStore,
    source_meta: codex_rollout::SessionMeta,
    rollout_id: RolloutId,
) -> ThreadStoreResult<RolloutRecorder> {
    let config = RolloutConfig {
        codex_home: store
            .config
            .codex_home
            .join(codex_rollout::ROLLOUT_REVISIONS_SUBDIR)
            .join(".staging"),
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

#[cfg(test)]
#[path = "revert_thread_tests.rs"]
mod tests;
