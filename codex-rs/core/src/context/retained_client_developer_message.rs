use super::ContextualUserFragment;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_utils_string::approx_bytes_for_tokens;
use codex_utils_string::approx_token_count;
use codex_utils_string::take_bytes_at_char_boundary;
use codex_utils_string::truncate_middle_with_token_budget;

pub(crate) const RETAINED_CLIENT_DEVELOPER_MESSAGE_MAX_TOKENS: usize = 1_000;
const ITEM_OVERHEAD_RESERVE_TOKENS: usize = 50;
const TRUNCATION_MARKER_RESERVE_TOKENS: usize = 50;
const RETAINED_TEXT_MAX_TOKENS: usize =
    RETAINED_CLIENT_DEVELOPER_MESSAGE_MAX_TOKENS - ITEM_OVERHEAD_RESERVE_TOKENS;
const RETAINED_TEXT_TRUNCATION_TOKENS: usize =
    RETAINED_TEXT_MAX_TOKENS - TRUNCATION_MARKER_RESERVE_TOKENS;

/// Bounded textual form of a client developer message retained across context windows.
///
/// Text parts are joined in order. Image and audio parts are omitted, and a message with no
/// text is not eligible for retention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetainedClientDeveloperMessage {
    text: String,
}

impl RetainedClientDeveloperMessage {
    pub(crate) fn from_response_item(item: &ResponseItem) -> Option<Self> {
        let ResponseItem::Message { role, content, .. } = item else {
            return None;
        };
        if role != "developer" {
            return None;
        }

        let mut text = String::new();
        for part in content {
            let part = match part {
                ContentItem::InputText { text } | ContentItem::OutputText { text } => text,
                ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => continue,
            };
            if part.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(part);
        }
        if text.is_empty() {
            return None;
        }

        Some(Self {
            text: bound_retained_text(&text),
        })
    }
}

impl ContextualUserFragment for RetainedClientDeveloperMessage {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("", "")
    }

    fn body(&self) -> String {
        self.text.clone()
    }
}

fn bound_retained_text(text: &str) -> String {
    if approx_token_count(text) <= RETAINED_TEXT_MAX_TOKENS {
        return text.to_string();
    }

    let (bounded, _) = truncate_middle_with_token_budget(text, RETAINED_TEXT_TRUNCATION_TOKENS);
    if approx_token_count(&bounded) <= RETAINED_CLIENT_DEVELOPER_MESSAGE_MAX_TOKENS {
        return bounded;
    }

    take_bytes_at_char_boundary(
        &bounded,
        approx_bytes_for_tokens(RETAINED_CLIENT_DEVELOPER_MESSAGE_MAX_TOKENS),
    )
    .to_string()
}

#[cfg(test)]
#[path = "retained_client_developer_message_tests.rs"]
mod tests;
