use std::borrow::Cow;

use sqlx::Row;
use sqlx::SqlStr;
use sqlx::SqlitePool;
use sqlx::migrate::Migration;
use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;

use super::STATE_MIGRATOR;
use super::repair_legacy_better_migration_versions;
use super::repair_legacy_recency_migration_version;
use super::runtime_state_migrator;

fn migrator_through(version: i64) -> Migrator {
    Migrator {
        migrations: Cow::Owned(
            STATE_MIGRATOR
                .migrations
                .iter()
                .filter(|migration| migration.version <= version)
                .cloned()
                .collect(),
        ),
        ignore_missing: STATE_MIGRATOR.ignore_missing,
        locking: STATE_MIGRATOR.locking,
        table_name: STATE_MIGRATOR.table_name.clone(),
        create_schemas: STATE_MIGRATOR.create_schemas.clone(),
        no_tx: STATE_MIGRATOR.no_tx,
    }
}

async fn insert_migration_thread(pool: &SqlitePool, id: &str, recency_at_ms: i64) {
    let recency_at = recency_at_ms / 1000;
    sqlx::query(
        r#"
INSERT INTO threads (
    id, rollout_path, created_at, updated_at, created_at_ms, updated_at_ms,
    recency_at, recency_at_ms, source, model_provider, cwd, title, preview,
    sandbox_policy, approval_mode
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(id)
    .bind(format!("/tmp/{id}.jsonl"))
    .bind(recency_at)
    .bind(recency_at)
    .bind(recency_at_ms)
    .bind(recency_at_ms)
    .bind(recency_at)
    .bind(recency_at_ms)
    .bind("cli")
    .bind("openai")
    .bind("/tmp")
    .bind("")
    .bind("visible")
    .bind("read-only")
    .bind("on-request")
    .execute(pool)
    .await
    .expect("migration fixture thread should insert");
}

#[tokio::test]
async fn queue_block_owner_migration_preserves_existing_pause_controls() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory database should open");
    migrator_through(/*version*/ 10_002)
        .run(&pool)
        .await
        .expect("thread queue schema should apply");
    sqlx::query(
        "INSERT INTO thread_queue_controls (thread_id, paused_reason, updated_at_ms) VALUES ('thread-1', 'interrupted', 1)",
    )
    .execute(&pool)
    .await
    .expect("legacy queue pause should insert");

    STATE_MIGRATOR
        .run(&pool)
        .await
        .expect("queue block owner migration should apply");

    let control = sqlx::query_as::<_, (String, Option<String>, Option<i64>)>(
        "SELECT paused_reason, blocked_submission_id, blocked_retry_allowed FROM thread_queue_controls WHERE thread_id = 'thread-1'",
    )
    .fetch_one(&pool)
    .await
    .expect("upgraded pause control should load");
    assert_eq!(control, ("interrupted".to_string(), None, None));
}

#[tokio::test]
async fn queue_block_owner_migration_guards_pre_upgrade_writers() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory database should open");
    STATE_MIGRATOR
        .run(&pool)
        .await
        .expect("thread queue schema should apply");
    sqlx::query(
        r#"
INSERT INTO thread_queue_items (
    id, thread_id, payload_json, payload_digest, client_user_message_id,
    queue_order, state, turn_id, terminal_status, created_at_ms, updated_at_ms
) VALUES
    ('forbidden-owner', 'thread-forbidden', 'owner', 'owner-digest', 'owner-client', 1, 'pending', NULL, NULL, 1, 1),
    ('forbidden-follower', 'thread-forbidden', 'follower', 'follower-digest', 'follower-client', 2, 'pending', NULL, NULL, 2, 2),
    ('allowed-owner', 'thread-allowed', 'allowed', 'allowed-digest', 'allowed-client', 1, 'pending', NULL, NULL, 1, 1)
        "#,
    )
    .execute(&pool)
    .await
    .expect("queue fixtures should insert");
    sqlx::query(
        r#"
INSERT INTO thread_queue_controls (
    thread_id, paused_reason, updated_at_ms, blocked_submission_id, blocked_retry_allowed
) VALUES
    ('thread-forbidden', 'interrupted', 1, 'forbidden-owner', 0),
    ('thread-allowed', 'interrupted', 1, 'allowed-owner', 1)
        "#,
    )
    .execute(&pool)
    .await
    .expect("blocked controls should insert");

    for (item_id, state) in [
        ("forbidden-owner", "inflight"),
        ("forbidden-follower", "starting"),
    ] {
        sqlx::query("UPDATE thread_queue_items SET state = ?, turn_id = 'old-turn' WHERE id = ?")
            .bind(state)
            .bind(item_id)
            .execute(&pool)
            .await
            .expect_err("pre-upgrade claim should be blocked");
    }
    sqlx::query(
        "UPDATE thread_queue_items SET payload_json = 'replaced', payload_digest = 'replaced-digest' WHERE id = 'forbidden-owner'",
    )
    .execute(&pool)
    .await
    .expect_err("pre-upgrade owner update should be blocked");
    let forbidden = sqlx::query_as::<_, (String, String, String, Option<String>)>(
        "SELECT payload_json, payload_digest, state, turn_id FROM thread_queue_items WHERE id = 'forbidden-owner'",
    )
    .fetch_one(&pool)
    .await
    .expect("forbidden owner should remain");
    assert_eq!(
        forbidden,
        (
            "owner".to_string(),
            "owner-digest".to_string(),
            "pending".to_string(),
            None,
        )
    );

    sqlx::query("DELETE FROM thread_queue_items WHERE id = 'forbidden-owner'")
        .execute(&pool)
        .await
        .expect("pre-upgrade owner deletion should succeed");
    let forbidden_control = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM thread_queue_controls WHERE thread_id = 'thread-forbidden'",
    )
    .fetch_optional(&pool)
    .await
    .expect("deleted owner control should query");
    assert_eq!(forbidden_control, None);
    assert_eq!(
        sqlx::query(
            "UPDATE thread_queue_items SET state = 'starting', turn_id = 'follower-turn' WHERE id = 'forbidden-follower'",
        )
        .execute(&pool)
        .await
        .expect("follower should claim after owner deletion")
        .rows_affected(),
        1
    );
    assert_eq!(
        sqlx::query(
            "UPDATE thread_queue_items SET state = 'starting', turn_id = 'allowed-turn' WHERE id = 'allowed-owner'",
        )
        .execute(&pool)
        .await
        .expect("retryable owner should remain claimable")
        .rows_affected(),
        1
    );
}

