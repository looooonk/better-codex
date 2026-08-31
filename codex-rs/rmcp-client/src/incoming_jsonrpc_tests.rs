use pretty_assertions::assert_eq;
use rmcp::model::JsonRpcMessage;
use rmcp::model::ServerResult;
use serde_json::Value;
use serde_json::json;

use super::*;

fn input_required(meta: Value, input_requests: Value, request_state: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 7,
        "result": {
            "resultType": "input_required",
            "inputRequests": input_requests,
            "requestState": request_state,
            "_meta": meta,
        },
    })
}

#[test]
fn input_required_discriminator_wins_over_tool_result_metadata() {
    let message = input_required(
        json!({"trace": "round-one"}),
        json!({
            "confirm": {
                "method": "elicitation/create",
                "params": {
                    "mode": "form",
                    "message": "Continue?",
                    "requestedSchema": {"type": "object", "properties": {}},
                },
            },
        }),
        json!("opaque"),
    );

    let decoded = deserialize_incoming_jsonrpc_message(
        &serde_json::to_vec(&message).expect("serialize message"),
        /*modern_session*/ true,
    )
    .expect("decode modern input-required response");
    let JsonRpcMessage::Response(response) = decoded else {
        panic!("expected JSON-RPC response");
    };
    let ServerResult::InputRequiredResult(result) = response.result else {
        panic!("expected typed input-required result");
    };

    assert_eq!(
        result.meta.map(|meta| Value::Object(meta.0)),
        Some(json!({"trace": "round-one"}))
    );
    assert_eq!(result.request_state.as_deref(), Some("opaque"));
    assert_eq!(result.input_requests.map(|requests| requests.len()), Some(1));
}

#[test]
fn modern_input_required_fields_are_bounded() {
    for (field, message) in [
        (
            "_meta",
            input_required(
                json!({"value": "x".repeat(MAX_MCP_MRTR_FIELD_BYTES)}),
                json!({}),
                json!("state"),
            ),
        ),
        (
            "inputRequests",
            input_required(
                json!({}),
                json!({"value": "x".repeat(MAX_MCP_MRTR_FIELD_BYTES)}),
                json!("state"),
            ),
        ),
        (
            "requestState",
            input_required(
                json!({}),
                json!({}),
                json!("x".repeat(MAX_MCP_MRTR_FIELD_BYTES)),
            ),
        ),
    ] {
        let error = deserialize_incoming_jsonrpc_message(
            &serde_json::to_vec(&message).expect("serialize message"),
            /*modern_session*/ true,
        )
        .expect_err("oversized modern MRTR field must be rejected");
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn sse_normalization_keeps_nested_input_metadata() {
    let message = input_required(
        json!({"response": "private"}),
        json!({
            "confirm": {
                "method": "elicitation/create",
                "params": {
                    "mode": "form",
                    "message": "Continue?",
                    "requestedSchema": {"type": "object", "properties": {}},
                    "_meta": {"prompt": "visible"},
                },
            },
        }),
        json!("opaque"),
    );
    let normalized = normalize_sse_jsonrpc_message(
        &serde_json::to_string(&message).expect("serialize message"),
        /*modern_session*/ true,
    )
    .expect("normalize SSE message")
    .expect("response metadata requires normalization");
    let normalized: Value = serde_json::from_str(&normalized).expect("parse normalized message");

    assert_eq!(normalized.pointer("/result/_meta"), None);
    assert_eq!(
        normalized.pointer("/result/inputRequests/confirm/params/_meta"),
        Some(&json!({"prompt": "visible"}))
    );
}

#[test]
fn legacy_sse_payload_is_unchanged() {
    let payload = serde_json::to_string(&input_required(
        json!({"response": "legacy"}),
        json!({}),
        json!("state"),
    ))
    .expect("serialize message");

    assert_eq!(
        normalize_sse_jsonrpc_message(&payload, /*modern_session*/ false)
            .expect("legacy payload should not be parsed"),
        None
    );
}
