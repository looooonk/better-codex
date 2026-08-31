use std::io::SeekFrom;
use std::path::Path;

use chrono::DateTime;
use codex_app_server_protocol::ThreadHistoryChangeSet;
use codex_app_server_protocol::project_rollout_line;
use codex_protocol::RolloutId;
use codex_rollout::RolloutLine;
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;

use super::LocalThreadStore;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

const MAX_PROJECTION_BATCH_BYTES: u64 = 8 * 1024 * 1024;

pub(super) async fn materialize_to_sqlite(
    store: &LocalThreadStore,
    rollout_id: RolloutId,
    rollout_path: &Path,
) -> ThreadStoreResult<()> {
    let mut start_offset =
        super::thread_history::next_rollout_byte_offset(store, rollout_id).await?;
    let (mut lines, mut next_offset, mut has_more) =
        read_complete_rollout_lines(rollout_path, start_offset).await?;
    if lines.is_empty() && start_offset == next_offset {
        return Ok(());
    }
    let subagent_history_start_ordinal = codex_rollout::read_session_meta_line(rollout_path)
        .await
        .map_err(thread_store_io_error)?
        .meta
        .subagent_history_start_ordinal;

    loop {
        let projections = lines
            .iter()
            .map(|line| {
                let created_at_ms = DateTime::parse_from_rfc3339(line.timestamp.as_str())
                    .map(|timestamp| timestamp.timestamp_millis())
                    .map_err(thread_history_error)?;
                let changes = if line.ordinal.is_some_and(|ordinal| {
                    subagent_history_start_ordinal.is_some_and(|start| ordinal < start)
                }) {
                    ThreadHistoryChangeSet::default()
                } else {
                    project_rollout_line(line)
                };
                Ok((line.ordinal, created_at_ms, changes))
            })
            .collect::<ThreadStoreResult<Vec<_>>>()?;
        super::thread_history::apply_projection(
            store,
            rollout_id,
            start_offset,
            next_offset,
            projections,
        )
        .await?;
        if !has_more {
            return Ok(());
        }
        start_offset = next_offset;
        (lines, next_offset, has_more) =
            read_complete_rollout_lines(rollout_path, start_offset).await?;
        if lines.is_empty() && start_offset == next_offset {
            return Ok(());
        }
    }
}

async fn read_complete_rollout_lines(
    rollout_path: &Path,
    start_offset: u64,
) -> ThreadStoreResult<(Vec<RolloutLine>, u64, bool)> {
    let file_len = match tokio::fs::metadata(rollout_path).await {
        Ok(metadata) => metadata.len(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound && start_offset == 0 => {
            return Ok((Vec::new(), 0, false));
        }
        Err(err) => return Err(thread_store_io_error(err)),
    };
    let remaining = file_len
        .checked_sub(start_offset)
        .ok_or_else(|| ThreadStoreError::Internal {
            message: "durable rollout shrank before projection".to_string(),
        })?;
    let byte_count = usize::try_from(remaining.min(MAX_PROJECTION_BATCH_BYTES)).map_err(|_| {
        ThreadStoreError::Internal {
            message: "projection batch exceeds addressable memory".to_string(),
        }
    })?;
    let mut bytes = vec![0; byte_count];
    let mut file = tokio::fs::File::open(rollout_path)
        .await
        .map_err(thread_store_io_error)?;
    file.seek(SeekFrom::Start(start_offset))
        .await
        .map_err(thread_store_io_error)?;
    file.read_exact(bytes.as_mut_slice())
        .await
        .map_err(thread_store_io_error)?;
    let complete_byte_count = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    if complete_byte_count == 0 && remaining > MAX_PROJECTION_BATCH_BYTES {
        return Err(ThreadStoreError::Internal {
            message: format!(
                "rollout record exceeds {MAX_PROJECTION_BATCH_BYTES} byte projection limit: {}",
                rollout_path.display()
            ),
        });
    }
    let next_offset = start_offset
        .checked_add(u64::try_from(complete_byte_count).map_err(|_| {
            ThreadStoreError::Internal {
                message: "durable rollout append exceeds addressable memory".to_string(),
            }
        })?)
        .ok_or_else(|| ThreadStoreError::Internal {
            message: "durable rollout byte offset overflow".to_string(),
        })?;
    let text = std::str::from_utf8(&bytes[..complete_byte_count]).map_err(|err| {
        ThreadStoreError::Internal {
            message: format!(
                "rollout projection contains invalid UTF-8 at {}: {err}",
                rollout_path.display()
            ),
        }
    })?;
    let mut lines = Vec::new();
    for line in text.lines() {
        let parsed = serde_json::from_str::<Value>(line).and_then(|mut value| {
            codex_rollout::redact_persisted_json(&mut value);
            serde_json::from_value::<RolloutLine>(value)
        })
        .map_err(|err| ThreadStoreError::Internal {
            message: format!(
                "rollout projection contains invalid JSON at {}: {err}",
                rollout_path.display()
            ),
        })?;
        lines.push(parsed);
    }
    Ok((
        lines,
        next_offset,
        remaining > MAX_PROJECTION_BATCH_BYTES,
    ))
}

fn thread_history_error(err: impl std::fmt::Display) -> ThreadStoreError {
    ThreadStoreError::Internal {
        message: format!("failed to project thread history: {err}"),
    }
}

fn thread_store_io_error(err: std::io::Error) -> ThreadStoreError {
    ThreadStoreError::Internal {
        message: err.to_string(),
    }
}

#[cfg(test)]
#[path = "thread_history_materialization_tests.rs"]
mod tests;
