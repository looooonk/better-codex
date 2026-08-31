use super::*;
use crate::RawPayloadKind;
use crate::RawTraceEventPayload;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

const SECRET: &str = "example_synthetic_bearer_token_123456";

#[test]
fn writer_redacts_payloads_and_events_without_changing_correlation_or_ciphertext()
-> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let writer = TraceWriter::create(
        temp.path(),
        "trace-1".to_string(),
        "rollout-1".to_string(),
        "thread-1".to_string(),
    )?;
    let payload = writer.write_json_payload(
        RawPayloadKind::ToolRuntimeEvent,
        &json!({
            "call_id": "call-1",
            "command": ["curl", "-H", format!("Authorization: Bearer {SECRET}")],
            "parsed_cmd": [{"type": "unknown", "cmd": format!("curl -H 'Bearer {SECRET}'")}],
            "arguments": format!(r#"{{"token":"{SECRET}"}}"#),
            "content": [{
                "type": "encrypted_content",
                "encrypted_content": "gAAAAABopaque-ciphertext",
            }],
        }),
    )?;
    writer.append(RawTraceEventPayload::Other {
        kind: "diagnostic".to_string(),
        summary: format!("Authorization: Bearer {SECRET}"),
        payloads: vec![payload.clone()],
        metadata: json!({"token": SECRET}),
    })?;

    let persisted: Value =
        serde_json::from_slice(&std::fs::read(temp.path().join(&payload.path))?)?;
    assert_eq!(persisted["call_id"], "call-1");
    assert_eq!(
        persisted["content"][0]["encrypted_content"],
        "gAAAAABopaque-ciphertext"
    );
    assert!(!persisted["command"].to_string().contains(SECRET));
    assert!(!persisted["parsed_cmd"].to_string().contains(SECRET));
    assert!(!persisted["arguments"].to_string().contains(SECRET));

    let event_log = std::fs::read_to_string(temp.path().join("trace.jsonl"))?;
    assert!(!event_log.contains(SECRET));
    assert!(event_log.contains("[REDACTED_SECRET]"));
    Ok(())
}
