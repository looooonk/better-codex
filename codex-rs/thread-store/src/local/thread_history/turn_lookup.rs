use codex_protocol::RolloutId;
use sqlx::Row;

use super::super::rollout_lineage::RolloutLineage;
use super::super::rollout_lineage::RolloutLineageSegment;
use super::sqlite_integer;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

pub(in crate::local) struct TurnRow {
    pub rollout_id: RolloutId,
    pub rollout_ordinal: i64,
    pub rollout_byte_offset: Option<i64>,
    pub rollout_end_ordinal: Option<i64>,
}

pub(in crate::local) async fn find_source_turn(
    pool: &sqlx::SqlitePool,
    lineage: &RolloutLineage,
    turn_id: &str,
) -> ThreadStoreResult<TurnRow> {
    for segment in lineage.segments() {
        if let Some(row) = query_turn_row(pool, segment, turn_id).await? {
            return Ok(row);
        }
    }
    Err(ThreadStoreError::InvalidRequest {
        message: format!("turn not found: {turn_id}"),
    })
}

async fn query_turn_row(
    pool: &sqlx::SqlitePool,
    segment: &RolloutLineageSegment,
    turn_id: &str,
) -> ThreadStoreResult<Option<TurnRow>> {
    let end_ordinal = segment
        .end_ordinal()
        .map(|ordinal| sqlite_integer(ordinal, "rollout ordinal"))
        .transpose()?;
    sqlx::query(
        r#"
SELECT rollout_ordinal, rollout_byte_offset, rollout_end_ordinal
FROM thread_turns
WHERE thread_id = ?
  AND turn_id = ?
  AND rollout_ordinal >= ?
  AND (? IS NULL OR rollout_ordinal < ?)
        "#,
    )
    .bind(segment.rollout_id().to_string())
    .bind(turn_id)
    .bind(sqlite_integer(segment.start_ordinal(), "rollout ordinal")?)
    .bind(end_ordinal)
    .bind(end_ordinal)
    .fetch_optional(pool)
    .await
    .map_err(|err| ThreadStoreError::Internal {
        message: format!("failed to resolve logical turn: {err}"),
    })
    .map(|row| {
        row.map(|row| TurnRow {
            rollout_id: segment.rollout_id(),
            rollout_ordinal: row.get("rollout_ordinal"),
            rollout_byte_offset: row.get("rollout_byte_offset"),
            rollout_end_ordinal: row.get("rollout_end_ordinal"),
        })
    })
}
