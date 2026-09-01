use super::*;
use crate::QueuedSubmissionRecord;
use crate::QueuedSubmissionAdmissionRejection;
use crate::QueuedSubmissionTerminalStatus;
use crate::QueueTerminalDisposition;
use crate::ThreadQueuePauseReason;
use sha2::Digest;
use sha2::Sha256;
use std::fmt;
use uuid::Uuid;

pub const MAX_QUEUED_SUBMISSIONS: usize = 100;
pub const MAX_QUEUED_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_QUEUE_IDENTIFIER_BYTES: usize = 256;
const TERMINAL_TOMBSTONES_PER_THREAD: i64 = 100;
pub(super) const QUEUED_SUBMISSION_COLUMNS: &str =
    "id, thread_id, payload_json, payload_digest, client_user_message_id, state, turn_id, admission_rejection, terminal_status";

#[derive(Debug)]
pub enum ThreadQueueError {
    QueueFull,
    InputBytesExceeded,
    InvalidReorder,
    InvalidIdentifier,
    ClientMessageConflict,
    Storage(anyhow::Error),
}

impl fmt::Display for ThreadQueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull => write!(
                formatter,
                "queue cannot contain more than {MAX_QUEUED_SUBMISSIONS} submissions"
            ),
            Self::InputBytesExceeded => write!(
                formatter,
                "queued input cannot exceed {MAX_QUEUED_INPUT_BYTES} bytes per thread"
            ),
            Self::InvalidReorder => write!(
                formatter,
                "queue reorder must include every pending submission exactly once"
            ),
            Self::InvalidIdentifier => write!(
                formatter,
                "queue identifiers must contain 1 to {MAX_QUEUE_IDENTIFIER_BYTES} bytes"
            ),
            Self::ClientMessageConflict => write!(
                formatter,
                "client message id is already associated with different queued input"
            ),
            Self::Storage(error) => write!(formatter, "queue storage failed: {error}"),
        }
    }
}

impl std::error::Error for ThreadQueueError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error.as_ref()),
            Self::QueueFull
            | Self::InputBytesExceeded
            | Self::InvalidReorder
            | Self::InvalidIdentifier
            | Self::ClientMessageConflict => None,
        }
    }
}

impl From<sqlx::Error> for ThreadQueueError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(error.into())
    }
}

impl From<anyhow::Error> for ThreadQueueError {
    fn from(error: anyhow::Error) -> Self {
        Self::Storage(error)
    }
}

