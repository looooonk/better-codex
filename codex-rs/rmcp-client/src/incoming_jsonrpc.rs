//! Compatibility decoding and bounds for modern multi-round server results.

use rmcp::model::InputRequiredResult;
use rmcp::model::JsonRpcResponse;
use rmcp::model::ServerJsonRpcMessage;
use rmcp::model::ServerResult;
use serde::de::Error as _;
use serde_json::Value;

const MAX_MCP_MRTR_FIELD_BYTES: usize = 4 * 1024;

pub(crate) fn deserialize_incoming_jsonrpc_message(
    bytes: &[u8],
    modern_session: bool,
) -> serde_json::Result<ServerJsonRpcMessage> {
    if !modern_session {
        return serde_json::from_slice(bytes);
    }

    let message: Value = serde_json::from_slice(bytes)?;
    validate_input_required_fields(&message)?;
    if message
        .pointer("/result/resultType")
        .and_then(Value::as_str)
        == Some("input_required")
    {
        let response: JsonRpcResponse<InputRequiredResult> = serde_json::from_value(message)?;
        return Ok(ServerJsonRpcMessage::response(
            ServerResult::InputRequiredResult(response.result),
            response.id,
        ));
    }

    serde_json::from_value(message)
}

pub(crate) fn normalize_sse_jsonrpc_message(
    payload: &str,
    modern_session: bool,
) -> serde_json::Result<Option<String>> {
    if !modern_session {
        return Ok(None);
    }

    let mut message: Value = serde_json::from_str(payload)?;
    validate_input_required_fields(&message)?;
    if message
        .pointer("/result/resultType")
        .and_then(Value::as_str)
        != Some("input_required")
    {
        return Ok(None);
    }

    let changed = message
        .get_mut("result")
        .and_then(Value::as_object_mut)
        .is_some_and(|result| {
            if !result.contains_key("_meta") {
                return false;
            }
            // rmcp tries CallToolResult before InputRequiredResult, and `_meta`
            // alone makes the former match. This invalid CallToolResult field
            // makes serde continue to the typed input-required variant.
            result.insert("content".to_string(), Value::Bool(false));
            true
        });
    changed
        .then(|| serde_json::to_string(&message))
        .transpose()
}

fn validate_input_required_fields(message: &Value) -> serde_json::Result<()> {
    if message
        .pointer("/result/resultType")
        .and_then(Value::as_str)
        != Some("input_required")
    {
        return Ok(());
    }

    for (name, pointer) in [
        ("_meta", "/result/_meta"),
        ("inputRequests", "/result/inputRequests"),
        ("requestState", "/result/requestState"),
    ] {
        let Some(value) = message.pointer(pointer) else {
            continue;
        };
        let size = serde_json::to_vec(value)?.len();
        if size > MAX_MCP_MRTR_FIELD_BYTES {
            return Err(serde_json::Error::custom(format!(
                "MCP input_required {name} exceeds {MAX_MCP_MRTR_FIELD_BYTES} bytes"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "incoming_jsonrpc_tests.rs"]
mod tests;
