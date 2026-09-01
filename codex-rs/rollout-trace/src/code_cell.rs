//! Hot-path helpers for recording code-mode runtime cell lifecycles.
//!
//! The public `exec` tool is reduced as a first-class `CodeCell` instead of a
//! generic tool call. This module keeps the runtime response serialization and
//! lifecycle event policy inside the trace crate while core carries a compact,
//! no-op capable handle through execution and waits.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use codex_code_mode::CellId;
use codex_code_mode::RuntimeResponse;
use serde::Serialize;
use tracing::warn;

use crate::model::AgentThreadId;
use crate::model::CodeCellRuntimeStatus;
use crate::model::CodexTurnId;
use crate::model::ModelVisibleCallId;
use crate::payload::RawPayloadKind;
use crate::payload::RawPayloadRef;
use crate::raw_event::RawTraceEventContext;
use crate::raw_event::RawTraceEventPayload;
use crate::writer::TraceWriter;

const MAX_CODE_CELL_SOURCE_BYTES: usize = 64 * 1_024;
const MAX_CODE_CELL_RESPONSE_BYTES: usize = 1_024 * 1_024;
const TRUNCATED_MARKER: &str = "\n[truncated]";

/// No-op capable trace handle for one code-mode runtime cell.
#[derive(Clone, Debug)]
pub struct CodeCellTraceContext {
    state: CodeCellTraceContextState,
}

#[derive(Clone, Debug)]
enum CodeCellTraceContextState {
    Disabled,
    Enabled(EnabledCodeCellTraceContext),
}

#[derive(Clone, Debug)]
struct EnabledCodeCellTraceContext {
    writer: Arc<TraceWriter>,
    thread_id: AgentThreadId,
    codex_turn_id: CodexTurnId,
    runtime_cell_id: String,
    terminal_response_payload: Arc<Mutex<Option<Option<RawPayloadRef>>>>,
}

/// Raw code-mode response captured at the runtime boundary.
///
/// This is not the model-visible custom-tool output. The reducer links that
/// output through `CodeCell.output_item_ids` once the conversation item appears.
/// Keeping the raw runtime payload here preserves stored-value and lifecycle
/// evidence without duplicating the model-facing transcript.
#[derive(Serialize)]
struct CodeCellResponseTracePayload<'a> {
    response: &'a RuntimeResponse,
}

#[derive(Serialize)]
struct TruncatedCodeCellResponseTracePayload<'a> {
    cell_id: &'a str,
    response_type: &'static str,
    truncated: bool,
}

impl CodeCellTraceContext {
    /// Builds a context that accepts trace calls and records nothing.
    pub(crate) fn disabled() -> Self {
        Self {
            state: CodeCellTraceContextState::Disabled,
        }
    }

    /// Builds a context for an already-known code-mode runtime cell.
    pub(crate) fn enabled(
        writer: Arc<TraceWriter>,
        thread_id: impl Into<AgentThreadId>,
        codex_turn_id: impl Into<CodexTurnId>,
        runtime_cell_id: impl Into<String>,
    ) -> Self {
        Self {
            state: CodeCellTraceContextState::Enabled(EnabledCodeCellTraceContext {
                writer,
                thread_id: thread_id.into(),
                codex_turn_id: codex_turn_id.into(),
                runtime_cell_id: runtime_cell_id.into(),
                terminal_response_payload: Arc::new(Mutex::new(None)),
            }),
        }
    }

    /// Records the parent runtime object before JavaScript can issue nested tool calls.
    pub fn record_started(
        &self,
        model_visible_call_id: impl Into<ModelVisibleCallId>,
        source_js: impl AsRef<str>,
    ) {
        let CodeCellTraceContextState::Enabled(context) = &self.state else {
            return;
        };
        append_with_context_best_effort(
            context,
            RawTraceEventPayload::CodeCellStarted {
                runtime_cell_id: context.runtime_cell_id.clone(),
                model_visible_call_id: model_visible_call_id.into(),
                source_js: truncate_utf8(source_js.as_ref(), MAX_CODE_CELL_SOURCE_BYTES),
            },
        );
    }

    /// Records the first response returned by the public code-mode `exec` tool.
    ///
    /// A yielded response returns control to the model while the cell keeps
    /// running. Terminal initial responses should be followed by `record_ended`
    /// by the caller so the reducer can distinguish model-visible output from
    /// runtime completion.
    pub fn record_initial_response(&self, response: &RuntimeResponse) {
        let CodeCellTraceContextState::Enabled(context) = &self.state else {
            return;
        };
        let response_payload = if is_terminal_runtime_response(response) {
            terminal_response_payload(context, response)
        } else {
            code_cell_response_payload(context, response)
        };
        append_with_context_best_effort(
            context,
            RawTraceEventPayload::CodeCellInitialResponse {
                runtime_cell_id: context.runtime_cell_id.clone(),
                status: code_cell_status_for_runtime_response(response),
                response_payload,
            },
        );
    }