impl StateRuntime {
    pub async fn enqueue_queued_submission(
        &self,
        thread_id: ThreadId,
        payload: &str,
        client_user_message_id: &str,
    ) -> Result<QueuedSubmissionRecord, ThreadQueueError> {
        validate_queue_identifier(client_user_message_id)?;
        if payload.len() > MAX_QUEUED_INPUT_BYTES {
            return Err(ThreadQueueError::InputBytesExceeded);
        }
        let payload_digest = queue_payload_digest(payload);
        if let Some(existing) = self
            .queued_submission_by_client_message_id(thread_id, client_user_message_id)
            .await?
        {
            return if existing.payload_digest == payload_digest {
                Ok(existing)
            } else {
                Err(ThreadQueueError::ClientMessageConflict)
            };
        }
        let id = Uuid::now_v7().to_string();
        let now_ms = datetime_to_epoch_millis(Utc::now());
        let row = sqlx::query(&format!(
            r#"
INSERT INTO thread_queue_items (
    id, thread_id, payload_json, payload_digest, client_user_message_id, queue_order,
    state, turn_id, admission_rejection, terminal_status, created_at_ms, updated_at_ms
)
SELECT ?, ?, ?, ?, ?,
       COALESCE((
           SELECT MAX(queue_order) FROM thread_queue_items
           WHERE thread_id = ? AND state != 'terminal'
       ), -1) + 1,
       'pending', NULL, NULL, NULL, ?, ?
WHERE (SELECT COUNT(*) FROM thread_queue_items WHERE thread_id = ? AND state != 'terminal') < ?
  AND COALESCE((
      SELECT SUM(length(CAST(payload_json AS BLOB)))
      FROM thread_queue_items WHERE thread_id = ? AND state != 'terminal'
  ), 0) + ? <= ?
ON CONFLICT(thread_id, client_user_message_id) DO NOTHING
RETURNING {QUEUED_SUBMISSION_COLUMNS}
            "#
        ))
        .bind(&id)
        .bind(thread_id.to_string())
        .bind(payload)
        .bind(&payload_digest)
        .bind(client_user_message_id)
        .bind(thread_id.to_string())
        .bind(now_ms)
        .bind(now_ms)
        .bind(thread_id.to_string())
        .bind(i64::try_from(MAX_QUEUED_SUBMISSIONS).map_err(anyhow::Error::from)?)
        .bind(thread_id.to_string())
        .bind(i64::try_from(payload.len()).map_err(anyhow::Error::from)?)
        .bind(i64::try_from(MAX_QUEUED_INPUT_BYTES).map_err(anyhow::Error::from)?)
        .fetch_optional(self.pool.as_ref())
        .await?;
        if let Some(row) = row {
            return QueuedSubmissionRecord::try_from_row(&row).map_err(Into::into);
        }
        if let Some(existing) = self
            .queued_submission_by_client_message_id(thread_id, client_user_message_id)
            .await?
        {
            return if existing.payload_digest == payload_digest {
                Ok(existing)
            } else {
                Err(ThreadQueueError::ClientMessageConflict)
            };
        }
        let (count, bytes): (i64, i64) = sqlx::query_as(
            r#"
SELECT COUNT(*), COALESCE(SUM(length(CAST(payload_json AS BLOB))), 0)
FROM thread_queue_items WHERE thread_id = ? AND state != 'terminal'
            "#,
        )
        .bind(thread_id.to_string())
        .fetch_one(self.pool.as_ref())
        .await?;
        if count >= i64::try_from(MAX_QUEUED_SUBMISSIONS).map_err(anyhow::Error::from)? {
            Err(ThreadQueueError::QueueFull)
        } else if bytes + i64::try_from(payload.len()).map_err(anyhow::Error::from)?
            > i64::try_from(MAX_QUEUED_INPUT_BYTES).map_err(anyhow::Error::from)?
        {
            Err(ThreadQueueError::InputBytesExceeded)
        } else {
            Err(ThreadQueueError::Storage(anyhow::anyhow!(
                "queue admission changed concurrently"
            )))
        }
    }

