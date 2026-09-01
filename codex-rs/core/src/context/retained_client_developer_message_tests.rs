use super::*;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;

fn developer_message(content: Vec<ContentItem>) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content,
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn retains_only_text_parts_in_order() {
    let fragment = RetainedClientDeveloperMessage::from_response_item(&developer_message(vec![
        ContentItem::InputText {
            text: "first".to_string(),
        },
        ContentItem::InputImage {
            image_url: "data:image/png;base64,aA==".to_string(),
            detail: None,
        },
        ContentItem::OutputText {
            text: "second".to_string(),
        },
        ContentItem::InputAudio {
            audio_url: "data:audio/wav;base64,aA==".to_string(),
        },
    ]))
    .expect("textual developer message should be retained");

    let item: ResponseItem = ContextualUserFragment::into(fragment);
    assert_eq!(
        item,
        developer_message(vec![ContentItem::InputText {
            text: "first\nsecond".to_string(),
        }])
    );
}

#[test]
fn omits_messages_without_text() {
    let fragment = RetainedClientDeveloperMessage::from_response_item(&developer_message(vec![
        ContentItem::InputImage {
            image_url: "data:image/png;base64,aA==".to_string(),
            detail: None,
        },
    ]));

    assert_eq!(fragment, None);
}

#[test]
fn truncates_rendered_message_below_hard_item_cap() {
    let text = format!("start-{}-end", "x".repeat(50_000));
    let fragment = RetainedClientDeveloperMessage::from_response_item(&developer_message(vec![
        ContentItem::InputText { text },
    ]))
    .expect("textual developer message should be retained");
    let rendered = fragment.render();

    assert!(rendered.starts_with("start-"));
    assert!(rendered.ends_with("-end"));
    assert!(rendered.contains("tokens truncated"));
    assert!(approx_token_count(&rendered) <= RETAINED_CLIENT_DEVELOPER_MESSAGE_MAX_TOKENS);
}
