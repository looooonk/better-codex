use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::UserInput;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RewindAnchor {
    pub(super) before_turn_id: String,
}

impl RewindAnchor {
    pub(super) fn for_opening_item(turn_id: &str, item: &ThreadItem) -> Option<Self> {
        let ThreadItem::UserMessage { content, .. } = item else {
            return None;
        };
        match content.as_slice() {
            [
                UserInput::Text {
                    text,
                    text_elements,
                },
            ] if !text.trim().is_empty() && text_elements.is_empty() => Some(Self {
                before_turn_id: turn_id.to_string(),
            }),
            _ => None,
        }
    }
}
