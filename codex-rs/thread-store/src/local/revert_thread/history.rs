use codex_protocol::protocol::HistoryPosition;
use codex_rollout::RolloutItem;
use codex_rollout::RolloutRecorder;

use super::super::LocalThreadStore;
use super::super::rollout_lineage::RolloutLineage;
use super::super::rollout_lineage::RolloutLineageSegment;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

const REPLACEMENT_COPY_BATCH_ITEMS: usize = 256;

pub(super) async fn full_history_end(
    lineage: &RolloutLineage,
) -> ThreadStoreResult<HistoryPosition> {
    let segment = lineage
        .segments()
        .last()
        .ok_or_else(|| ThreadStoreError::Internal {
            message: "revert rollout lineage is empty".to_string(),
        })?;
    let end_byte_offset = tokio::fs::metadata(segment.rollout_path.as_path())
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!(
                "failed to inspect current rollout {}: {err}",
                segment.rollout_path.display()
            ),
        })?
        .len();
    let (previous_ordinal, next_ordinal) = codex_rollout::rollout_ordinals_at_boundary(
        segment.rollout_path.as_path(),
        end_byte_offset,
    )
    .await
    .map_err(|err| ThreadStoreError::Internal {
        message: format!(
            "failed to locate current rollout end in {}: {err}",
            segment.rollout_path.display()
        ),
    })?;
    if next_ordinal.is_some() {
        return Err(ThreadStoreError::Internal {
            message: format!(
                "current rollout end is not the final record in {}",
                segment.rollout_path.display()
            ),
        });
    }
    Ok(HistoryPosition {
        thread_id: segment.rollout_id(),
        end_ordinal_exclusive: previous_ordinal.checked_add(1).ok_or_else(|| {
            ThreadStoreError::Internal {
                message: "current rollout ordinal overflow".to_string(),
            }
        })?,
        end_byte_offset,
    })
}

pub(super) async fn copy_history_prefix(
    recorder: &RolloutRecorder,
    lineage: &RolloutLineage,
    history_end: Option<HistoryPosition>,
) -> ThreadStoreResult<()> {
    let Some(history_end) = history_end else {
        return Ok(());
    };
    let end_segment_index = lineage
        .segments()
        .iter()
        .position(|segment| segment.rollout_id() == history_end.thread_id)
        .ok_or_else(|| ThreadStoreError::Internal {
            message: "revert history prefix is outside the selected rollout lineage".to_string(),
        })?;
    let mut batch = Vec::with_capacity(REPLACEMENT_COPY_BATCH_ITEMS);
    for (index, segment) in lineage.segments()[..=end_segment_index].iter().enumerate() {
        let end = if index == end_segment_index {
            history_end
        } else {
            segment.end.ok_or_else(|| ThreadStoreError::Internal {
                message: "revert lineage segment is missing its inherited boundary".to_string(),
            })?
        };
        copy_segment_prefix(recorder, segment, end, &mut batch).await?;
    }
    flush_copy_batch(recorder, &mut batch).await
}

async fn copy_segment_prefix(
    recorder: &RolloutRecorder,
    segment: &RolloutLineageSegment,
    end: HistoryPosition,
    batch: &mut Vec<RolloutItem>,
) -> ThreadStoreResult<()> {
    let expected_previous =
        end.end_ordinal_exclusive
            .checked_sub(1)
            .ok_or_else(|| ThreadStoreError::Internal {
                message: "revert history prefix includes session metadata".to_string(),
            })?;
    let (previous_ordinal, next_ordinal) = codex_rollout::rollout_ordinals_at_boundary(
        segment.rollout_path.as_path(),
        end.end_byte_offset,
    )
    .await
    .map_err(|err| ThreadStoreError::Internal {
        message: format!(
            "failed to validate revert boundary in {}: {err}",
            segment.rollout_path.display()
        ),
    })?;
    if previous_ordinal != expected_previous
        || next_ordinal.is_some_and(|ordinal| ordinal != end.end_ordinal_exclusive)
    {
        return Err(ThreadStoreError::Internal {
            message: format!(
                "revert boundary disagrees with rollout ordinals in {}",
                segment.rollout_path.display()
            ),
        });
    }

    let mut reader = codex_rollout::open_rollout_line_reader(segment.rollout_path.as_path())
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!(
                "failed to open inherited rollout {}: {err}",
                segment.rollout_path.display()
            ),
        })?;
    let mut next_expected = segment.start_ordinal();
    while let Some(line) = reader
        .next_line()
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!(
                "failed to read inherited rollout {}: {err}",
                segment.rollout_path.display()
            ),
        })?
    {
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str(&line).map_err(|err| ThreadStoreError::Internal {
            message: format!(
                "failed to parse inherited rollout {}: {err}",
                segment.rollout_path.display()
            ),
        })?;
        let record = codex_rollout::decode_rollout_line(value).map_err(|err| {
            ThreadStoreError::Internal {
                message: format!(
                    "failed to decode inherited rollout {}: {err}",
                    segment.rollout_path.display()
                ),
            }
        })?;
        let ordinal = record.ordinal.ok_or_else(|| ThreadStoreError::Internal {
            message: format!(
                "inherited paginated rollout {} contains a record without an ordinal",
                segment.rollout_path.display()
            ),
        })?;
        if ordinal < segment.start_ordinal() {
            continue;
        }
        if ordinal >= end.end_ordinal_exclusive {
            break;
        }
        if ordinal != next_expected {
            return Err(ThreadStoreError::Internal {
                message: format!(
                    "inherited paginated rollout {} has a non-contiguous prefix",
                    segment.rollout_path.display()
                ),
            });
        }
        batch.push(record.item);
        next_expected = next_expected
            .checked_add(1)
            .ok_or_else(|| ThreadStoreError::Internal {
                message: "revert history prefix ordinal overflow".to_string(),
            })?;
        if batch.len() == REPLACEMENT_COPY_BATCH_ITEMS {
            flush_copy_batch(recorder, batch).await?;
        }
    }
    if next_expected != end.end_ordinal_exclusive {
        return Err(ThreadStoreError::Internal {
            message: format!(
                "inherited rollout {} ended before the revert boundary",
                segment.rollout_path.display()
            ),
        });
    }
    Ok(())
}

async fn flush_copy_batch(
    recorder: &RolloutRecorder,
    batch: &mut Vec<RolloutItem>,
) -> ThreadStoreResult<()> {
    if batch.is_empty() {
        return Ok(());
    }
    recorder
        .record_canonical_items(batch.as_slice())
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!("failed to queue inherited history for reverted rollout: {err}"),
        })?;
    recorder
        .flush()
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!("failed to persist inherited history for reverted rollout: {err}"),
        })?;
    batch.clear();
    Ok(())
}

pub(super) async fn history_end_before_turn(
    store: &LocalThreadStore,
    lineage: &RolloutLineage,
    turn_id: &str,
) -> ThreadStoreResult<Option<HistoryPosition>> {
    let pool = store.thread_history_db().await?;
    let row = super::super::thread_history::find_source_turn(pool, lineage, turn_id).await?;
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