#[tokio::test]
async fn recency_migration_backfills_and_seeds_old_binary_inserts() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory database should open");
    migrator_through(/*version*/ 37)
        .run(&pool)
        .await
        .expect("pre-recency migrations should apply");

    sqlx::query(
        r#"
INSERT INTO threads (
    id,
    rollout_path,
    created_at,
    updated_at,
    created_at_ms,
    updated_at_ms,
    source,
    model_provider,
    cwd,
    title,
    sandbox_policy,
    approval_mode
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("00000000-0000-0000-0000-000000000001")
    .bind("/tmp/first.jsonl")
    .bind(1_700_000_000_i64)
    .bind(1_700_000_100_i64)
    .bind(1_700_000_000_123_i64)
    .bind(1_700_000_100_456_i64)
    .bind("cli")
    .bind("openai")
    .bind("/tmp")
    .bind("")
    .bind("read-only")
    .bind("on-request")
    .execute(&pool)
    .await
    .expect("legacy row should insert");

    STATE_MIGRATOR
        .run(&pool)
        .await
        .expect("recency migration should apply");

    let backfilled = sqlx::query(
        "SELECT updated_at, updated_at_ms, recency_at, recency_at_ms FROM threads WHERE id = ?",
    )
    .bind("00000000-0000-0000-0000-000000000001")
    .fetch_one(&pool)
    .await
    .expect("backfilled row should load");
    assert_eq!(backfilled.get::<i64, _>("recency_at"), 1_700_000_100);
    assert_eq!(backfilled.get::<i64, _>("recency_at_ms"), 1_700_000_100_456);

    sqlx::query(
        r#"
INSERT INTO threads (
    id,
    rollout_path,
    created_at,
    updated_at,
    created_at_ms,
    updated_at_ms,
    source,
    model_provider,
    cwd,
    title,
    sandbox_policy,
    approval_mode
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("00000000-0000-0000-0000-000000000002")
    .bind("/tmp/second.jsonl")
    .bind(1_700_000_200_i64)
    .bind(1_700_000_300_i64)
    .bind(1_700_000_200_123_i64)
    .bind(1_700_000_300_456_i64)
    .bind("cli")
    .bind("openai")
    .bind("/tmp")
    .bind("")
    .bind("read-only")
    .bind("on-request")
    .execute(&pool)
    .await
    .expect("old-binary row should insert");

    let seeded = sqlx::query("SELECT recency_at, recency_at_ms FROM threads WHERE id = ?")
        .bind("00000000-0000-0000-0000-000000000002")
        .fetch_one(&pool)
        .await
        .expect("old-binary row should load");
    assert_eq!(seeded.get::<i64, _>("recency_at"), 1_700_000_300);
    assert_eq!(seeded.get::<i64, _>("recency_at_ms"), 1_700_000_300_456);
}

#[tokio::test]
async fn repairs_recency_migration_that_was_applied_as_version_38() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory database should open");
    migrator_through(/*version*/ 37)
        .run(&pool)
        .await
        .expect("pre-recency migrations should apply");

    let recency_migration = STATE_MIGRATOR
        .migrations
        .iter()
        .find(|migration| migration.version == 39)
        .expect("recency migration should exist");
    let mut legacy_migrations = STATE_MIGRATOR
        .migrations
        .iter()
        .filter(|migration| migration.version <= 37)
        .cloned()
        .collect::<Vec<_>>();
    legacy_migrations.push(Migration::new(
        38,
        recency_migration.description.clone(),
        recency_migration.migration_type,
        recency_migration.sql.clone(),
        recency_migration.no_tx,
    ));
    let legacy_recency_migrator = Migrator::with_migrations(legacy_migrations);
    legacy_recency_migrator
        .run(&pool)
        .await
        .expect("legacy recency migration should apply as version 38");

    repair_legacy_recency_migration_version(&pool, &STATE_MIGRATOR)
        .await
        .expect("legacy migration history should be repaired");
    STATE_MIGRATOR
        .run(&pool)
        .await
        .expect("current migrations should apply after repair");

    let applied = sqlx::query(
        "SELECT version, checksum FROM _sqlx_migrations WHERE version >= 38 ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .expect("applied migrations should load")
    .into_iter()
    .map(|row| {
        (
            row.get::<i64, _>("version"),
            row.get::<Vec<u8>, _>("checksum"),
        )
    })
    .collect::<Vec<_>>();
    let expected = STATE_MIGRATOR
        .migrations
        .iter()
        .filter(|migration| migration.version >= 38)
        .map(|migration| (migration.version, migration.checksum.to_vec()))
        .collect::<Vec<_>>();
    assert_eq!(applied, expected);
}

#[tokio::test]
async fn legacy_pinning_and_section_membership_remain_independent() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory database should open");
    migrator_through(/*version*/ 43)
        .run(&pool)
        .await
        .expect("legacy pin migration should apply");
    insert_migration_thread(
        &pool,
        "00000000-0000-0000-0000-000000000041",
        /*recency_at_ms*/ 1_700_000_041_000,
    )
    .await;
    sqlx::query("UPDATE threads SET is_pinned = 1 WHERE id = ?")
        .bind("00000000-0000-0000-0000-000000000041")
        .execute(&pool)
        .await
        .expect("legacy pin should update");

    migrator_through(/*version*/ 45)
        .run(&pool)
        .await
        .expect("section migration should apply");
    insert_migration_thread(
        &pool,
        "00000000-0000-0000-0000-000000000042",
        /*recency_at_ms*/ 1_700_000_042_000,
    )
    .await;
    sqlx::query("UPDATE threads SET thread_section_id = ? WHERE id = ?")
        .bind(crate::PINNED_THREAD_SECTION_ID)
        .bind("00000000-0000-0000-0000-000000000042")
        .execute(&pool)
        .await
        .expect("section membership should update");

    STATE_MIGRATOR
        .run(&pool)
        .await
        .expect("remaining migrations should apply");
    let rows = sqlx::query_as::<_, (String, i64, Option<String>, Option<i64>, Option<i64>)>(
        "SELECT id, is_pinned, thread_section_id, section_position, section_entered_at_ms FROM threads ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .expect("pin and section state should load");
    assert_eq!(
        rows,
        vec![
            (
                "00000000-0000-0000-0000-000000000041".to_string(),
                1,
                None,
                None,
                None,
            ),
            (
                "00000000-0000-0000-0000-000000000042".to_string(),
                0,
                Some(crate::PINNED_THREAD_SECTION_ID.to_string()),
                Some(1_000_000),
                Some(1_700_000_042_000),
            ),
        ]
    );
}

#[tokio::test]
async fn section_order_migration_backfills_stable_ranks_and_usable_index() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory database should open");
    migrator_through(/*version*/ 45)
        .run(&pool)
        .await
        .expect("pre-order migrations should apply");
    for (id, recency_at_ms) in [
        ("00000000-0000-0000-0000-000000000051", 2_000),
        ("00000000-0000-0000-0000-000000000052", 2_000),
        ("00000000-0000-0000-0000-000000000053", 1_000),
    ] {
        insert_migration_thread(&pool, id, recency_at_ms).await;
        sqlx::query("UPDATE threads SET thread_section_id = ? WHERE id = ?")
            .bind(crate::PINNED_THREAD_SECTION_ID)
            .bind(id)
            .execute(&pool)
            .await
            .expect("section membership should update");
    }
    insert_migration_thread(
        &pool,
        "00000000-0000-0000-0000-000000000054",
        /*recency_at_ms*/ 3_000,
    )
    .await;

    migrator_through(/*version*/ 46)
        .run(&pool)
        .await
        .expect("section order migration should apply");
    let sectioned = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT id, section_position, section_entered_at_ms FROM threads WHERE thread_section_id = ? ORDER BY section_position",
    )
    .bind(crate::PINNED_THREAD_SECTION_ID)
    .fetch_all(&pool)
    .await
    .expect("backfilled section order should load");
    assert_eq!(
        sectioned,
        vec![
            (
                "00000000-0000-0000-0000-000000000052".to_string(),
                1_000_000,
                2_000,
            ),
            (
                "00000000-0000-0000-0000-000000000051".to_string(),
                2_000_000,
                2_000,
            ),
            (
                "00000000-0000-0000-0000-000000000053".to_string(),
                3_000_000,
                1_000,
            ),
        ]
    );
    assert_eq!(
        sqlx::query_as::<_, (Option<i64>, Option<i64>)>(
            "SELECT section_position, section_entered_at_ms FROM threads WHERE id = ?",
        )
        .bind("00000000-0000-0000-0000-000000000054")
        .fetch_one(&pool)
        .await
        .expect("unsectioned order should load"),
        (None, None)
    );
    let query_plan = sqlx::query(
        "EXPLAIN QUERY PLAN SELECT id FROM threads WHERE archived = 0 AND thread_section_id = ? AND preview <> '' ORDER BY section_position ASC, id ASC LIMIT 10",
    )
    .bind(crate::PINNED_THREAD_SECTION_ID)
    .fetch_all(&pool)
    .await
    .expect("section order query plan should load");
    assert!(query_plan.iter().any(|row| {
        row.get::<String, _>("detail")
            .contains("idx_threads_section_position")
    }));
}

#[tokio::test]
async fn migration_preserves_better_agent_jobs_without_claiming_upstream_0042() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory database should open");
    migrator_through(/*version*/ 41)
        .run(&pool)
        .await
        .expect("Better migrations through 0041 should apply");

    sqlx::query(
        r#"
INSERT INTO agent_jobs (
    id, name, status, instruction, input_headers_json, input_csv_path,
    output_csv_path, created_at, updated_at, max_runtime_seconds
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("job-1")
    .bind("durable job")
    .bind("running")
    .bind("keep processing")
    .bind("[]")
    .bind("/tmp/input.csv")
    .bind("/tmp/output.csv")
    .bind(1_700_000_000_i64)
    .bind(1_700_000_100_i64)
    .bind(300_i64)
    .execute(&pool)
    .await
    .expect("legacy job should insert");
    sqlx::query(
        r#"
INSERT INTO agent_job_items (
    job_id, item_id, row_index, row_json, status, created_at, updated_at
) VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("job-1")
    .bind("item-1")
    .bind(7_i64)
    .bind(r#"{"prompt":"persist me"}"#)
    .bind("pending")
    .bind(1_700_000_000_i64)
    .bind(1_700_000_100_i64)
    .execute(&pool)
    .await
    .expect("legacy job item should insert");

    STATE_MIGRATOR
        .run(&pool)
        .await
        .expect("current migrations should preserve Better agent jobs");

    let job = sqlx::query_as::<_, (String, String, i64, Option<i64>)>(
        "SELECT name, status, auto_export, max_runtime_seconds FROM agent_jobs WHERE id = ?",
    )
    .bind("job-1")
    .fetch_one(&pool)
    .await
    .expect("preserved job should load");
    assert_eq!(
        job,
        (
            "durable job".to_string(),
            "running".to_string(),
            1,
            Some(300),
        )
    );
    let item = sqlx::query_as::<_, (String, i64, String, String)>(
        "SELECT item_id, row_index, row_json, status FROM agent_job_items WHERE job_id = ?",
    )
    .bind("job-1")
    .fetch_one(&pool)
    .await
    .expect("preserved job item should load");
    assert_eq!(
        item,
        (
            "item-1".to_string(),
            7,
            r#"{"prompt":"persist me"}"#.to_string(),
            "pending".to_string(),
        )
    );
    assert!(
        STATE_MIGRATOR
            .migrations
            .iter()
            .all(|migration| migration.version != 42)
    );
}

#[tokio::test]
async fn runtime_migration_restores_agent_jobs_after_upstream_0042() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory database should open");
    let mut upstream_migrations = STATE_MIGRATOR
        .migrations
        .iter()
        .filter(|migration| migration.version <= 48)
        .cloned()
        .collect::<Vec<_>>();
    let predecessor = upstream_migrations
        .iter()
        .find(|migration| migration.version == 41)
        .expect("migration 0041 should exist");
    let migration_type = predecessor.migration_type;
    let no_tx = predecessor.no_tx;
    upstream_migrations.push(Migration::new(
        42,
        Cow::Borrowed("drop agent jobs"),
        migration_type,
        SqlStr::from_static(
            "DROP TABLE IF EXISTS agent_job_items;\nDROP TABLE IF EXISTS agent_jobs;\n",
        ),
        no_tx,
    ));
    upstream_migrations.sort_by_key(|migration| migration.version);
    Migrator::with_migrations(upstream_migrations)
        .run(&pool)
        .await
        .expect("upstream migrations through 0048 should apply");

    let dropped_tables = sqlx::query_scalar::<_, i64>(
        r#"
SELECT COUNT(*) FROM sqlite_master
WHERE type = 'table' AND name IN ('agent_jobs', 'agent_job_items')
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("dropped tables should be inspected");
    assert_eq!(dropped_tables, 0);

    runtime_state_migrator()
        .run(&pool)
        .await
        .expect("runtime migration should tolerate 0042 and apply 10001");
    sqlx::query(
        r#"
INSERT INTO agent_jobs (
    id, name, status, instruction, input_headers_json, input_csv_path,
    output_csv_path, created_at, updated_at, max_runtime_seconds
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("job-restored")
    .bind("restored job")
    .bind("pending")
    .bind("resume processing")
    .bind("[]")
    .bind("/tmp/input.csv")
    .bind("/tmp/output.csv")
    .bind(1_700_000_200_i64)
    .bind(1_700_000_200_i64)
    .bind(600_i64)
    .execute(&pool)
    .await
    .expect("restored agent job schema should accept current rows");
    sqlx::query(
        r#"
INSERT INTO agent_job_items (
    job_id, item_id, row_index, row_json, status, created_at, updated_at
) VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("job-restored")
    .bind("item-restored")
    .bind(0_i64)
    .bind("{}")
    .bind("pending")
    .bind(1_700_000_200_i64)
    .bind(1_700_000_200_i64)
    .execute(&pool)
    .await
    .expect("restored agent job item schema should accept current rows");

    let restored = sqlx::query_as::<_, (String, Option<i64>, String)>(
        r#"
SELECT jobs.name, jobs.max_runtime_seconds, items.item_id
FROM agent_jobs AS jobs
JOIN agent_job_items AS items ON items.job_id = jobs.id
WHERE jobs.id = ?
        "#,
    )
    .bind("job-restored")
    .fetch_one(&pool)
    .await
    .expect("restored job should load");
    assert_eq!(
        restored,
        (
            "restored job".to_string(),
            Some(600),
            "item-restored".to_string(),
        )
    );
}

#[tokio::test]
async fn repairs_legacy_better_migration_versions_before_applying_upstream_collisions() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory database should open");
    let mut legacy_migrations = STATE_MIGRATOR
        .migrations
        .iter()
        .filter(|migration| migration.version <= 48)
        .cloned()
        .collect::<Vec<_>>();
    for (legacy_version, reserved_version) in [(49, 10_001), (50, 10_002), (51, 10_003)] {
        let reserved_migration = STATE_MIGRATOR
            .migrations
            .iter()
            .find(|migration| migration.version == reserved_version)
            .expect("reserved Better migration should exist");
        legacy_migrations.push(Migration::new(
            legacy_version,
            reserved_migration.description.clone(),
            reserved_migration.migration_type,
            reserved_migration.sql.clone(),
            reserved_migration.no_tx,
        ));
    }
    Migrator::with_migrations(legacy_migrations)
        .run(&pool)
        .await
        .expect("legacy Better migrations should apply at colliding versions");

    repair_legacy_better_migration_versions(&pool, &STATE_MIGRATOR)
        .await
        .expect("legacy Better migration history should be repaired");
    runtime_state_migrator()
        .run(&pool)
        .await
        .expect("upstream and reserved Better migrations should coexist");

    let applied = sqlx::query_as::<_, (i64, Vec<u8>)>(
        "SELECT version, checksum FROM _sqlx_migrations WHERE version >= 49 ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .expect("repaired migrations should load");
    let expected = STATE_MIGRATOR
        .migrations
        .iter()
        .filter(|migration| migration.version >= 49)
        .map(|migration| (migration.version, migration.checksum.to_vec()))
        .collect::<Vec<_>>();
    assert_eq!(applied, expected);
    let tables = sqlx::query_scalar::<_, String>(
        r#"
SELECT name FROM sqlite_master
WHERE type = 'table'
  AND name IN ('projects', 'thread_artifacts', 'thread_queue_items', 'thread_queue_controls')
ORDER BY name
        "#,
    )
    .fetch_all(&pool)
    .await
    .expect("upstream and Better tables should load");
    assert_eq!(
        tables,
        vec![
            "projects".to_string(),
            "thread_artifacts".to_string(),
            "thread_queue_controls".to_string(),
            "thread_queue_items".to_string(),
        ]
    );
}

#[tokio::test]
async fn upstream_migrations_0049_through_0052_accept_reserved_better_migrations() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory database should open");
    migrator_through(/*version*/ 52)
        .run(&pool)
        .await
        .expect("upstream migrations through 0052 should apply");

    repair_legacy_better_migration_versions(&pool, &STATE_MIGRATOR)
        .await
        .expect("genuine upstream migration history should remain unchanged");
    runtime_state_migrator()
        .run(&pool)
        .await
        .expect("reserved Better migrations should apply after upstream 0052");

    let applied_versions = sqlx::query_scalar::<_, i64>(
        "SELECT version FROM _sqlx_migrations WHERE version >= 49 ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .expect("migration versions should load");
    assert_eq!(
        applied_versions,
        vec![49, 50, 51, 52, 10_001, 10_002, 10_003]
    );
}
