use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use pretty_assertions::assert_eq;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::writer::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use super::*;

fn temp_codex_home() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("codex-state-log-db-{}", Uuid::new_v4()))
}

async fn wait_for_log_count(runtime: &StateRuntime, expected: usize) -> Vec<crate::LogRow> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let rows = runtime
            .query_logs(&crate::LogQuery::default())
            .await
            .expect("query logs");
        if rows.len() == expected {
            return rows;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {expected} logs; saw {}",
            rows.len()
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

fn test_entry(message: &str) -> LogEntry {
    LogEntry {
        ts: 1,
        ts_nanos: 2,
        level: "INFO".to_string(),
        target: "test".to_string(),
        message: Some(message.to_string()),
        feedback_log_body: Some(message.to_string()),
        thread_id: Some("thread-1".to_string()),
        process_uuid: Some("process-1".to_string()),
        module_path: Some("module".to_string()),
        file: Some("file.rs".to_string()),
        line: Some(7),
    }
}

#[derive(Clone, Default)]
struct SharedWriter {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl SharedWriter {
    fn snapshot(&self) -> String {
        String::from_utf8(self.bytes.lock().expect("writer mutex poisoned").clone())
            .expect("valid utf-8")
    }
}

struct SharedWriterGuard {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl<'a> MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriterGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriterGuard {
            bytes: Arc::clone(&self.bytes),
        }
    }
}

impl io::Write for SharedWriterGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes
            .lock()
            .expect("writer mutex poisoned")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn sqlite_feedback_logs_match_feedback_formatter_shape() {
    let codex_home = temp_codex_home();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");
    let writer = SharedWriter::default();
    let layer = start(runtime.clone());

    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(writer.clone())
                .with_ansi(false)
                .with_target(false)
                .with_filter(Targets::new().with_default(tracing::Level::TRACE)),
        )
        .with(
            layer
                .clone()
                .with_filter(Targets::new().with_default(tracing::Level::TRACE)),
        );
    let guard = subscriber.set_default();

    tracing::trace!("threadless-before");
    tracing::info_span!("feedback-thread", thread_id = "thread-1", turn = 1).in_scope(|| {
        tracing::info!(foo = 2, "thread-scoped");
    });
    tracing::debug!("threadless-after");

    layer.flush().await;
    drop(guard);

    let feedback_logs = writer.snapshot();
    let without_timestamps = |logs: &str| {
        logs.lines()
            .map(|line| match line.split_once(' ') {
                Some((_, rest)) => rest,
                None => line,
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let sqlite_logs = String::from_utf8(
        runtime
            .query_feedback_logs("thread-1")
            .await
            .expect("query feedback logs"),
    )
    .expect("valid utf-8");
    assert_eq!(
        without_timestamps(&sqlite_logs),
        without_timestamps(&feedback_logs)
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn flush_persists_logs_for_query() {
    let codex_home = temp_codex_home();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");
    let layer = start(runtime.clone());

    let guard = tracing_subscriber::registry()
        .with(
            layer
                .clone()
                .with_filter(Targets::new().with_default(tracing::Level::TRACE)),
        )
        .set_default();

    tracing::info!("buffered-log");

    layer.flush().await;
    drop(guard);

    let after_flush = runtime
        .query_logs(&crate::LogQuery::default())
        .await
        .expect("query logs after flush");
    assert_eq!(after_flush.len(), 1);
    assert_eq!(after_flush[0].message.as_deref(), Some("buffered-log"));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn configured_batch_size_flushes_without_explicit_flush() {
    let codex_home = temp_codex_home();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");
    let layer = LogDbLayer::start_with_config(
        runtime.clone(),
        LogSinkQueueConfig {
            queue_capacity: 8,
            batch_size: 2,
            flush_interval: std::time::Duration::from_secs(60),
        },
    );

    let guard = tracing_subscriber::registry()
        .with(
            layer
                .clone()
                .with_filter(Targets::new().with_default(tracing::Level::TRACE)),
        )
        .set_default();

    tracing::info!("first-batch-log");
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    assert_eq!(
        runtime
            .query_logs(&crate::LogQuery::default())
            .await
            .expect("query logs before batch fills")
            .len(),
        0
    );

    tracing::info!("second-batch-log");
    let after_batch = wait_for_log_count(&runtime, /*expected*/ 2).await;
    drop(guard);

    assert_eq!(
        after_batch
            .iter()
            .map(|row| row.message.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("first-batch-log"), Some("second-batch-log")]
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn configured_flush_interval_persists_buffered_logs() {
    let codex_home = temp_codex_home();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");
    let layer = LogDbLayer::start_with_config(
        runtime.clone(),
        LogSinkQueueConfig {
            queue_capacity: 8,
            batch_size: 128,
            flush_interval: std::time::Duration::from_millis(10),
        },
    );
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let guard = tracing_subscriber::registry()
        .with(
            layer
                .clone()
                .with_filter(Targets::new().with_default(tracing::Level::TRACE)),
        )
        .set_default();

    tracing::info!("interval-log");
    let after_interval = wait_for_log_count(&runtime, /*expected*/ 1).await;
    drop(guard);

    assert_eq!(after_interval[0].message.as_deref(), Some("interval-log"));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn event_queue_drops_new_entries_when_full() {
    let (sender, mut receiver) = mpsc::channel(1);
    let layer = LogDbLayer {
        sender,
        process_uuid: "process-1".to_string(),
    };

    layer.try_send(test_entry("first-queued-log"));
    layer.try_send(test_entry("dropped-log"));

    match receiver.try_recv().expect("first entry queued") {
        LogDbCommand::Entry(entry) => {
            assert_eq!(entry.message.as_deref(), Some("first-queued-log"));
        }
        LogDbCommand::Flush(_) => panic!("expected queued entry"),
    }
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn flush_waits_for_queue_capacity_and_receiver_processing() {
    let (sender, mut receiver) = mpsc::channel(1);
    let layer = LogDbLayer {
        sender,
        process_uuid: "process-1".to_string(),
    };

    layer.try_send(test_entry("queued-before-flush"));
    let mut flush_task = tokio::spawn({
        let layer = layer.clone();
        async move {
            layer.flush().await;
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    assert!(!flush_task.is_finished());

    match receiver.recv().await.expect("queued entry") {
        LogDbCommand::Entry(entry) => {
            assert_eq!(entry.message.as_deref(), Some("queued-before-flush"));
        }
        LogDbCommand::Flush(_) => panic!("expected queued entry"),
    }

    match receiver.recv().await.expect("flush command") {
        LogDbCommand::Flush(reply) => {
            assert!(!flush_task.is_finished());
            let _ = reply.send(());
        }
        LogDbCommand::Entry(_) => panic!("expected flush command"),
    }

    tokio::time::timeout(std::time::Duration::from_secs(1), &mut flush_task)
        .await
        .expect("flush task completes")
        .expect("flush task succeeds");
}
