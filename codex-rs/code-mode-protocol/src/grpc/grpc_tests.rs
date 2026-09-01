use std::fmt::Debug;

use pretty_assertions::assert_eq;
use prost::Message;

use super::CellClosed;
use super::ContentItem;
use super::ImageContent;
use super::ImageDetail;
use super::OpenSessionRequest;
use super::SessionCellExecutionLimits;
use super::SessionEvent;
use super::TextContent;
use super::ToolCall;
use super::ToolKind;
use super::ToolName;
use super::content_item;
use super::session_event;

fn assert_wire_fixture<M>(message: M, expected: &[u8])
where
    M: Debug + Default + Message + PartialEq,
{
    assert_eq!(message.encode_to_vec(), expected.to_vec());
    assert_eq!(
        M::decode(expected).expect("wire fixture should decode"),
        message
    );
}

#[test]
fn open_session_limits_preserve_optional_presence_on_wire() {
    assert_wire_fixture(
        OpenSessionRequest {
            cell_execution_limits: Some(SessionCellExecutionLimits {
                max_yield_time_ms: Some(250),
                max_heap_size_bytes: Some(16 * 1_024 * 1_024),
            }),
        },
        &[0x0a, 0x08, 0x08, 0xfa, 0x01, 0x10, 0x80, 0x80, 0x80, 0x08],
    );
    assert_wire_fixture(
        OpenSessionRequest {
            cell_execution_limits: Some(SessionCellExecutionLimits {
                max_yield_time_ms: Some(0),
                max_heap_size_bytes: None,
            }),
        },
        &[0x0a, 0x02, 0x08, 0x00],
    );
    assert_wire_fixture(OpenSessionRequest::default(), &[]);
}

#[test]
fn tool_call_wire_fixture_pins_callback_field_numbers() {
    assert_wire_fixture(
        ToolCall {
            session_id: "s".to_string(),
            execution_id: "e".to_string(),
            cell_id: "c".to_string(),
            invocation_id: "i".to_string(),
            runtime_tool_call_id: "r".to_string(),
            tool_name: Some(ToolName {
                name: "n".to_string(),
                namespace: Some(String::new()),
            }),
            tool_kind: ToolKind::Freeform as i32,
            input_json: Some(Vec::new()),
            sequence: 1,
        },
        &[
            0x0a, 0x01, b's', 0x12, 0x01, b'e', 0x1a, 0x01, b'c', 0x22, 0x01, b'i', 0x2a,
            0x01, b'r', 0x32, 0x05, 0x0a, 0x01, b'n', 0x12, 0x00, 0x38, 0x02, 0x42, 0x00, 0x48,
            0x01,
        ],
    );
}

#[test]
fn cell_closed_wire_fixture_pins_session_event_variant() {
    assert_wire_fixture(
        SessionEvent {
            event: Some(session_event::Event::CellClosed(CellClosed {
                execution_id: "e".to_string(),
                cell_id: "c".to_string(),
                final_tool_call_sequence: 9,
            })),
        },
        &[0x2a, 0x08, 0x0a, 0x01, b'e', 0x12, 0x01, b'c', 0x18, 0x09],
    );
}

#[test]
fn content_item_wire_fixture_pins_supported_variants() {
    assert_wire_fixture(
        ContentItem {
            item: Some(content_item::Item::Text(TextContent {
                text: "t".to_string(),
            })),
        },
        &[0x0a, 0x03, 0x0a, 0x01, b't'],
    );
    assert_wire_fixture(
        ContentItem {
            item: Some(content_item::Item::Image(ImageContent {
                image_url: "i".to_string(),
                detail: Some(ImageDetail::Original as i32),
            })),
        },
        &[0x12, 0x05, 0x0a, 0x01, b'i', 0x10, 0x04],
    );
}

#[test]
fn content_item_ignores_the_retired_audio_wire_field() {
    let retired_audio = [0x1a, 0x03, 0x0a, 0x01, b'a'];
    let decoded = ContentItem::decode(retired_audio.as_slice())
        .expect("retired audio field should remain a valid unknown field");

    assert_eq!(decoded, ContentItem::default());
    assert_eq!(decoded.encode_to_vec(), Vec::<u8>::new());
}
