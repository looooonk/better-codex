//! App-level user message model shared by legacy and app-shell wiring.

use std::path::PathBuf;

use crate::bottom_pane::LocalImageAttachment;
use crate::bottom_pane::MentionBinding;
use codex_protocol::models::local_image_label_text;
use codex_protocol::user_input::TextElement;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UserMessage {
    pub(crate) text: String,
    pub(crate) local_images: Vec<LocalImageAttachment>,
    /// Remote image attachments represented as URLs (for example data URLs)
    /// provided by app-server clients.
    ///
    /// Unlike `local_images`, these are not created by TUI image attach/paste
    /// flows. The TUI can restore and remove them while editing/backtracking.
    pub(crate) remote_image_urls: Vec<String>,
    pub(crate) text_elements: Vec<TextElement>,
    pub(crate) mention_bindings: Vec<MentionBinding>,
}

impl From<String> for UserMessage {
    fn from(text: String) -> Self {
        Self {
            text,
            local_images: Vec::new(),
            remote_image_urls: Vec::new(),
            // Plain text conversion has no UI element ranges.
            text_elements: Vec::new(),
            mention_bindings: Vec::new(),
        }
    }
}

impl From<&str> for UserMessage {
    fn from(text: &str) -> Self {
        Self {
            text: text.to_string(),
            local_images: Vec::new(),
            remote_image_urls: Vec::new(),
            // Plain text conversion has no UI element ranges.
            text_elements: Vec::new(),
            mention_bindings: Vec::new(),
        }
    }
}

pub(crate) fn create_initial_user_message(
    text: Option<String>,
    local_image_paths: Vec<PathBuf>,
    text_elements: Vec<TextElement>,
) -> Option<UserMessage> {
    let text = text.unwrap_or_default();
    if text.is_empty() && local_image_paths.is_empty() {
        None
    } else {
        let local_images = local_image_paths
            .into_iter()
            .enumerate()
            .map(|(idx, path)| LocalImageAttachment {
                placeholder: local_image_label_text(idx + 1),
                path,
            })
            .collect();
        Some(UserMessage {
            text,
            local_images,
            remote_image_urls: Vec::new(),
            text_elements,
            mention_bindings: Vec::new(),
        })
    }
}
