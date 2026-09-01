use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use codex_protocol::RolloutId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::HistoryPosition;
use codex_protocol::protocol::ThreadHistoryMode;

use super::LocalThreadStore;
use super::thread_rollout_resolver;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

const MAX_ROLLOUT_LINEAGE_SEGMENTS: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RolloutLineageSegment {
    pub(super) rollout_id: RolloutId,
    pub(super) rollout_path: PathBuf,
    pub(super) start_ordinal: u64,
    pub(super) end: Option<HistoryPosition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RolloutLineage {
    pub(super) segments: Vec<RolloutLineageSegment>,
}

impl LocalThreadStore {
    pub(super) async fn resolve_rollout_lineage(
        &self,
        requested_thread_id: ThreadId,
    ) -> ThreadStoreResult<RolloutLineage> {
        self.resolve_rollout_lineage_with_representation(
            requested_thread_id,
            LineageRepresentation::Existing,
        )
        .await
    }

    pub(super) async fn resolve_rollout_lineage_for_reference(
        &self,
        requested_thread_id: ThreadId,
    ) -> ThreadStoreResult<RolloutLineage> {
        self.resolve_rollout_lineage_with_representation(
            requested_thread_id,
            LineageRepresentation::PlainForReference,
        )
        .await
    }

    async fn resolve_rollout_lineage_with_representation(
        &self,
        requested_thread_id: ThreadId,
        representation: LineageRepresentation,
    ) -> ThreadStoreResult<RolloutLineage> {
        let mut segments = Vec::new();
        let mut seen = HashSet::new();
        let mut next_rollout_id = None;
        let mut end = None;

        loop {
            if segments.len() == MAX_ROLLOUT_LINEAGE_SEGMENTS {
                return Err(malformed_lineage(
                    requested_thread_id,
                    "lineage exceeds 128 rollout segments",
                ));
            }
            let coordination_id = next_rollout_id.unwrap_or(requested_thread_id);
            let _writer_guard = match representation {
                LineageRepresentation::Existing => None,
                LineageRepresentation::PlainForReference => {
                    Some(self.live_writer_locks.lock(coordination_id).await)
                }
            };
            let (rollout_id, rollout_path) = match next_rollout_id {
                Some(rollout_id) => {
                    let rollout_path = resolve_rollout_path_by_id(self, rollout_id)
                        .await?
                        .ok_or_else(|| malformed_lineage(rollout_id, "missing source rollout"))?;
                    (rollout_id, rollout_path)
                }
                None => {
                    let resolved = thread_rollout_resolver::resolve_current_including_archived(
                        self,
                        requested_thread_id,
                    )
                    .await?
                    .ok_or_else(|| {
                        malformed_lineage(requested_thread_id, "missing source rollout")
                    })?;
                    (resolved.rollout_id, resolved.path)
                }
            };
            if !seen.insert(rollout_id) {
                return Err(malformed_lineage(requested_thread_id, "cycle detected"));
            }
            let rollout_path = match representation {
                LineageRepresentation::Existing => rollout_path,
                LineageRepresentation::PlainForReference => {
                    let rollout_path = super::helpers::scoped_rollout_path(
                        self.config.codex_home.clone(),
                        rollout_path.as_path(),
                        "Codex home",
                    )?;
                    codex_rollout::materialize_rollout_for_reference(rollout_path.as_path())
                        .await
                        .map_err(|err| ThreadStoreError::Internal {
                            message: format!(
                                "failed to materialize referenced rollout {}: {err}",
                                rollout_path.display()
                            ),
                        })?
                }
            };
            let meta = codex_rollout::read_session_meta_line(rollout_path.as_path())
                .await
                .map_err(|err| ThreadStoreError::Internal {
                    message: format!(
                        "failed to read lineage metadata {}: {err}",
                        rollout_path.display()
                    ),
                })?;
            let canonical_rollout_id = codex_rollout::rollout_id_from_path(rollout_path.as_path());
            let revision_rollout_id = rollout_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.strip_suffix(".zst").unwrap_or(name))
                .and_then(|name| name.strip_suffix(".jsonl"))
                .and_then(|name| RolloutId::from_string(name).ok());
            let path_rollout_id = canonical_rollout_id.or(revision_rollout_id);
            let stable_metadata_identity = canonical_rollout_id == Some(requested_thread_id)
                && meta.meta.rollout_id == Some(rollout_id);
            if (path_rollout_id != Some(rollout_id) && !stable_metadata_identity)
                || meta
                    .meta
                    .rollout_id
                    .is_some_and(|metadata_rollout_id| metadata_rollout_id != rollout_id)
            {
                return Err(malformed_lineage(
                    requested_thread_id,
                    format!(
                        "source rollout identity disagrees with requested rollout {rollout_id}: {}",
                        rollout_path.display()
                    )
                    .as_str(),
                ));
            }
            if meta.meta.id != requested_thread_id {
                return Err(malformed_lineage(
                    requested_thread_id,
                    "source rollout belongs to another thread",
                ));
            }
            if meta.meta.history_mode != ThreadHistoryMode::Paginated {
                return Err(malformed_lineage(
                    requested_thread_id,
                    "source rollout is not paginated",
                ));
            }
            if let Some(end) = end {
                validate_cutoff_bounds(requested_thread_id, rollout_path.as_path(), &end).await?;
            }
            let start_ordinal = match meta.meta.history_base {
                Some(base) => base.end_ordinal_exclusive.checked_add(1).ok_or_else(|| {
                    malformed_lineage(requested_thread_id, "source ordinal overflow")
                })?,
                None => 1,
            };
            segments.push(RolloutLineageSegment {
                rollout_id,
                rollout_path,
                start_ordinal,
                end,
            });

            let Some(base) = meta.meta.history_base else {
                break;
            };
            next_rollout_id = Some(base.thread_id);
            end = Some(base);
        }

        segments.reverse();
        Ok(RolloutLineage { segments })
    }
}

