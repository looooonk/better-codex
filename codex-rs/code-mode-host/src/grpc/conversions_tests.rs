use codex_code_mode_protocol::CellId;
use codex_code_mode_protocol::FunctionCallOutputContentItem;
use codex_code_mode_protocol::ImageDetail;
use codex_code_mode_protocol::RuntimeResponse;
use codex_code_mode_protocol::WaitOutcome;
use codex_code_mode_protocol::grpc as proto;
use codex_code_mode_protocol::grpc::MAX_APPLICATION_MESSAGE_BYTES;
use pretty_assertions::assert_eq;
use tonic::Code;

use super::execute_request;
use super::execution_outcome;
use crate::grpc::validation::MAX_TOOL_DEFINITIONS;
use crate::grpc::validation::MAX_TOOL_DESCRIPTION_BYTES;

#[test]
fn rejects_missing_names_unknown_tool_kinds_and_invalid_json_schemas() {
    let definition = proto::ToolDefinition {
        name: "echo".to_string(),
        tool_name: None,
        description: String::new(),
        kind: proto::ToolKind::Function as i32,
        input_schema_json: None,
        output_schema_json: None,
    };
    let request = |definition| proto::ExecuteRequest {
        session_id: "session".to_string(),
        execution_id: "execution".to_string(),
        tool_call_id: "call".to_string(),
        source: String::new(),
        enabled_tools: vec![definition],
        yield_time_ms: None,
        max_output_tokens: None,
    };

    assert_eq!(
        execute_request(request(definition.clone()))
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    let definition = proto::ToolDefinition {
        tool_name: Some(proto::ToolName {
            name: "echo".to_string(),
            namespace: Some("tools".to_string()),
        }),
        ..definition
    };
    assert_eq!(
        execute_request(request(proto::ToolDefinition {
            kind: proto::ToolKind::Unspecified as i32,
            ..definition.clone()
        }))
        .unwrap_err()
        .code(),
        Code::InvalidArgument
    );
    assert_eq!(
        execute_request(request(proto::ToolDefinition {
            input_schema_json: Some(b"not-json".to_vec()),
            ..definition
        }))
        .unwrap_err()
        .code(),
        Code::InvalidArgument
    );
}

#[test]
fn maps_text_image_and_terminal_error_without_losing_details() {
    let outcome = execution_outcome(RuntimeResponse::Result {
        cell_id: CellId::new("cell".to_string()),
        content_items: vec![
            FunctionCallOutputContentItem::InputText {
                text: "hello".to_string(),
            },
            FunctionCallOutputContentItem::InputImage {
                image_url: "data:image/png;base64,YQ==".to_string(),
                detail: Some(ImageDetail::Original),
            },
        ],
        error_text: Some("failed".to_string()),
    })
    .expect("bounded execution outcome");

    assert_eq!(
        outcome,
        proto::ExecutionOutcome {
            cell_id: "cell".to_string(),
            content_items: vec![
                proto::ContentItem {
                    item: Some(proto::content_item::Item::Text(proto::TextContent {
                        text: "hello".to_string(),
                    })),
                },
                proto::ContentItem {
                    item: Some(proto::content_item::Item::Image(proto::ImageContent {
                        image_url: "data:image/png;base64,YQ==".to_string(),
                        detail: Some(proto::ImageDetail::Original as i32),
                    })),
                },
            ],
            outcome: Some(proto::execution_outcome::Outcome::Completed(
                proto::ExecutionCompleted {
                    error_text: Some("failed".to_string()),
                },
            )),
        }
    );
}

#[test]
fn rejects_oversized_text_and_image_outcomes_for_streams_and_unary_responses() {
    let response = |item| RuntimeResponse::Result {
        cell_id: CellId::new("cell".to_string()),
        content_items: vec![item],
        error_text: None,
    };

    let text = "x".repeat(MAX_APPLICATION_MESSAGE_BYTES);
    assert_eq!(
        super::execute_event(response(FunctionCallOutputContentItem::InputText { text }))
            .expect_err("oversized execution event")
            .code(),
        Code::ResourceExhausted
    );

    let image_url = format!(
        "data:image/png;base64,{}",
        "A".repeat(MAX_APPLICATION_MESSAGE_BYTES)
    );
    assert_eq!(
        super::wait_response(WaitOutcome::LiveCell(response(
            FunctionCallOutputContentItem::InputImage {
                image_url,
                detail: Some(ImageDetail::Low),
            },
        )))
        .expect_err("oversized wait response")
        .code(),
        Code::ResourceExhausted
    );
}

#[test]
fn enforces_tool_count_and_description_bounds() {
    let definition = proto::ToolDefinition {
        name: "echo".to_string(),
        tool_name: Some(proto::ToolName {
            name: "echo".to_string(),
            namespace: None,
        }),
        description: "x".repeat(MAX_TOOL_DESCRIPTION_BYTES),
        kind: proto::ToolKind::Function as i32,
        input_schema_json: None,
        output_schema_json: None,
    };
    let request = |enabled_tools| proto::ExecuteRequest {
        session_id: "session".to_string(),
        execution_id: "execution".to_string(),
        tool_call_id: "call".to_string(),
        source: String::new(),
        enabled_tools,
        yield_time_ms: None,
        max_output_tokens: None,
    };

    let count_definition = proto::ToolDefinition {
        description: String::new(),
        ..definition.clone()
    };
    assert!(
        execute_request(request(vec![
            count_definition.clone();
            MAX_TOOL_DEFINITIONS
        ]))
        .is_ok()
    );
    assert_eq!(
        execute_request(request(vec![count_definition; MAX_TOOL_DEFINITIONS + 1]))
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert!(execute_request(request(vec![definition.clone()])).is_ok());
    assert_eq!(
        execute_request(request(vec![proto::ToolDefinition {
            description: "x".repeat(MAX_TOOL_DESCRIPTION_BYTES + 1),
            ..definition
        }]))
        .unwrap_err()
        .code(),
        Code::InvalidArgument
    );
}
