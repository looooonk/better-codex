use codex_protocol::turn_input::TurnInput;
use codex_protocol::user_input::UserInput;
use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;

pub(crate) async fn repair_migrations(
    pool: &SqlitePool,
    migrator: &Migrator,
) -> anyhow::Result<()> {
    if sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(pool)
    .await?
    .is_none()
    {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for (old, new) in [(49, 10_001), (50, 10_002), (51, 10_003)] {
        let Some(migration) = migrator.migrations.iter().find(|item| item.version == new) else {
            continue;
        };
        sqlx::query(
            "UPDATE _sqlx_migrations SET version = ?, description = ?
             WHERE version = ? AND checksum = ?
             AND NOT EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version = ?)",
        )
        .bind(new)
        .bind(migration.description.as_ref())
        .bind(old)
        .bind(migration.checksum.as_ref())
        .bind(new)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM _sqlx_migrations WHERE version = ? AND checksum = ?
             AND EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version = ? AND checksum = ?)",
        )
        .bind(old)
        .bind(migration.checksum.as_ref())
        .bind(new)
        .bind(migration.checksum.as_ref())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn import_queue(state: &SqlitePool, queue: &SqlitePool) -> anyhow::Result<()> {
    let mut offset = 0_i64;
    loop {
        let rows = sqlx::query(
            "SELECT id, thread_id, payload_json, client_user_message_id, created_at_ms,
                    updated_at_ms, EXISTS (
                        SELECT 1 FROM thread_queue_controls c WHERE c.thread_id = q.thread_id
                    ) OR EXISTS (
                        SELECT 1 FROM thread_queue_items a WHERE a.thread_id = q.thread_id
                        AND a.state IN ('starting', 'inflight')
                    ) AS paused
             FROM thread_queue_items q WHERE state = 'pending'
               AND NOT EXISTS (
                   SELECT 1 FROM thread_queue_controls c WHERE c.thread_id = q.thread_id
                     AND c.blocked_submission_id = q.id AND c.blocked_retry_allowed = 0
               )
             ORDER BY thread_id, queue_order LIMIT 100 OFFSET ?",
        )
        .bind(offset)
        .fetch_all(state)
        .await?;
        if rows.is_empty() {
            break;
        }
        offset += rows.len() as i64;
        let mut tx = queue.begin().await?;
        for row in rows {
            let id: String = row.try_get("id")?;
            if sqlx::query("INSERT OR IGNORE INTO better_queue_imports (id) VALUES (?)")
                .bind(&id)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                == 0
            {
                continue;
            }
            let content: Vec<UserInput> = serde_json::from_str(row.try_get("payload_json")?)?;
            let payload = serde_json::to_string(&TurnInput::UserInput {
                content,
                client_id: Some(row.try_get("client_user_message_id")?),
            })?;
            let thread_id: String = row.try_get("thread_id")?;
            sqlx::query(
                "INSERT INTO queued_items
                 (id, thread_id, payload_json, queue_order, created_at_ms, updated_at_ms)
                 SELECT ?, ?, ?, COALESCE(MAX(queue_order), -1) + 1, ?, ?
                 FROM queued_items WHERE thread_id = ?",
            )
            .bind(id)
            .bind(&thread_id)
            .bind(payload)
            .bind(row.try_get::<i64, _>("created_at_ms")?)
            .bind(row.try_get::<i64, _>("updated_at_ms")?)
            .bind(&thread_id)
            .execute(&mut *tx)
            .await?;
            if row.try_get::<bool, _>("paused")? {
                sqlx::query("INSERT OR IGNORE INTO better_queue_pauses (thread_id) VALUES (?)")
                    .bind(thread_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "better_compat_tests.rs"]
mod tests;