async fn resolve_rollout_path_by_id(
    store: &LocalThreadStore,
    rollout_id: RolloutId,
) -> ThreadStoreResult<Option<PathBuf>> {
    codex_rollout::find_rollout_path_by_rollout_id(store.config.codex_home.as_path(), rollout_id)
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!("failed to locate rollout {rollout_id}: {err}"),
        })
}

#[derive(Clone, Copy)]
enum LineageRepresentation {
    Existing,
    PlainForReference,
}

impl RolloutLineage {
    pub(super) fn segments(&self) -> &[RolloutLineageSegment] {
        self.segments.as_slice()
    }

    pub(super) fn segment_index_for_ordinal(&self, ordinal: u64) -> Option<usize> {
        self.segments.iter().position(|segment| {
            ordinal >= segment.start_ordinal()
                && segment
                    .end_ordinal()
                    .is_none_or(|end_ordinal| ordinal < end_ordinal)
        })
    }
}

impl RolloutLineageSegment {
    pub(super) fn rollout_id(&self) -> RolloutId {
        self.rollout_id
    }

    pub(super) fn start_ordinal(&self) -> u64 {
        self.start_ordinal
    }

    pub(super) fn end_ordinal(&self) -> Option<u64> {
        self.end.map(|end| end.end_ordinal_exclusive)
    }
}

async fn validate_cutoff_bounds(
    requested_thread_id: ThreadId,
    rollout_path: &Path,
    end: &HistoryPosition,
) -> ThreadStoreResult<()> {
    if end.end_ordinal_exclusive == 0 {
        return Err(malformed_lineage(
            requested_thread_id,
            "cutoff cannot include source session metadata",
        ));
    }
    let (previous_ordinal, next_ordinal) =
        codex_rollout::rollout_ordinals_at_boundary(rollout_path, end.end_byte_offset)
            .await
            .map_err(|err| {
                let detail = format!("invalid cutoff record boundary: {err}");
                malformed_lineage(requested_thread_id, detail.as_str())
            })?;
    let expected_previous = end
        .end_ordinal_exclusive
        .checked_sub(1)
        .ok_or_else(|| malformed_lineage(requested_thread_id, "cutoff ordinal underflow"))?;
    if previous_ordinal != expected_previous
        || next_ordinal.is_some_and(|ordinal| ordinal != end.end_ordinal_exclusive)
    {
        return Err(malformed_lineage(
            requested_thread_id,
            "cutoff byte offset disagrees with rollout ordinals",
        ));
    }
    Ok(())
}

fn malformed_lineage(thread_id: ThreadId, detail: &str) -> ThreadStoreError {
    ThreadStoreError::InvalidRequest {
        message: format!("invalid paginated history lineage for {thread_id}: {detail}"),
    }
}

#[cfg(test)]
#[path = "rollout_lineage_tests.rs"]
mod tests;
