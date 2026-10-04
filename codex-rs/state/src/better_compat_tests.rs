use super::*;
use crate::SqliteConfig;
use crate::migrations::QUEUE_MIGRATOR;
use crate::migrations::STATE_MIGRATOR;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use std::borrow::Cow;

#[tokio::test]
async fn archives_retired_agent_jobs_without_losing_rows() -> anyhow::Result<()> {
    let directory = crate::runtime::test_support::unique_temp_dir();
    tokio::fs::create_dir_all(&directory).await?;
    let _cleanup = scopeguard::guard(directory.clone(), |path| {
        let _ = std::fs::remove_dir_all(path);
    });
    let sqlite = SqliteConfig::new_for_testing(directory.as_path().abs());
    let state = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
    let mut previous = crate::migrations::runtime_state_migrator();
    previous.migrations = Cow::Owned(
        STATE_MIGRATOR
            .migrations
            .iter()
            .filter(|m| m.version < 10_005)
            .cloned()
            .collect(),
    );
    previous.run(&state).await?;
    sqlx::raw_sql(
        r#"
        INSERT INTO agent_jobs
            (id, name, status, instruction, input_headers_json, input_csv_path,
             output_csv_path, created_at, updated_at)
        VALUES ('job', 'legacy', 'pending', 'task', '[]', 'in.csv', 'out.csv', 1, 1);
        INSERT INTO agent_job_items
            (job_id, item_id, row_index, row_json, status, created_at, updated_at)
        VALUES ('job', 'item', 0, '{"value":"retained"}', 'pending', 1, 1);
        "#,
    )
    .execute(&state)
    .await?;
    STATE_MIGRATOR.run(&state).await?;
    assert_eq!(
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT jobs.id, items.item_id, items.row_json FROM better_legacy_agent_jobs jobs \
         JOIN better_legacy_agent_job_items items ON items.job_id = jobs.id",
        )
        .fetch_all(&state)
        .await?,
        vec![(
            "job".to_string(),
            "item".to_string(),
            r#"{"value":"retained"}"#.to_string()
        )]
    );
    assert_eq!(sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('agent_jobs', 'agent_job_items')",
    ).fetch_all(&state).await?, Vec::<String>::new());
    assert!(
        sqlx::query(
            "INSERT INTO better_legacy_agent_job_items \
        (job_id, item_id, row_index, row_json, status, created_at, updated_at) \
        VALUES ('missing', 'orphan', 1, '{}', 'pending', 1, 1)"
        )
        .execute(&state)
        .await
        .is_err()
    );
    state.close().await;
    Ok(())
}

