use super::StateRuntime;
use super::open_state_sqlite;
use super::open_thread_history_db;
use super::runtime_state_migrator;
use super::sqlite_integrity_check;
use super::state_db_path;
use super::test_support::unique_temp_dir;
use super::thread_history_db_path;
use crate::DB_INIT_METRIC;
use crate::DbTelemetry;
use crate::migrations::STATE_MIGRATOR;
use crate::migrations::THREAD_HISTORY_MIGRATOR;
use pretty_assertions::assert_eq;
use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use sqlx::migrate::Migrator;
use sqlx::sqlite::SqliteConnectOptions;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

#[derive(Default)]
struct TestTelemetry {
    counters: Mutex<Vec<MetricEvent>>,
}

#[derive(Debug, Eq, PartialEq)]
struct MetricEvent {
    name: String,
    tags: BTreeMap<String, String>,
}

impl TestTelemetry {
    fn counters(&self) -> Vec<MetricEvent> {
        self.counters
            .lock()
            .expect("telemetry lock")
            .iter()
            .map(|event| MetricEvent {
                name: event.name.clone(),
                tags: event.tags.clone(),
            })
            .collect()
    }
}

impl DbTelemetry for TestTelemetry {
    fn counter(&self, name: &str, _inc: i64, tags: &[(&str, &str)]) {
        self.counters
            .lock()
            .expect("telemetry lock")
            .push(MetricEvent {
                name: name.to_string(),
                tags: tags_to_map(tags),
            });
    }

    fn record_duration(&self, _name: &str, _duration: std::time::Duration, _tags: &[(&str, &str)]) {
    }
}

fn tags_to_map(tags: &[(&str, &str)]) -> BTreeMap<String, String> {
    tags.iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

async fn open_db_pool(path: &Path) -> SqlitePool {
    SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(false),
    )
    .await
    .expect("open sqlite pool")
}

