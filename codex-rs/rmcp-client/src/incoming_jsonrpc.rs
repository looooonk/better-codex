//! Compatibility decoding and bounds for modern multi-round server results.

use std::sync::Arc;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

use rmcp::model::DiscoverResult;
use rmcp::model::InputRequiredResult;
use rmcp::model::InitializeResult;
use rmcp::model::JsonRpcResponse;
use rmcp::model::ProtocolVersion;
use rmcp::model::ServerJsonRpcMessage;
use rmcp::model::ServerResult;
use serde::de::Error as _;
use serde_json::Value;

use crate::protocol_mode::McpProtocolMode;

const MAX_MCP_MRTR_FIELD_BYTES: usize = 4 * 1024;
const STDIO_PROTOCOL_UNKNOWN: u8 = 0;
const STDIO_PROTOCOL_LEGACY: u8 = 1;
const STDIO_PROTOCOL_MODERN: u8 = 2;

#[derive(Clone, Debug)]
pub(crate) struct StdioProtocolState {
    requested_modern: bool,
    negotiated: Arc<AtomicU8>,
}

impl StdioProtocolState {
    pub(crate) fn new(protocol_mode: McpProtocolMode) -> Self {
        Self {
            requested_modern: protocol_mode == McpProtocolMode::V20260728,
            negotiated: Arc::new(AtomicU8::new(STDIO_PROTOCOL_UNKNOWN)),
        }
    }

    pub(crate) fn deserialize(
        &self,
        bytes: &[u8],
    ) -> serde_json::Result<ServerJsonRpcMessage> {
        self.observe_negotiated_protocol(bytes);
        deserialize_incoming_jsonrpc_message(bytes, self.modern_session())
    }

    pub(crate) fn enforce_modern_bounds(&self) -> bool {
        self.requested_modern
            && self.negotiated.load(Ordering::Acquire) != STDIO_PROTOCOL_LEGACY
    }

    fn modern_session(&self) -> bool {
        self.requested_modern
            && self.negotiated.load(Ordering::Acquire) == STDIO_PROTOCOL_MODERN
    }

    fn observe_negotiated_protocol(&self, bytes: &[u8]) {
        if !self.requested_modern
            || self.negotiated.load(Ordering::Acquire) != STDIO_PROTOCOL_UNKNOWN
        {
            return;
        }

        let negotiated = serde_json::from_slice::<JsonRpcResponse<DiscoverResult>>(bytes)
            .ok()
            .map(|response| {
                response
                    .result
                    .supported_versions
                    .contains(&ProtocolVersion::V_2026_07_28)
            })
            .or_else(|| {
                serde_json::from_slice::<JsonRpcResponse<InitializeResult>>(bytes)
                    .ok()
                    .map(|response| {
                        response.result.protocol_version == ProtocolVersion::V_2026_07_28
                    })
            });
        if let Some(negotiated) = negotiated {
            self.negotiated.store(
                if negotiated {
                    STDIO_PROTOCOL_MODERN
                } else {
                    STDIO_PROTOCOL_LEGACY
                },
                Ordering::Release,
            );
        }
    }
}

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
