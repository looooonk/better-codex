use std::fs::File;
use std::io;
#[cfg(test)]
use std::path::Path;

use codex_protocol::protocol::SessionMetaLine;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_rollout::ModelContextScan;
use codex_rollout::ModelContextScanProgress;
use codex_rollout::ReverseJsonlScanner;
use codex_rollout::RolloutItem;
use codex_rollout::ScanOutcome;
use serde_json::Value;

use super::LocalThreadStore;
use super::read_thread;
use super::rollout_lineage::RolloutLineage;
#[cfg(test)]
use super::rollout_lineage::RolloutLineageSegment;
use super::thread_rollout_resolver;
use crate::LoadThreadHistoryParams;
use crate::StoredModelContext;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

#[cfg(test)]
#[path = "model_context_tests.rs"]
mod tests;

/// Loads rollout items needed to reconstruct the latest model-visible context.
///
/// Paginated lineage is materialized as plain JSONL and reverse-scanned from the selected head
/// through its immutable ancestors. The returned replay starts with the selected rollout's
/// canonical `SessionMeta`. Legacy rollouts keep the existing full-history path.
pub(super) async fn load_latest_model_context(
    store: &LocalThreadStore,
    params: LoadThreadHistoryParams,
) -> ThreadStoreResult<StoredModelContext> {
    let resolved = if params.include_archived {
        thread_rollout_resolver::resolve_current_including_archived(store, params.thread_id).await?
    } else {
        thread_rollout_resolver::resolve_current(store, params.thread_id).await?
    };
    let path =
        resolved
            .map(|resolved| resolved.path)
            .ok_or_else(|| ThreadStoreError::InvalidRequest {
                message: format!("no rollout found for thread id {}", params.thread_id),
            })?;

    let session_meta = codex_rollout::read_session_meta_line(path.as_path())
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!("failed to read session metadata {}: {err}", path.display()),
        })?;
    if session_meta.meta.id != params.thread_id {
        return Err(ThreadStoreError::InvalidRequest {
            message: format!(
                "rollout at {} belongs to thread {}, not {}",
                path.display(),
                session_meta.meta.id,
                params.thread_id
            ),
        });
    }

    let items = if matches!(session_meta.meta.history_mode, ThreadHistoryMode::Paginated) {
        let lineage = store
            .resolve_rollout_lineage_for_reference(params.thread_id)
            .await?;
        scan_model_context_from_lineage(lineage, session_meta).await?
    } else {
        read_thread::load_history_items(path.as_path()).await?
    };

    Ok(StoredModelContext {
        thread_id: params.thread_id,
        items,
    })
}

async fn scan_model_context_from_lineage(
    lineage: RolloutLineage,
    session_meta: SessionMetaLine,
) -> ThreadStoreResult<Vec<RolloutItem>> {
    let scan = tokio::task::spawn_blocking(move || {
        scan_model_context_from_lineage_blocking(&lineage, session_meta)
    })
    .await
    .map_err(|err| ThreadStoreError::Internal {
        message: format!("failed to join model context scan: {err}"),
    })?;
    match scan {
        Ok(items) => Ok(items),
        Err(err) => Err(ThreadStoreError::Internal {
            message: format!("failed to scan paginated model context lineage: {err}"),
        }),
    }
}

#[cfg(test)]
fn scan_model_context_from_end_blocking(
    path: &Path,
    session_meta: SessionMetaLine,
) -> io::Result<Vec<RolloutItem>> {
    let path_rollout_id = codex_rollout::rollout_id_from_path(path);
    if let (Some(path_rollout_id), Some(metadata_rollout_id)) =
        (path_rollout_id, session_meta.meta.rollout_id)
        && path_rollout_id != session_meta.meta.id
        && path_rollout_id != metadata_rollout_id
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "rollout identity disagrees with canonical filename: {}",
                path.display()
            ),
        ));
    }
    let rollout_id = session_meta
        .meta
        .rollout_id
        .or(path_rollout_id)
        .unwrap_or(session_meta.meta.id);
    scan_model_context_from_lineage_blocking(
        &RolloutLineage {
            segments: vec![RolloutLineageSegment {
                rollout_id,
                rollout_path: path.to_path_buf(),
                start_ordinal: 1,
                end: None,
            }],
        },
        session_meta,
    )
}

fn scan_model_context_from_lineage_blocking(
    lineage: &RolloutLineage,
    session_meta: SessionMetaLine,
) -> io::Result<Vec<RolloutItem>> {
    let mut scan = ModelContextScan::default();
    'segments: for segment in lineage.segments().iter().rev() {
        let file = File::open(segment.rollout_path.as_path())?;
        let mut scanner = match segment.end.map(|end| end.end_byte_offset) {
            Some(end_byte_offset) => ReverseJsonlScanner::new_at(file, end_byte_offset)?,
            None => ReverseJsonlScanner::new(file)?,
        };
        while let Some(outcome) = scanner.scan_next::<Value>()? {
            let ScanOutcome::Parsed(mut value) = outcome else {
                continue;
            };
            codex_rollout::redact_persisted_json(&mut value);
            let Ok(line) = codex_rollout::decode_rollout_line(value) else {
                continue;
            };
            if matches!(&line.item, RolloutItem::SessionMeta(_)) {
                break;
            }
            match scan.push(line.item) {
                ModelContextScanProgress::Continue => {}
                ModelContextScanProgress::Complete => break 'segments,
            }
        }
    }

    let canonical_meta = session_meta.clone();
    let mut items = scan.finish(session_meta);
    if !matches!(items.first(), Some(RolloutItem::SessionMeta(_))) {
        items.insert(0, RolloutItem::SessionMeta(canonical_meta));
    }
    Ok(items)
}