    pub async fn queued_submission_by_client_message_id(
        &self,
        thread_id: ThreadId,
        client_user_message_id: &str,
    ) -> Result<Option<QueuedSubmissionRecord>, ThreadQueueError> {
        validate_queue_identifier(client_user_message_id)?;
        let row = sqlx::query(&format!(
            "SELECT {QUEUED_SUBMISSION_COLUMNS} FROM thread_queue_items WHERE thread_id = ? AND client_user_message_id = ?"
        ))
        .bind(thread_id.to_string())
        .bind(client_user_message_id)
        .fetch_optional(self.pool.as_ref())
        .await?;
        row.as_ref()
            .map(QueuedSubmissionRecord::try_from_row)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn list_queued_submissions(
        &self,
        thread_id: ThreadId,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<QueuedSubmissionRecord>, ThreadQueueError> {
        let rows = sqlx::query(&format!(
            r#"
SELECT {QUEUED_SUBMISSION_COLUMNS}
FROM thread_queue_items
WHERE thread_id = ? AND state = 'pending'
ORDER BY queue_order, created_at_ms, id
LIMIT ? OFFSET ?
            "#
        ))
        .bind(thread_id.to_string())
        .bind(i64::try_from(limit).map_err(anyhow::Error::from)?)
        .bind(i64::try_from(offset).map_err(anyhow::Error::from)?)
        .fetch_all(self.pool.as_ref())
        .await?;
        rows.iter()
            .map(QueuedSubmissionRecord::try_from_row)
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub async fn queued_submission(
        &self,
        thread_id: ThreadId,
        item_id: &str,
    ) -> Result<Option<QueuedSubmissionRecord>, ThreadQueueError> {
        validate_queue_identifier(item_id)?;
        let row = sqlx::query(&format!(
            "SELECT {QUEUED_SUBMISSION_COLUMNS} FROM thread_queue_items WHERE thread_id = ? AND id = ?"
        ))
        .bind(thread_id.to_string())
        .bind(item_id)
        .fetch_optional(self.pool.as_ref())
        .await?;
        row.as_ref()
            .map(QueuedSubmissionRecord::try_from_row)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn active_queued_submission(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<QueuedSubmissionRecord>, ThreadQueueError> {
        let row = sqlx::query(&format!(
            r#"
SELECT {QUEUED_SUBMISSION_COLUMNS}
FROM thread_queue_items
WHERE thread_id = ? AND state IN ('starting', 'inflight')
ORDER BY updated_at_ms DESC LIMIT 1
            "#
        ))
        .bind(thread_id.to_string())
        .fetch_optional(self.pool.as_ref())
        .await?;
        row.as_ref()
            .map(QueuedSubmissionRecord::try_from_row)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn update_queued_submission(
        &self,
        thread_id: ThreadId,
        item_id: &str,
        payload: &str,
    ) -> Result<Option<QueuedSubmissionRecord>, ThreadQueueError> {
        validate_queue_identifier(item_id)?;
        if payload.len() > MAX_QUEUED_INPUT_BYTES {
            return Err(ThreadQueueError::InputBytesExceeded);
        }
        let row = sqlx::query(&format!(
            r#"
UPDATE thread_queue_items
SET payload_json = ?, payload_digest = ?, updated_at_ms = ?
WHERE thread_id = ? AND id = ? AND state = 'pending'
  AND COALESCE((
      SELECT SUM(length(CAST(payload_json AS BLOB)))
      FROM thread_queue_items
      WHERE thread_id = ? AND state != 'terminal' AND id <> ?
  ), 0) + ? <= ?
RETURNING {QUEUED_SUBMISSION_COLUMNS}
            "#
        ))
        .bind(payload)
        .bind(queue_payload_digest(payload))
        .bind(datetime_to_epoch_millis(Utc::now()))
        .bind(thread_id.to_string())
        .bind(item_id)
        .bind(thread_id.to_string())
        .bind(item_id)
        .bind(i64::try_from(payload.len()).map_err(anyhow::Error::from)?)
        .bind(i64::try_from(MAX_QUEUED_INPUT_BYTES).map_err(anyhow::Error::from)?)
        .fetch_optional(self.pool.as_ref())
        .await?;
        if let Some(row) = row {
            return QueuedSubmissionRecord::try_from_row(&row)
                .map(Some)
                .map_err(Into::into);
        }
        let pending = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM thread_queue_items WHERE thread_id = ? AND id = ? AND state = 'pending'",
        )
        .bind(thread_id.to_string())
        .bind(item_id)
        .fetch_optional(self.pool.as_ref())
        .await?
        .is_some();
        if pending {
            Err(ThreadQueueError::InputBytesExceeded)
        } else {
            Ok(None)
        }
    }

    pub async fn delete_queued_submission(
        &self,
        thread_id: ThreadId,
        item_id: &str,
    ) -> Result<bool, ThreadQueueError> {
        validate_queue_identifier(item_id)?;
        Ok(sqlx::query(
            "DELETE FROM thread_queue_items WHERE thread_id = ? AND id = ? AND state = 'pending'",
        )
        .bind(thread_id.to_string())
        .bind(item_id)
        .execute(self.pool.as_ref())
        .await?
        .rows_affected()
            > 0)
    }

    pub async fn reorder_queued_submissions(
        &self,
        thread_id: ThreadId,
        item_ids: &[String],
    ) -> Result<(), ThreadQueueError> {
        for item_id in item_ids {
            validate_queue_identifier(item_id)?;
        }
        let mut transaction = self.pool.begin().await?;
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT id, queue_order FROM thread_queue_items WHERE thread_id = ? AND state = 'pending' ORDER BY queue_order, id",
        )
        .bind(thread_id.to_string())
        .fetch_all(transaction.as_mut())
        .await?;
        let mut expected = rows.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
        let mut requested = item_ids.to_vec();
        expected.sort();
        requested.sort();
        if expected != requested || requested.windows(2).any(|pair| pair[0] == pair[1]) {
            transaction.rollback().await?;
            return Err(ThreadQueueError::InvalidReorder);
        }
        let max_order = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(queue_order) FROM thread_queue_items WHERE thread_id = ? AND state != 'terminal'",
        )
        .bind(thread_id.to_string())
        .fetch_one(transaction.as_mut())
        .await?
        .unwrap_or(-1);
        let temporary_base = max_order
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("queue order overflow"))?;
        let mut available_orders = rows
            .into_iter()
            .map(|(_, queue_order)| queue_order)
            .collect::<Vec<_>>();
        available_orders.sort_unstable();
        for (index, item_id) in item_ids.iter().enumerate() {
            let temporary_order = temporary_base
                .checked_add(i64::try_from(index).map_err(anyhow::Error::from)?)
                .ok_or_else(|| anyhow::anyhow!("queue order overflow"))?;
            sqlx::query(
                "UPDATE thread_queue_items SET queue_order = ?, updated_at_ms = ? WHERE thread_id = ? AND id = ? AND state = 'pending'",
            )
            .bind(temporary_order)
            .bind(datetime_to_epoch_millis(Utc::now()))
            .bind(thread_id.to_string())
            .bind(item_id)
            .execute(transaction.as_mut())
            .await?;
        }
        for (item_id, queue_order) in item_ids.iter().zip(available_orders) {
            sqlx::query(
                "UPDATE thread_queue_items SET queue_order = ? WHERE thread_id = ? AND id = ? AND state = 'pending'",
            )
            .bind(queue_order)
            .bind(thread_id.to_string())
            .bind(item_id)
            .execute(transaction.as_mut())
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn release_queued_submission_claim(
        &self,
        thread_id: ThreadId,
        item_id: &str,
        turn_id: &str,
    ) -> Result<bool, ThreadQueueError> {
        validate_queue_identifier(item_id)?;
        Ok(sqlx::query(
            r#"
UPDATE thread_queue_items
SET state = 'pending', turn_id = NULL,
    admission_rejection = NULL,
    updated_at_ms = ?
WHERE thread_id = ? AND id = ? AND turn_id = ? AND state = 'starting'
            "#,
        )
        .bind(datetime_to_epoch_millis(Utc::now()))
        .bind(thread_id.to_string())
        .bind(item_id)
        .bind(turn_id)
        .execute(self.pool.as_ref())
        .await?
        .rows_affected()
            > 0)
    }

    pub async fn mark_queued_submission_inflight(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
    ) -> Result<bool, ThreadQueueError> {
        Ok(sqlx::query(
            r#"
UPDATE thread_queue_items SET state = 'inflight', updated_at_ms = ?
WHERE thread_id = ? AND turn_id = ? AND state = 'starting'
            "#,
        )
        .bind(datetime_to_epoch_millis(Utc::now()))
        .bind(thread_id.to_string())
        .bind(turn_id)
        .execute(self.pool.as_ref())
        .await?
        .rows_affected()
            > 0)
    }

    pub async fn mark_queued_submission_admission_rejected(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        rejection: QueuedSubmissionAdmissionRejection,
    ) -> Result<bool, ThreadQueueError> {
        Ok(sqlx::query(
            r#"
UPDATE thread_queue_items SET admission_rejection = ?, updated_at_ms = ?
WHERE thread_id = ? AND turn_id = ? AND state IN ('starting', 'inflight')
            "#,
        )
        .bind(rejection.as_str())
        .bind(datetime_to_epoch_millis(Utc::now()))
        .bind(thread_id.to_string())
        .bind(turn_id)
        .execute(self.pool.as_ref())
        .await?
        .rows_affected()
            > 0)
    }

    pub async fn queued_submission_for_turn(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
    ) -> Result<Option<QueuedSubmissionRecord>, ThreadQueueError> {
        let row = sqlx::query(&format!(
            "SELECT {QUEUED_SUBMISSION_COLUMNS} FROM thread_queue_items WHERE thread_id = ? AND turn_id = ?"
        ))
        .bind(thread_id.to_string())
        .bind(turn_id)
        .fetch_optional(self.pool.as_ref())
        .await?;
        row.as_ref()
            .map(QueuedSubmissionRecord::try_from_row)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn thread_queue_pause_reason(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<ThreadQueuePauseReason>, ThreadQueueError> {
        let value = sqlx::query_scalar::<_, String>(
            "SELECT paused_reason FROM thread_queue_controls WHERE thread_id = ?",
        )
        .bind(thread_id.to_string())
        .fetch_optional(self.pool.as_ref())
        .await?;
        value
            .as_deref()
            .map(ThreadQueuePauseReason::parse)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn finish_queued_submission(
        &self,
        thread_id: ThreadId,
        turn_id: &str,
        status: QueuedSubmissionTerminalStatus,
        disposition: QueueTerminalDisposition,
    ) -> Result<bool, ThreadQueueError> {
        let mut transaction = self.pool.begin().await?;
        let changed = sqlx::query(
            r#"
UPDATE thread_queue_items
SET state = 'terminal', terminal_status = ?, payload_json = '[]', updated_at_ms = ?
WHERE thread_id = ? AND turn_id = ? AND state IN ('starting', 'inflight')
            "#,
        )
        .bind(status.as_str())
        .bind(datetime_to_epoch_millis(Utc::now()))
        .bind(thread_id.to_string())
        .bind(turn_id)
        .execute(transaction.as_mut())
        .await?
        .rows_affected()
            > 0;
        if changed {
            if let QueueTerminalDisposition::Pause(reason) = disposition {
                sqlx::query(
                    r#"
INSERT INTO thread_queue_controls (thread_id, paused_reason, updated_at_ms)
VALUES (?, ?, ?)
ON CONFLICT(thread_id) DO UPDATE SET
    paused_reason = excluded.paused_reason,
    updated_at_ms = excluded.updated_at_ms
                    "#,
                )
                .bind(thread_id.to_string())
                .bind(reason.as_str())
                .bind(datetime_to_epoch_millis(Utc::now()))
                .execute(transaction.as_mut())
                .await?;
            }
            sqlx::query(
                r#"
DELETE FROM thread_queue_items
WHERE rowid IN (
    SELECT rowid FROM thread_queue_items
    WHERE thread_id = ? AND state = 'terminal'
    ORDER BY updated_at_ms DESC, id DESC
    LIMIT -1 OFFSET ?
)
                "#,
            )
            .bind(thread_id.to_string())
            .bind(TERMINAL_TOMBSTONES_PER_THREAD)
            .execute(transaction.as_mut())
            .await?;
        }
        transaction.commit().await?;
        Ok(changed)
    }
}

pub(super) fn validate_queue_identifier(identifier: &str) -> Result<(), ThreadQueueError> {
    if identifier.is_empty() || identifier.len() > MAX_QUEUE_IDENTIFIER_BYTES {
        Err(ThreadQueueError::InvalidIdentifier)
    } else {
        Ok(())
    }
}

fn queue_payload_digest(payload: &str) -> String {
    format!("{:x}", Sha256::digest(payload.as_bytes()))
}

#[cfg(test)]
#[path = "thread_queue_tests.rs"]
mod tests;
