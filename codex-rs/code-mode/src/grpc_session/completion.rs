use codex_code_mode_protocol::encode_bounded_json;
use codex_code_mode_protocol::grpc;
use codex_code_mode_protocol::grpc::MAX_APPLICATION_MESSAGE_BYTES;
use codex_code_mode_protocol::grpc::MAX_TOOL_ERROR_BYTES;
use prost::Message;

const TRUNCATED_SUFFIX: &str = "... [truncated]";

pub(super) fn request(
    session_id: &str,
    invocation_id: &str,
    result: Result<serde_json::Value, String>,
) -> grpc::CompleteToolCallRequest {
    request_with_maximum(
        session_id,
        invocation_id,
        result,
        MAX_APPLICATION_MESSAGE_BYTES,
    )
}

fn request_with_maximum(
    session_id: &str,
    invocation_id: &str,
    result: Result<serde_json::Value, String>,
    maximum_message_bytes: usize,
) -> grpc::CompleteToolCallRequest {
    let outcome = match result {
        Ok(value) => match encode_bounded_json(&value, maximum_message_bytes) {
            Ok(output_json) => {
                grpc::complete_tool_call_request::Outcome::Succeeded(grpc::ToolCallSucceeded {
                    output_json,
                })
            }
            Err(error) => grpc::complete_tool_call_request::Outcome::Failed(grpc::ToolCallFailed {
                message: bounded_error(format!("failed to encode code-mode tool result: {error}")),
            }),
        },
        Err(message) => grpc::complete_tool_call_request::Outcome::Failed(grpc::ToolCallFailed {
            message: bounded_error(message),
        }),
    };
    let mut request = grpc::CompleteToolCallRequest {
        session_id: session_id.to_string(),
        invocation_id: invocation_id.to_string(),
        outcome: Some(outcome),
    };
    let encoded_bytes = request.encoded_len();
    if encoded_bytes > maximum_message_bytes {
        request.outcome = Some(grpc::complete_tool_call_request::Outcome::Failed(
            grpc::ToolCallFailed {
                message: bounded_error(format!(
                    "code-mode tool result of {encoded_bytes} encoded bytes exceeds the application limit of {maximum_message_bytes} bytes"
                )),
            },
        ));
    }
    request
}

fn bounded_error(mut message: String) -> String {
    if message.len() <= MAX_TOOL_ERROR_BYTES {
        return message;
    }
    let mut boundary = MAX_TOOL_ERROR_BYTES - TRUNCATED_SUFFIX.len();
    while !message.is_char_boundary(boundary) {
        boundary -= 1;
    }
    message.truncate(boundary);
    message.push_str(TRUNCATED_SUFFIX);
    message
}

#[cfg(test)]
#[path = "completion_tests.rs"]
mod tests;
