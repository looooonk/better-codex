use std::fs;
use std::sync::Arc;

use codex_code_mode::CellId;
use codex_code_mode::FunctionCallOutputContentItem;
use codex_code_mode::RuntimeResponse;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use super::CodeCellTraceContext;
use super::MAX_CODE_CELL_RESPONSE_BYTES;
use super::MAX_CODE_CELL_SOURCE_BYTES;
use super::TRUNCATED_MARKER;
use crate::bundle::PAYLOADS_DIR_NAME;
use crate::bundle::RAW_EVENT_LOG_FILE_NAME;
use crate::writer::TraceWriter;

#[test]
fn started_source_is_utf8_truncated_to_the_trace_limit() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let context = trace_context(&temp)?;
    let source = "é".repeat(MAX_CODE_CELL_SOURCE_BYTES / 2 + 10);

    context.record_started("call-1", &source);

    let events = read_events(&temp)?;
    let persisted_source = events[0]["payload"]["source_js"]
        .as_str()
        .expect("source should be a string");
    assert!(persisted_source.len() <= MAX_CODE_CELL_SOURCE_BYTES);
    assert!(persisted_source.ends_with(TRUNCATED_MARKER));
    Ok(())
}

#[test]
fn terminal_initial_and_ended_events_reuse_one_payload() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let context = trace_context(&temp)?;
    let response = result_response("complete");

    context.record_initial_response(&response);
    context.record_ended(&response);

    let events = read_events(&temp)?;
    assert_eq!(
        events[0]["payload"]["response_payload"],
        events[1]["payload"]["response_payload"],
    );
    assert_eq!(payload_file_count(&temp)?, 1);
    Ok(())
}

#[test]
fn oversized_response_uses_one_bounded_truncation_payload() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let context = trace_context(&temp)?;
    let response = result_response(&"x".repeat(MAX_CODE_CELL_RESPONSE_BYTES + 1));

    context.record_initial_response(&response);
    context.record_ended(&response);

    let events = read_events(&temp)?;
    assert_eq!(
        events[0]["payload"]["response_payload"],
        events[1]["payload"]["response_payload"],
    );
    assert_eq!(payload_file_count(&temp)?, 1);
    let payload_path = temp.path().join(PAYLOADS_DIR_NAME).join("1.json");
    assert!(fs::metadata(&payload_path)?.len() <= MAX_CODE_CELL_RESPONSE_BYTES as u64);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(payload_path)?)?,
        json!({
            "cell_id": "cell-1",
            "response_type": "result",
            "truncated": true,
        }),
    );
    Ok(())
}

fn trace_context(temp: &TempDir) -> anyhow::Result<CodeCellTraceContext> {
    let writer = Arc::new(TraceWriter::create(
        temp.path(),
        "trace-1".to_string(),
        "rollout-1".to_string(),
        "thread-1".to_string(),
    )?);
    Ok(CodeCellTraceContext::enabled(
        writer,
        "thread-1",
        "turn-1",
        "cell-1",
    ))
}

fn result_response(text: &str) -> RuntimeResponse {
    RuntimeResponse::Result {
        cell_id: CellId::new("cell-1".to_string()),
        content_items: vec![FunctionCallOutputContentItem::InputText {
            text: text.to_string(),
        }],
        error_text: None,
    }
}

fn read_events(temp: &TempDir) -> anyhow::Result<Vec<Value>> {
    fs::read_to_string(temp.path().join(RAW_EVENT_LOG_FILE_NAME))?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

fn payload_file_count(temp: &TempDir) -> anyhow::Result<usize> {
    Ok(fs::read_dir(temp.path().join(PAYLOADS_DIR_NAME))?.count())
}
