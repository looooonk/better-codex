use super::StateRuntime;
use super::format_feedback_log_line;
use super::test_support::unique_temp_dir;
use crate::LogEntry;
use crate::LogQuery;
use crate::logs_db_path;
use crate::migrations::LOGS_MIGRATOR;
use chrono::Utc;
use pretty_assertions::assert_eq;
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use sqlx::sqlite::SqliteConnectOptions;
use std::borrow::Cow;
use std::path::Path;

async fn open_db_pool(path: &Path) -> SqlitePool {
    SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(false),
    )
    .await
    .expect("open sqlite pool")
}

async fn log_row_count(path: &Path) -> i64 {
    let pool = open_db_pool(path).await;
    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM logs")
        .fetch_one(&pool)
        .await
        .expect("count log rows");
    pool.close().await;
    count
}

#[tokio::test]
async fn insert_logs_use_dedicated_log_database() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .insert_logs(&[LogEntry {
            ts: 1,
            ts_nanos: 0,
            level: "INFO".to_string(),
            target: "cli".to_string(),
            message: Some("dedicated-log-db".to_string()),
            feedback_log_body: Some("dedicated-log-db".to_string()),
            thread_id: Some("thread-1".to_string()),
            process_uuid: Some("proc-1".to_string()),
            module_path: Some("mod".to_string()),
            file: Some("main.rs".to_string()),
            line: Some(7),
        }])
        .await
        .expect("insert test logs");

    let logs_count = log_row_count(logs_db_path(codex_home.as_path()).as_path()).await;

    assert_eq!(logs_count, 1);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn init_migrates_message_only_logs_db_to_feedback_log_body_schema() {
    let codex_home = unique_temp_dir();
    tokio::fs::create_dir_all(&codex_home)
        .await
        .expect("create codex home");
    let logs_path = logs_db_path(codex_home.as_path());
    let old_logs_migrator = Migrator {
        migrations: Cow::Owned(vec![LOGS_MIGRATOR.migrations[0].clone()]),
        ignore_missing: false,
        locking: true,
        no_tx: false,
        table_name: LOGS_MIGRATOR.table_name.clone(),
        create_schemas: LOGS_MIGRATOR.create_schemas.clone(),
    };
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(&logs_path)
            .create_if_missing(true),
    )
    .await
    .expect("open old logs db");
    old_logs_migrator
        .run(&pool)
        .await
        .expect("apply old logs schema");
    sqlx::query(
        "INSERT INTO logs (ts, ts_nanos, level, target, message, module_path, file, line, thread_id, process_uuid, estimated_bytes) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Utc::now().timestamp())
    .bind(0_i64)
    .bind("INFO")
    .bind("cli")
    .bind("legacy-body")
    .bind("mod")
    .bind("main.rs")
    .bind(7_i64)
    .bind("thread-1")
    .bind("proc-1")
    .bind(16_i64)
    .execute(&pool)
    .await
    .expect("insert legacy log row");
    pool.close().await;
    drop(pool);

    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let rows = runtime
        .query_logs(&LogQuery::default())
        .await
        .expect("query migrated logs");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].message.as_deref(), Some("legacy-body"));

    let migrated_pool = open_db_pool(logs_path.as_path()).await;
    let columns = sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info('logs')")
        .fetch_all(&migrated_pool)
        .await
        .expect("load migrated columns");
    assert_eq!(
        columns,
        vec![
            "id".to_string(),
            "ts".to_string(),
            "ts_nanos".to_string(),
            "level".to_string(),
            "target".to_string(),
            "feedback_log_body".to_string(),
            "module_path".to_string(),
            "file".to_string(),
            "line".to_string(),
            "thread_id".to_string(),
            "process_uuid".to_string(),
            "estimated_bytes".to_string(),
        ]
    );
    let indexes =
        sqlx::query_scalar::<_, String>("SELECT name FROM pragma_index_list('logs') ORDER BY name")
            .fetch_all(&migrated_pool)
            .await
            .expect("load migrated indexes");
    assert_eq!(
        indexes,
        vec![
            "idx_logs_process_uuid_threadless_ts".to_string(),
            "idx_logs_thread_id".to_string(),
            "idx_logs_thread_id_ts".to_string(),
            "idx_logs_ts".to_string(),
        ]
    );
    migrated_pool.close().await;

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn init_configures_logs_db_with_incremental_auto_vacuum() {
    let codex_home = unique_temp_dir();
    let _runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let pool = open_db_pool(logs_db_path(codex_home.as_path()).as_path()).await;
    let auto_vacuum = sqlx::query_scalar::<_, i64>("PRAGMA auto_vacuum")
        .fetch_one(&pool)
        .await
        .expect("read auto_vacuum pragma");
    assert_eq!(auto_vacuum, 2);
    pool.close().await;

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[test]
fn format_feedback_log_line_matches_feedback_formatter_shape() {
    assert_eq!(
        format_feedback_log_line(
            /*ts*/ 1,
            /*ts_nanos*/ 123_456_000,
            "INFO",
            "alpha"
        ),
        "1970-01-01T00:00:01.123456Z  INFO alpha\n"
    );
}

#[test]
fn format_feedback_log_line_preserves_existing_trailing_newline() {
    assert_eq!(
        format_feedback_log_line(
            /*ts*/ 1,
            /*ts_nanos*/ 123_456_000,
            "INFO",
            "alpha\n"
        ),
        "1970-01-01T00:00:01.123456Z  INFO alpha\n"
    );
}

#[tokio::test]
async fn query_logs_with_search_matches_rendered_body_substring() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .insert_logs(&[
            LogEntry {
                ts: 1_700_000_001,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("alpha".to_string()),
                feedback_log_body: Some("foo=1 alpha".to_string()),
                thread_id: Some("thread-1".to_string()),
                process_uuid: None,
                file: Some("main.rs".to_string()),
                line: Some(42),
                module_path: None,
            },
            LogEntry {
                ts: 1_700_000_002,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("alphabet".to_string()),
                feedback_log_body: Some("foo=2 alphabet".to_string()),
                thread_id: Some("thread-1".to_string()),
                process_uuid: None,
                file: Some("main.rs".to_string()),
                line: Some(43),
                module_path: None,
            },
        ])
        .await
        .expect("insert test logs");

    let rows = runtime
        .query_logs(&LogQuery {
            search: Some("foo=2".to_string()),
            ..Default::default()
        })
        .await
        .expect("query matching logs");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].message.as_deref(), Some("foo=2 alphabet"));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn query_logs_filters_level_set_without_rewriting_stored_level() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .insert_logs(&[
            LogEntry {
                ts: 1,
                ts_nanos: 0,
                level: "TRACE".to_string(),
                target: "cli".to_string(),
                message: Some("trace-row".to_string()),
                feedback_log_body: Some("trace-row".to_string()),
                thread_id: None,
                process_uuid: None,
                file: Some("main.rs".to_string()),
                line: Some(1),
                module_path: None,
            },
            LogEntry {
                ts: 2,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("info-row".to_string()),
                feedback_log_body: Some("info-row".to_string()),
                thread_id: None,
                process_uuid: None,
                file: Some("main.rs".to_string()),
                line: Some(2),
                module_path: None,
            },
            LogEntry {
                ts: 3,
                ts_nanos: 0,
                level: "warn".to_string(),
                target: "cli".to_string(),
                message: Some("warn-row".to_string()),
                feedback_log_body: Some("warn-row".to_string()),
                thread_id: None,
                process_uuid: None,
                file: Some("main.rs".to_string()),
                line: Some(3),
                module_path: None,
            },
            LogEntry {
                ts: 4,
                ts_nanos: 0,
                level: "ERROR".to_string(),
                target: "cli".to_string(),
                message: Some("error-row".to_string()),
                feedback_log_body: Some("error-row".to_string()),
                thread_id: None,
                process_uuid: None,
                file: Some("main.rs".to_string()),
                line: Some(4),
                module_path: None,
            },
        ])
        .await
        .expect("insert test logs");

    let rows = runtime
        .query_logs(&LogQuery {
            levels_upper: vec!["WARN".to_string(), "ERROR".to_string()],
            ..Default::default()
        })
        .await
        .expect("query matching logs");
    let actual = rows
        .iter()
        .map(|row| (row.level.as_str(), row.message.as_deref()))
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![("warn", Some("warn-row")), ("ERROR", Some("error-row"))]
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn insert_logs_prunes_old_rows_when_thread_exceeds_size_limit() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let six_mebibytes = "a".repeat(6 * 1024 * 1024);
    runtime
        .insert_logs(&[
            LogEntry {
                ts: 1,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("small".to_string()),
                feedback_log_body: Some(six_mebibytes.clone()),
                thread_id: Some("thread-1".to_string()),
                process_uuid: Some("proc-1".to_string()),
                file: Some("main.rs".to_string()),
                line: Some(1),
                module_path: Some("mod".to_string()),
            },
            LogEntry {
                ts: 2,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("small".to_string()),
                feedback_log_body: Some(six_mebibytes.clone()),
                thread_id: Some("thread-1".to_string()),
                process_uuid: Some("proc-1".to_string()),
                file: Some("main.rs".to_string()),
                line: Some(2),
                module_path: Some("mod".to_string()),
            },
        ])
        .await
        .expect("insert test logs");

    let rows = runtime
        .query_logs(&LogQuery {
            thread_ids: vec!["thread-1".to_string()],
            ..Default::default()
        })
        .await
        .expect("query thread logs");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ts, 2);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn insert_logs_prunes_single_thread_row_when_it_exceeds_size_limit() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let eleven_mebibytes = "d".repeat(11 * 1024 * 1024);
    runtime
        .insert_logs(&[LogEntry {
            ts: 1,
            ts_nanos: 0,
            level: "INFO".to_string(),
            target: "cli".to_string(),
            message: Some("small".to_string()),
            feedback_log_body: Some(eleven_mebibytes),
            thread_id: Some("thread-oversized".to_string()),
            process_uuid: Some("proc-1".to_string()),
            file: Some("main.rs".to_string()),
            line: Some(1),
            module_path: Some("mod".to_string()),
        }])
        .await
        .expect("insert test log");

    let rows = runtime
        .query_logs(&LogQuery {
            thread_ids: vec!["thread-oversized".to_string()],
            ..Default::default()
        })
        .await
        .expect("query thread logs");

    assert!(rows.is_empty());

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn insert_logs_prunes_threadless_rows_per_process_uuid_only() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let six_mebibytes = "b".repeat(6 * 1024 * 1024);
    runtime
        .insert_logs(&[
            LogEntry {
                ts: 1,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some(six_mebibytes.clone()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-1".to_string()),
                file: Some("main.rs".to_string()),
                line: Some(1),
                module_path: Some("mod".to_string()),
            },
            LogEntry {
                ts: 2,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some(six_mebibytes.clone()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-1".to_string()),
                file: Some("main.rs".to_string()),
                line: Some(2),
                module_path: Some("mod".to_string()),
            },
            LogEntry {
                ts: 3,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some(six_mebibytes),
                feedback_log_body: None,
                thread_id: Some("thread-1".to_string()),
                process_uuid: Some("proc-1".to_string()),
                file: Some("main.rs".to_string()),
                line: Some(3),
                module_path: Some("mod".to_string()),
            },
        ])
        .await
        .expect("insert test logs");

    let rows = runtime
        .query_logs(&LogQuery {
            thread_ids: vec!["thread-1".to_string()],
            include_threadless: true,
            ..Default::default()
        })
        .await
        .expect("query thread and threadless logs");

    let mut timestamps: Vec<i64> = rows.into_iter().map(|row| row.ts).collect();
    timestamps.sort_unstable();
    assert_eq!(timestamps, vec![2, 3]);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn insert_logs_prunes_single_threadless_process_row_when_it_exceeds_size_limit() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let eleven_mebibytes = "e".repeat(11 * 1024 * 1024);
    runtime
        .insert_logs(&[LogEntry {
            ts: 1,
            ts_nanos: 0,
            level: "INFO".to_string(),
            target: "cli".to_string(),
            message: Some("small".to_string()),
            feedback_log_body: Some(eleven_mebibytes),
            thread_id: None,
            process_uuid: Some("proc-oversized".to_string()),
            file: Some("main.rs".to_string()),
            line: Some(1),
            module_path: Some("mod".to_string()),
        }])
        .await
        .expect("insert test log");

    let rows = runtime
        .query_logs(&LogQuery {
            include_threadless: true,
            ..Default::default()
        })
        .await
        .expect("query threadless logs");

    assert!(rows.is_empty());

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn insert_logs_prunes_threadless_rows_with_null_process_uuid() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let six_mebibytes = "c".repeat(6 * 1024 * 1024);
    runtime
        .insert_logs(&[
            LogEntry {
                ts: 1,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some(six_mebibytes.clone()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: None,
                file: Some("main.rs".to_string()),
                line: Some(1),
                module_path: Some("mod".to_string()),
            },
            LogEntry {
                ts: 2,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some(six_mebibytes),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: None,
                file: Some("main.rs".to_string()),
                line: Some(2),
                module_path: Some("mod".to_string()),
            },
            LogEntry {
                ts: 3,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("small".to_string()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-1".to_string()),
                file: Some("main.rs".to_string()),
                line: Some(3),
                module_path: Some("mod".to_string()),
            },
        ])
        .await
        .expect("insert test logs");

    let rows = runtime
        .query_logs(&LogQuery {
            include_threadless: true,
            ..Default::default()
        })
        .await
        .expect("query threadless logs");

    let mut timestamps: Vec<i64> = rows.into_iter().map(|row| row.ts).collect();
    timestamps.sort_unstable();
    assert_eq!(timestamps, vec![2, 3]);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn insert_logs_prunes_single_threadless_null_process_row_when_it_exceeds_limit() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let eleven_mebibytes = "f".repeat(11 * 1024 * 1024);
    runtime
        .insert_logs(&[LogEntry {
            ts: 1,
            ts_nanos: 0,
            level: "INFO".to_string(),
            target: "cli".to_string(),
            message: Some("small".to_string()),
            feedback_log_body: Some(eleven_mebibytes),
            thread_id: None,
            process_uuid: None,
            file: Some("main.rs".to_string()),
            line: Some(1),
            module_path: Some("mod".to_string()),
        }])
        .await
        .expect("insert test log");

    let rows = runtime
        .query_logs(&LogQuery {
            include_threadless: true,
            ..Default::default()
        })
        .await
        .expect("query threadless logs");

    assert!(rows.is_empty());

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn insert_logs_prunes_old_rows_when_thread_exceeds_row_limit() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let entries: Vec<LogEntry> = (1..=1_001)
        .map(|ts| LogEntry {
            ts,
            ts_nanos: 0,
            level: "INFO".to_string(),
            target: "cli".to_string(),
            message: Some(format!("thread-row-{ts}")),
            feedback_log_body: None,
            thread_id: Some("thread-row-limit".to_string()),
            process_uuid: Some("proc-1".to_string()),
            file: Some("main.rs".to_string()),
            line: Some(ts),
            module_path: Some("mod".to_string()),
        })
        .collect();
    runtime
        .insert_logs(&entries)
        .await
        .expect("insert test logs");

    let rows = runtime
        .query_logs(&LogQuery {
            thread_ids: vec!["thread-row-limit".to_string()],
            ..Default::default()
        })
        .await
        .expect("query thread logs");

    let timestamps: Vec<i64> = rows.into_iter().map(|row| row.ts).collect();
    assert_eq!(timestamps.len(), 1_000);
    assert_eq!(timestamps.first().copied(), Some(2));
    assert_eq!(timestamps.last().copied(), Some(1_001));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn insert_logs_prunes_old_threadless_rows_when_process_exceeds_row_limit() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let entries: Vec<LogEntry> = (1..=1_001)
        .map(|ts| LogEntry {
            ts,
            ts_nanos: 0,
            level: "INFO".to_string(),
            target: "cli".to_string(),
            message: Some(format!("process-row-{ts}")),
            feedback_log_body: None,
            thread_id: None,
            process_uuid: Some("proc-row-limit".to_string()),
            file: Some("main.rs".to_string()),
            line: Some(ts),
            module_path: Some("mod".to_string()),
        })
        .collect();
    runtime
        .insert_logs(&entries)
        .await
        .expect("insert test logs");

    let rows = runtime
        .query_logs(&LogQuery {
            include_threadless: true,
            ..Default::default()
        })
        .await
        .expect("query threadless logs");

    let timestamps: Vec<i64> = rows
        .into_iter()
        .filter(|row| row.process_uuid.as_deref() == Some("proc-row-limit"))
        .map(|row| row.ts)
        .collect();
    assert_eq!(timestamps.len(), 1_000);
    assert_eq!(timestamps.first().copied(), Some(2));
    assert_eq!(timestamps.last().copied(), Some(1_001));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn insert_logs_prunes_old_threadless_null_process_rows_when_row_limit_exceeded() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let entries: Vec<LogEntry> = (1..=1_001)
        .map(|ts| LogEntry {
            ts,
            ts_nanos: 0,
            level: "INFO".to_string(),
            target: "cli".to_string(),
            message: Some(format!("null-process-row-{ts}")),
            feedback_log_body: None,
            thread_id: None,
            process_uuid: None,
            file: Some("main.rs".to_string()),
            line: Some(ts),
            module_path: Some("mod".to_string()),
        })
        .collect();
    runtime
        .insert_logs(&entries)
        .await
        .expect("insert test logs");

    let rows = runtime
        .query_logs(&LogQuery {
            include_threadless: true,
            ..Default::default()
        })
        .await
        .expect("query threadless logs");

    let timestamps: Vec<i64> = rows
        .into_iter()
        .filter(|row| row.process_uuid.is_none())
        .map(|row| row.ts)
        .collect();
    assert_eq!(timestamps.len(), 1_000);
    assert_eq!(timestamps.first().copied(), Some(2));
    assert_eq!(timestamps.last().copied(), Some(1_001));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn query_feedback_logs_returns_newest_lines_within_limit_in_order() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .insert_logs(&[
            LogEntry {
                ts: 1,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("alpha".to_string()),
                feedback_log_body: None,
                thread_id: Some("thread-1".to_string()),
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 2,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("bravo".to_string()),
                feedback_log_body: None,
                thread_id: Some("thread-1".to_string()),
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 3,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("charlie".to_string()),
                feedback_log_body: None,
                thread_id: Some("thread-1".to_string()),
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
        ])
        .await
        .expect("insert test logs");

    let bytes = runtime
        .query_feedback_logs("thread-1")
        .await
        .expect("query feedback logs");

    assert_eq!(
        String::from_utf8(bytes).expect("valid utf-8"),
        [
            format_feedback_log_line(/*ts*/ 1, /*ts_nanos*/ 0, "INFO", "alpha"),
            format_feedback_log_line(/*ts*/ 2, /*ts_nanos*/ 0, "INFO", "bravo"),
            format_feedback_log_line(/*ts*/ 3, /*ts_nanos*/ 0, "INFO", "charlie"),
        ]
        .concat()
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn query_feedback_logs_excludes_oversized_newest_row() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");
    let eleven_mebibytes = "z".repeat(11 * 1024 * 1024);

    runtime
        .insert_logs(&[
            LogEntry {
                ts: 1,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("small".to_string()),
                feedback_log_body: None,
                thread_id: Some("thread-oversized".to_string()),
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 2,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some(eleven_mebibytes),
                feedback_log_body: None,
                thread_id: Some("thread-oversized".to_string()),
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
        ])
        .await
        .expect("insert test logs");

    let bytes = runtime
        .query_feedback_logs("thread-oversized")
        .await
        .expect("query feedback logs");

    assert_eq!(bytes, Vec::<u8>::new());

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn query_feedback_logs_includes_threadless_rows_from_same_process() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .insert_logs(&[
            LogEntry {
                ts: 1,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("threadless-before".to_string()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 2,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("thread-scoped".to_string()),
                feedback_log_body: None,
                thread_id: Some("thread-1".to_string()),
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 3,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("threadless-after".to_string()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 4,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("other-process-threadless".to_string()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-2".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
        ])
        .await
        .expect("insert test logs");

    let bytes = runtime
        .query_feedback_logs("thread-1")
        .await
        .expect("query feedback logs");

    assert_eq!(
        String::from_utf8(bytes).expect("valid utf-8"),
        [
            format_feedback_log_line(
                /*ts*/ 1,
                /*ts_nanos*/ 0,
                "INFO",
                "threadless-before"
            ),
            format_feedback_log_line(/*ts*/ 2, /*ts_nanos*/ 0, "INFO", "thread-scoped"),
            format_feedback_log_line(
                /*ts*/ 3,
                /*ts_nanos*/ 0,
                "INFO",
                "threadless-after"
            ),
        ]
        .concat()
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn query_feedback_logs_excludes_threadless_rows_from_prior_processes() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .insert_logs(&[
            LogEntry {
                ts: 1,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("old-process-threadless".to_string()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-old".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 2,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("old-process-thread".to_string()),
                feedback_log_body: None,
                thread_id: Some("thread-1".to_string()),
                process_uuid: Some("proc-old".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 3,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("new-process-thread".to_string()),
                feedback_log_body: None,
                thread_id: Some("thread-1".to_string()),
                process_uuid: Some("proc-new".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 4,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("new-process-threadless".to_string()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-new".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
        ])
        .await
        .expect("insert test logs");

    let bytes = runtime
        .query_feedback_logs("thread-1")
        .await
        .expect("query feedback logs");

    assert_eq!(
        String::from_utf8(bytes).expect("valid utf-8"),
        [
            format_feedback_log_line(
                /*ts*/ 2,
                /*ts_nanos*/ 0,
                "INFO",
                "old-process-thread"
            ),
            format_feedback_log_line(
                /*ts*/ 3,
                /*ts_nanos*/ 0,
                "INFO",
                "new-process-thread"
            ),
            format_feedback_log_line(
                /*ts*/ 4,
                /*ts_nanos*/ 0,
                "INFO",
                "new-process-threadless"
            ),
        ]
        .concat()
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn query_feedback_logs_keeps_newest_suffix_across_thread_and_threadless_logs() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");
    let thread_marker = "thread-scoped-oldest";
    let threadless_older_marker = "threadless-older";
    let threadless_newer_marker = "threadless-newer";
    let five_mebibytes = format!("{threadless_older_marker} {}", "a".repeat(5 * 1024 * 1024));
    let four_and_half_mebibytes = format!(
        "{threadless_newer_marker} {}",
        "b".repeat((9 * 1024 * 1024) / 2)
    );
    let one_mebibyte = format!("{thread_marker} {}", "c".repeat(1024 * 1024));

    runtime
        .insert_logs(&[
            LogEntry {
                ts: 1,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some(one_mebibyte.clone()),
                feedback_log_body: None,
                thread_id: Some("thread-1".to_string()),
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 2,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some(five_mebibytes),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 3,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some(four_and_half_mebibytes),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
        ])
        .await
        .expect("insert test logs");

    let bytes = runtime
        .query_feedback_logs("thread-1")
        .await
        .expect("query feedback logs");
    let logs = String::from_utf8(bytes).expect("valid utf-8");

    assert!(!logs.contains(thread_marker));
    assert!(logs.contains(threadless_older_marker));
    assert!(logs.contains(threadless_newer_marker));
    assert_eq!(logs.matches('\n').count(), 2);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn query_feedback_logs_for_threads_merges_requested_threads_and_threadless_rows() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .insert_logs(&[
            LogEntry {
                ts: 1,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("thread-1".to_string()),
                feedback_log_body: None,
                thread_id: Some("thread-1".to_string()),
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 2,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("thread-2".to_string()),
                feedback_log_body: None,
                thread_id: Some("thread-2".to_string()),
                process_uuid: Some("proc-2".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 3,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("threadless-proc-1".to_string()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-1".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 4,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("threadless-proc-2".to_string()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-2".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 5,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("thread-3".to_string()),
                feedback_log_body: None,
                thread_id: Some("thread-3".to_string()),
                process_uuid: Some("proc-3".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
            LogEntry {
                ts: 6,
                ts_nanos: 0,
                level: "INFO".to_string(),
                target: "cli".to_string(),
                message: Some("threadless-proc-3".to_string()),
                feedback_log_body: None,
                thread_id: None,
                process_uuid: Some("proc-3".to_string()),
                file: None,
                line: None,
                module_path: None,
            },
        ])
        .await
        .expect("insert test logs");

    let bytes = runtime
        .query_feedback_logs_for_threads(&["thread-1", "thread-2"])
        .await
        .expect("query feedback logs");

    assert_eq!(
        String::from_utf8(bytes).expect("valid utf-8"),
        [
            format_feedback_log_line(/*ts*/ 1, /*ts_nanos*/ 0, "INFO", "thread-1"),
            format_feedback_log_line(/*ts*/ 2, /*ts_nanos*/ 0, "INFO", "thread-2"),
            format_feedback_log_line(
                /*ts*/ 3,
                /*ts_nanos*/ 0,
                "INFO",
                "threadless-proc-1"
            ),
            format_feedback_log_line(
                /*ts*/ 4,
                /*ts_nanos*/ 0,
                "INFO",
                "threadless-proc-2"
            ),
        ]
        .concat()
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn query_feedback_logs_for_threads_returns_empty_for_empty_thread_list() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let bytes = runtime
        .query_feedback_logs_for_threads(&[])
        .await
        .expect("query feedback logs");

    assert_eq!(bytes, Vec::<u8>::new());

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}