#[tokio::test]
async fn imports_pending_input_in_order_once_and_preserves_pauses() -> anyhow::Result<()> {
    let directory = crate::runtime::test_support::unique_temp_dir();
    tokio::fs::create_dir_all(&directory).await?;
    let _cleanup = scopeguard::guard(directory.clone(), |path| {
        let _ = std::fs::remove_dir_all(path);
    });
    let sqlite = SqliteConfig::new_for_testing(directory.as_path().abs());
    let state = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
    let queue = sqlite.open_read_write_pool(&sqlite.queue_db_path()).await?;
    let mut previous = crate::migrations::runtime_state_migrator();
    previous.migrations = Cow::Owned(
        STATE_MIGRATOR
            .migrations
            .iter()
            .filter(|m| m.version < 10_004)
            .cloned()
            .collect(),
    );
    previous.run(&state).await?;
    QUEUE_MIGRATOR.run(&queue).await?;
    sqlx::raw_sql(
        r#"
        INSERT INTO thread_queue_items
            (id, thread_id, payload_json, payload_digest, client_user_message_id,
             queue_order, state, turn_id, terminal_status, created_at_ms, updated_at_ms)
        VALUES
            ('z', 'thread', '[{"type":"text","text":"first","text_elements":[]}]',
             'digest-z', 'client-z', 0, 'pending', NULL, NULL, 10, 20),
            ('a', 'thread', '[{"type":"text","text":"second","text_elements":[]}]',
             'digest-a', 'client-a', 1, 'pending', NULL, NULL, 11, 21),
            ('active', 'thread', '[]', 'active', 'active', 2, 'inflight', 'turn', NULL, 12, 22),
            ('done', 'thread', '[]', 'done', 'done', 3, 'terminal', 'done', 'completed', 13, 23),
            ('durable', 'thread', '[]', 'durable', 'durable', 4, 'pending', NULL, NULL, 14, 24);
        INSERT INTO thread_queue_controls
            (thread_id, paused_reason, updated_at_ms, blocked_submission_id, blocked_retry_allowed)
        VALUES ('thread', 'interrupted', 24, 'durable', 0);
        "#,
    )
    .execute(&state)
    .await?;
    STATE_MIGRATOR.run(&state).await?;
    import_queue(&state, &queue).await?;
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, payload_json FROM queued_items ORDER BY queue_order")
            .fetch_all(&queue)
            .await?;
    assert_eq!(
        rows,
        vec![
            (
                "z".to_string(),
                serde_json::to_string(&TurnInput::UserInput {
                    content: serde_json::from_str(
                        r#"[{"type":"text","text":"first","text_elements":[]}]"#
                    )?,
                    client_id: Some("client-z".to_string()),
                })?
            ),
            (
                "a".to_string(),
                serde_json::to_string(&TurnInput::UserInput {
                    content: serde_json::from_str(
                        r#"[{"type":"text","text":"second","text_elements":[]}]"#
                    )?,
                    client_id: Some("client-a".to_string()),
                })?
            ),
        ]
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT thread_id FROM better_queue_pauses")
            .fetch_all(&queue)
            .await?,
        vec!["thread".to_string()]
    );
    sqlx::query("DELETE FROM queued_items WHERE id = 'z'")
        .execute(&queue)
        .await?;
    sqlx::query("DELETE FROM better_queue_pauses")
        .execute(&queue)
        .await?;
    import_queue(&state, &queue).await?;
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT id FROM queued_items")
            .fetch_all(&queue)
            .await?,
        vec!["a".to_string()]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM better_queue_pauses")
            .fetch_one(&queue)
            .await?,
        0
    );
    assert!(
        sqlx::query("DELETE FROM thread_queue_items WHERE id = 'z'")
            .execute(&state)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM thread_queue_controls WHERE thread_id = 'thread'")
            .execute(&state)
            .await
            .is_err()
    );
    state.close().await;
    queue.close().await;
    Ok(())
}

#[tokio::test]
async fn repairs_only_matching_legacy_migration_checksums() -> anyhow::Result<()> {
    let directory = crate::runtime::test_support::unique_temp_dir();
    tokio::fs::create_dir_all(&directory).await?;
    let _cleanup = scopeguard::guard(directory.clone(), |path| {
        let _ = std::fs::remove_dir_all(path);
    });
    let sqlite = SqliteConfig::new_for_testing(directory.as_path().abs());
    let pool = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
    sqlx::raw_sql("CREATE TABLE _sqlx_migrations (version INTEGER PRIMARY KEY, description TEXT, checksum BLOB);")
        .execute(&pool).await?;
    for (old, new) in [(49, 10_001), (50, 10_002), (51, 10_003)] {
        let migration = STATE_MIGRATOR
            .migrations
            .iter()
            .find(|m| m.version == new)
            .unwrap();
        sqlx::query("INSERT INTO _sqlx_migrations VALUES (?, 'legacy', ?)")
            .bind(old)
            .bind(migration.checksum.as_ref())
            .execute(&pool)
            .await?;
    }
    repair_migrations(&pool, &STATE_MIGRATOR).await?;
    repair_migrations(&pool, &STATE_MIGRATOR).await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await?,
        vec![10_001, 10_002, 10_003]
    );
    sqlx::query("INSERT INTO _sqlx_migrations VALUES (49, 'upstream', X'1234')")
        .execute(&pool)
        .await?;
    repair_migrations(&pool, &STATE_MIGRATOR).await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await?,
        vec![49, 10_001, 10_002, 10_003]
    );
    pool.close().await;
    Ok(())
}