#[tokio::test]
async fn sqlite_integrity_check_reports_ok_for_valid_db() {
    let codex_home = unique_temp_dir();
    tokio::fs::create_dir_all(&codex_home)
        .await
        .expect("create codex home");
    let path = state_db_path(codex_home.as_path());
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .expect("open sqlite db");
    sqlx::query("CREATE TABLE sample (id INTEGER PRIMARY KEY)")
        .execute(&pool)
        .await
        .expect("create sample table");
    pool.close().await;

    let result = sqlite_integrity_check(&path)
        .await
        .expect("integrity check should run");

    assert_eq!(result, vec!["ok".to_string()]);
    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn open_state_sqlite_tolerates_newer_applied_migrations() {
    let codex_home = unique_temp_dir();
    tokio::fs::create_dir_all(&codex_home)
        .await
        .expect("create codex home");
    let state_path = state_db_path(codex_home.as_path());
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(&state_path)
            .create_if_missing(true),
    )
    .await
    .expect("open state db");
    STATE_MIGRATOR
        .run(&pool)
        .await
        .expect("apply current state schema");
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(9_999_i64)
    .bind("future migration")
    .bind(true)
    .bind(vec![1_u8, 2, 3, 4])
    .bind(1_i64)
    .execute(&pool)
    .await
    .expect("insert future migration record");
    pool.close().await;

    let strict_pool = open_db_pool(state_path.as_path()).await;
    let strict_err = STATE_MIGRATOR
        .run(&strict_pool)
        .await
        .expect_err("strict migrator should reject newer applied migrations");
    assert!(matches!(strict_err, MigrateError::VersionMissing(9_999)));
    strict_pool.close().await;

    let tolerant_migrator = runtime_state_migrator();
    let tolerant_pool = open_state_sqlite(
        state_path.as_path(),
        &tolerant_migrator,
        /*telemetry_override*/ None,
    )
    .await
    .expect("runtime migrator should tolerate newer applied migrations");
    tolerant_pool.close().await;

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn thread_history_abort_reason_migration_invalidates_interrupted_projections() {
    let codex_home = unique_temp_dir();
    tokio::fs::create_dir_all(&codex_home)
        .await
        .expect("create codex home");
    let history_path = thread_history_db_path(codex_home.as_path());
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(&history_path)
            .create_if_missing(true),
    )
    .await
    .expect("open old thread history db");
    let old_migrator = Migrator {
        migrations: Cow::Owned(
            THREAD_HISTORY_MIGRATOR
                .migrations
                .iter()
                .filter(|migration| migration.version <= 3)
                .cloned()
                .collect(),
        ),
        ignore_missing: THREAD_HISTORY_MIGRATOR.ignore_missing,
        locking: THREAD_HISTORY_MIGRATOR.locking,
        table_name: THREAD_HISTORY_MIGRATOR.table_name.clone(),
        create_schemas: THREAD_HISTORY_MIGRATOR.create_schemas.clone(),
        no_tx: THREAD_HISTORY_MIGRATOR.no_tx,
    };
    old_migrator
        .run(&pool)
        .await
        .expect("apply old thread history schema");
    for (thread_id, status) in [
        ("thread-completed", "completed"),
        ("thread-interrupted", "interrupted"),
    ] {
        sqlx::query(
            "INSERT INTO thread_turns (thread_id, turn_id, rollout_ordinal, status) VALUES (?, 'turn-1', 1, ?)",
        )
        .bind(thread_id)
        .bind(status)
        .execute(&pool)
        .await
        .expect("insert old projected turn");
        sqlx::query(
            "INSERT INTO thread_history_projection_state (thread_id, next_rollout_byte_offset, next_rollout_ordinal) VALUES (?, 100, 2)",
        )
        .bind(thread_id)
        .execute(&pool)
        .await
        .expect("insert old projection state");
    }
    pool.close().await;

    let pool = open_thread_history_db(codex_home.as_path())
        .await
        .expect("upgrade thread history db");
    let turns = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT thread_id, abort_reason FROM thread_turns ORDER BY thread_id",
    )
    .fetch_all(&pool)
    .await
    .expect("read upgraded turns");
    let projected_threads = sqlx::query_scalar::<_, String>(
        "SELECT thread_id FROM thread_history_projection_state ORDER BY thread_id",
    )
    .fetch_all(&pool)
    .await
    .expect("read upgraded projection states");
    let migration_applied = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 4 AND success = 1",
    )
    .fetch_one(&pool)
    .await
    .expect("read applied migration");
    assert_eq!(
        (turns, projected_threads, migration_applied),
        (
            vec![
                ("thread-completed".to_string(), None),
                ("thread-interrupted".to_string(), None),
            ],
            vec!["thread-completed".to_string()],
            1,
        )
    );
    pool.close().await;

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn init_records_successful_sqlite_init_phases_to_explicit_telemetry() {
    let codex_home = unique_temp_dir();
    let telemetry = TestTelemetry::default();

    let runtime = StateRuntime::init_with_telemetry_for_tests(
        codex_home.clone(),
        "test-provider".to_string(),
        &telemetry,
    )
    .await
    .expect("state runtime should initialize");

    let phases = telemetry
        .counters()
        .into_iter()
        .filter(|event| event.name == DB_INIT_METRIC)
        .filter(|event| event.tags.get("status").map(String::as_str) == Some("success"))
        .filter_map(|event| event.tags.get("phase").cloned())
        .collect::<BTreeSet<_>>();
    let expected = [
        "open_state",
        "migrate_state",
        "open_logs",
        "migrate_logs",
        "open_goals",
        "migrate_goals",
        "open_memories",
        "migrate_memories",
        "ensure_backfill_state",
        "post_init_query",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    assert_eq!(phases, expected);

    runtime.close().await;
    let _ = tokio::fs::remove_dir_all(codex_home).await;
}
