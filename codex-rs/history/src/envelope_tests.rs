use std::borrow::Borrow;

use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;

use super::*;

#[test]
fn accessors_preserve_response_item() {
    let expected_item = ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "hello".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    let mut envelope = ResponseItemEnvelope::new(expected_item.clone());
    envelope.metadata = Some(CodexHarnessMetadata {
        client_authored: true,
    });

    assert_eq!(&*envelope, &expected_item);
    assert_eq!(
        envelope.metadata,
        Some(CodexHarnessMetadata {
            client_authored: true,
        })
    );
    let borrowed: &ResponseItem = envelope.borrow();
    assert_eq!(borrowed, &expected_item);
    let replacement_item = ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: "goodbye".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    *envelope = replacement_item.clone();

    assert_eq!(envelope.into_item(), replacement_item);
}