    /// Records the terminal lifecycle point for a code-mode runtime cell.
    pub fn record_ended(&self, response: &RuntimeResponse) {
        let CodeCellTraceContextState::Enabled(context) = &self.state else {
            return;
        };
        let response_payload = terminal_response_payload(context, response);
        append_with_context_best_effort(
            context,
            RawTraceEventPayload::CodeCellEnded {
                runtime_cell_id: context.runtime_cell_id.clone(),
                status: code_cell_status_for_runtime_response(response),
                response_payload,
            },
        );
    }
}

fn is_terminal_runtime_response(response: &RuntimeResponse) -> bool {
    !matches!(response, RuntimeResponse::Yielded { .. })
}

fn terminal_response_payload(
    context: &EnabledCodeCellTraceContext,
    response: &RuntimeResponse,
) -> Option<RawPayloadRef> {
    let mut cached = context
        .terminal_response_payload
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    cached.clone().unwrap_or_else(|| {
        let payload = code_cell_response_payload(context, response);
        *cached = Some(payload.clone());
        payload
    })
}

fn code_cell_status_for_runtime_response(response: &RuntimeResponse) -> CodeCellRuntimeStatus {
    match response {
        RuntimeResponse::Yielded { .. } => CodeCellRuntimeStatus::Yielded,
        RuntimeResponse::Terminated { .. } => CodeCellRuntimeStatus::Terminated,
        RuntimeResponse::Result { error_text, .. } => {
            if error_text.is_some() {
                CodeCellRuntimeStatus::Failed
            } else {
                CodeCellRuntimeStatus::Completed
            }
        }
    }
}

fn code_cell_response_payload(
    context: &EnabledCodeCellTraceContext,
    response: &RuntimeResponse,
) -> Option<RawPayloadRef> {
    let payload = CodeCellResponseTracePayload { response };
    match context.writer.write_json_payload_bounded(
        RawPayloadKind::ToolResult,
        &payload,
        MAX_CODE_CELL_RESPONSE_BYTES,
    ) {
        Ok(payload_ref) => Some(payload_ref),
        Err(err) => {
            warn!("code cell rollout trace payload was omitted: {err:#}");
            let payload = TruncatedCodeCellResponseTracePayload {
                cell_id: runtime_response_cell_id(response).as_str(),
                response_type: runtime_response_type(response),
                truncated: true,
            };
            write_json_payload_best_effort(&context.writer, RawPayloadKind::ToolResult, &payload)
        }
    }
}

fn runtime_response_cell_id(response: &RuntimeResponse) -> &CellId {
    match response {
        RuntimeResponse::Yielded { cell_id, .. }
        | RuntimeResponse::Terminated { cell_id, .. }
        | RuntimeResponse::Result { cell_id, .. } => cell_id,
    }
}

fn runtime_response_type(response: &RuntimeResponse) -> &'static str {
    match response {
        RuntimeResponse::Yielded { .. } => "yielded",
        RuntimeResponse::Terminated { .. } => "terminated",
        RuntimeResponse::Result { .. } => "result",
    }
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_string();
    }
    let maximum_content_bytes = maximum_bytes.saturating_sub(TRUNCATED_MARKER.len());
    let mut end = maximum_content_bytes.min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = String::with_capacity(end + TRUNCATED_MARKER.len());
    truncated.push_str(&value[..end]);
    truncated.push_str(TRUNCATED_MARKER);
    truncated
}

fn write_json_payload_best_effort(
    writer: &TraceWriter,
    kind: RawPayloadKind,
    payload: &impl Serialize,
) -> Option<RawPayloadRef> {
    match writer.write_json_payload(kind, payload) {
        Ok(payload_ref) => Some(payload_ref),
        Err(err) => {
            warn!("failed to write rollout trace payload: {err:#}");
            None
        }
    }
}

fn append_with_context_best_effort(
    context: &EnabledCodeCellTraceContext,
    payload: RawTraceEventPayload,
) {
    let event_context = RawTraceEventContext {
        thread_id: Some(context.thread_id.clone()),
        codex_turn_id: Some(context.codex_turn_id.clone()),
    };
    if let Err(err) = context.writer.append_with_context(event_context, payload) {
        warn!("failed to append rollout trace event: {err:#}");
    }
}

#[cfg(test)]
#[path = "code_cell_trace_tests.rs"]
mod tests;
