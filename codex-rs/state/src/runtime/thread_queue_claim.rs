use super::StateRuntime;
use super::datetime_to_epoch_millis;
use super::thread_queue::QUEUED_SUBMISSION_COLUMNS;
use super::thread_queue::ThreadQueueError;
use super::thread_queue::validate_queue_identifier;
use crate::QueuedSubmissionRecord;
use crate::QueuedSubmissionState;
use chrono::Utc;
use codex_protocol::ThreadId;
use sqlx::Sqlite;
use sqlx::SqliteConnection;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueueClaimResult {
    Claimed(QueuedSubmissionRecord),
    Existing(QueuedSubmissionRecord),
    Busy(QueuedSubmissionRecord),
    Empty,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueClaimAndResumeResult {
    pub claim: QueueClaimResult,
    pub resumed: bool,
}

#[derive(Clone, Copy)]
enum QueueClaimPausePolicy {
    Preserve,
    ResumeOnSuccess,
}

impl StateRuntime {
    pub async fn claim_queued_submission(
        &self,
        thread_id: ThreadId,
        item_id: Option<&str>,
        turn_id: &str,
    ) -> Result<QueueClaimResult, ThreadQueueError> {
        Ok(self
            .claim_queued_submission_with_pause_policy(
                thread_id,
                item_id,
                turn_id,
                QueueClaimPausePolicy::Preserve,
            )
            .await?
            .claim)
    }

    pub async fn claim_queued_submission_and_resume(
        &self,
        thread_id: ThreadId,
        item_id: Option<&str>,
        turn_id: &str,
    ) -> Result<QueueClaimAndResumeResult, ThreadQueueError> {
        self.claim_queued_submission_with_pause_policy(
            thread_id,
            item_id,
            turn_id,
            QueueClaimPausePolicy::ResumeOnSuccess,
        )
        .await
    }

    async fn claim_queued_submission_with_pause_policy(
        &self,
        thread_id: ThreadId,
        item_id: Option<&str>,
        turn_id: &str,
        pause_policy: QueueClaimPausePolicy,
    ) -> Result<QueueClaimAndResumeResult, ThreadQueueError> {
        if let Some(item_id) = item_id {
            validate_queue_identifier(item_id)?;
        }
        let mut transaction = self.pool.begin().await?;
        if let Some(item_id) = item_id
            && let Some(existing) = queued_submission_on_connection(
                transaction.as_mut(),
                thread_id,
                item_id,
            )
            .await?
            && existing.state != QueuedSubmissionState::Pending
        {
            return finish_queue_claim(
                transaction,
                thread_id,
                QueueClaimResult::Existing(existing),
                pause_policy,
            )
            .await;
        }
        if let Some(active) =
            active_queued_submission_on_connection(transaction.as_mut(), thread_id).await?
        {
            return finish_queue_claim(
                transaction,
                thread_id,
                QueueClaimResult::Busy(active),
                pause_policy,
            )
            .await;
        }
        let now_ms = datetime_to_epoch_millis(Utc::now());
        let row = if let Some(item_id) = item_id {
            sqlx::query(&format!(
                r#"
UPDATE thread_queue_items
SET state = 'starting', turn_id = ?, updated_at_ms = ?
WHERE thread_id = ? AND id = ? AND state = 'pending'
  AND NOT EXISTS (
      SELECT 1 FROM thread_queue_items
      WHERE thread_id = ? AND state IN ('starting', 'inflight')
  )
RETURNING {QUEUED_SUBMISSION_COLUMNS}
                "#
            ))
            .bind(turn_id)
            .bind(now_ms)
            .bind(thread_id.to_string())
            .bind(item_id)
            .bind(thread_id.to_string())
            .fetch_optional(transaction.as_mut())
            .await?
        } else {
            sqlx::query(&format!(
                r#"
UPDATE thread_queue_items
SET state = 'starting', turn_id = ?, updated_at_ms = ?
WHERE id = (
    SELECT id FROM thread_queue_items
    WHERE thread_id = ? AND state = 'pending'
    ORDER BY queue_order, created_at_ms, id LIMIT 1
)
  AND state = 'pending'
  AND NOT EXISTS (
      SELECT 1 FROM thread_queue_items
      WHERE thread_id = ? AND state IN ('starting', 'inflight')
  )
RETURNING {QUEUED_SUBMISSION_COLUMNS}
                "#
            ))
            .bind(turn_id)
            .bind(now_ms)
            .bind(thread_id.to_string())
            .bind(thread_id.to_string())
            .fetch_optional(transaction.as_mut())
            .await?
        };
        if let Some(row) = row {
            let record = QueuedSubmissionRecord::try_from_row(&row)?;
            return finish_queue_claim(
                transaction,
                thread_id,
                QueueClaimResult::Claimed(record),
                pause_policy,
            )
            .await;
        }
        if let Some(item_id) = item_id
            && let Some(existing) = queued_submission_on_connection(
                transaction.as_mut(),
                thread_id,
                item_id,
            )
            .await?
            && existing.state != QueuedSubmissionState::Pending
        {
            return finish_queue_claim(
                transaction,
                thread_id,
                QueueClaimResult::Existing(existing),
                pause_policy,
            )
            .await;
        }
        let claim = if let Some(active) =
            active_queued_submission_on_connection(transaction.as_mut(), thread_id).await?
        {
            QueueClaimResult::Busy(active)
        } else {
            QueueClaimResult::Empty
        };
        finish_queue_claim(transaction, thread_id, claim, pause_policy).await
    }
}

async fn finish_queue_claim(
    mut transaction: sqlx::Transaction<'_, Sqlite>,
    thread_id: ThreadId,
    claim: QueueClaimResult,
    pause_policy: QueueClaimPausePolicy,
) -> Result<QueueClaimAndResumeResult, ThreadQueueError> {
    let should_resume = match (&claim, pause_policy) {
        (QueueClaimResult::Claimed(_), QueueClaimPausePolicy::ResumeOnSuccess) => true,
        (QueueClaimResult::Existing(record), QueueClaimPausePolicy::ResumeOnSuccess) => {
            record.turn_id.is_some()
        }
        (
            QueueClaimResult::Busy(_) | QueueClaimResult::Empty,
            QueueClaimPausePolicy::ResumeOnSuccess,
        )
        | (_, QueueClaimPausePolicy::Preserve) => false,
    };
    let resumed = if should_resume {
        sqlx::query("DELETE FROM thread_queue_controls WHERE thread_id = ?")
            .bind(thread_id.to_string())
            .execute(transaction.as_mut())
            .await?
            .rows_affected()
            > 0
    } else {
        false
    };
    transaction.commit().await?;
    Ok(QueueClaimAndResumeResult { claim, resumed })
}

async fn queued_submission_on_connection(
    connection: &mut SqliteConnection,
    thread_id: ThreadId,
    item_id: &str,
) -> Result<Option<QueuedSubmissionRecord>, ThreadQueueError> {
    let row = sqlx::query(&format!(
        "SELECT {QUEUED_SUBMISSION_COLUMNS} FROM thread_queue_items WHERE thread_id = ? AND id = ?"
    ))
    .bind(thread_id.to_string())
    .bind(item_id)
    .fetch_optional(&mut *connection)
    .await?;
    row.as_ref()
        .map(QueuedSubmissionRecord::try_from_row)
        .transpose()
        .map_err(Into::into)
}

async fn active_queued_submission_on_connection(
    connection: &mut SqliteConnection,
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
    .fetch_optional(&mut *connection)
    .await?;
    row.as_ref()
        .map(QueuedSubmissionRecord::try_from_row)
        .transpose()
        .map_err(Into::into)
}
