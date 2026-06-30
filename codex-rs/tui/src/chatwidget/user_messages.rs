//! User-message models and helpers for the chat widget.
//!
//! The app-server preserves user input as structured chunks, while chat history
//! renders a single prompt row. This module owns the draft/message data models,
//! merge/remap behavior, display projection, and the small compare key used to
//! suppress duplicate rows for pending steers.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use crate::bottom_pane::LocalImageAttachment;
use crate::thread_input_state::PendingSteerCompareKey;
use crate::thread_input_state::UserMessageHistoryOverride;
use crate::thread_input_state::UserMessageHistoryRecord;
use crate::user_message::UserMessage;
use codex_app_server_protocol::TextElement as AppServerTextElement;
use codex_app_server_protocol::UserInput;
use codex_protocol::models::local_image_label_text;
use codex_protocol::user_input::ByteRange;
use codex_protocol::user_input::TextElement;

use super::ChatWidget;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ShellEscapePolicy {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueueDrain {
    Continue,
    Stop,
}

#[derive(Debug)]
pub(super) struct PendingSteer {
    pub(super) user_message: UserMessage,
    pub(super) history_record: UserMessageHistoryRecord,
    pub(super) compare_key: PendingSteerCompareKey,
}

fn append_text_with_rebased_elements(
    target_text: &mut String,
    target_text_elements: &mut Vec<TextElement>,
    text: &str,
    text_elements: impl IntoIterator<Item = TextElement>,
) {
    let offset = target_text.len();
    target_text.push_str(text);
    target_text_elements.extend(text_elements.into_iter().map(|mut element| {
        element.byte_range.start += offset;
        element.byte_range.end += offset;
        element
    }));
}

pub(super) fn app_server_text_elements(elements: &[TextElement]) -> Vec<AppServerTextElement> {
    elements.iter().cloned().map(Into::into).collect()
}

fn build_placeholder_mapping(
    local_images: Vec<LocalImageAttachment>,
    next_label: &mut usize,
) -> (HashMap<String, String>, Vec<LocalImageAttachment>) {
    let mut mapping: HashMap<String, String> = HashMap::new();
    let mut remapped_images = Vec::new();
    for attachment in local_images {
        let new_placeholder = local_image_label_text(*next_label);
        *next_label += 1;
        mapping.insert(attachment.placeholder.clone(), new_placeholder.clone());
        remapped_images.push(LocalImageAttachment {
            placeholder: new_placeholder,
            path: attachment.path,
        });
    }
    (mapping, remapped_images)
}

fn remap_placeholders_in_text(
    text: String,
    text_elements: Vec<TextElement>,
    mapping: &HashMap<String, String>,
) -> (String, Vec<TextElement>) {
    if mapping.is_empty() {
        return (text, text_elements);
    }

    let mut elements = text_elements;
    elements.sort_by_key(|elem| elem.byte_range.start);

    let mut cursor = 0usize;
    let mut rebuilt = String::new();
    let mut rebuilt_elements = Vec::new();
    for mut elem in elements {
        let start = elem.byte_range.start.min(text.len());
        let end = elem.byte_range.end.min(text.len());
        if let Some(segment) = text.get(cursor..start) {
            rebuilt.push_str(segment);
        }

        let original = text.get(start..end).unwrap_or("");
        let placeholder = elem.placeholder(&text);
        let replacement = placeholder
            .and_then(|ph| mapping.get(ph))
            .map(String::as_str)
            .unwrap_or(original);

        let elem_start = rebuilt.len();
        rebuilt.push_str(replacement);
        let elem_end = rebuilt.len();

        if let Some(remapped) = placeholder.and_then(|ph| mapping.get(ph)) {
            elem.set_placeholder(Some(remapped.clone()));
        }
        elem.byte_range = (elem_start..elem_end).into();
        rebuilt_elements.push(elem);
        cursor = end;
    }
    if let Some(segment) = text.get(cursor..) {
        rebuilt.push_str(segment);
    }

    (rebuilt, rebuilt_elements)
}

pub(super) fn remap_colliding_paste_placeholders(
    mut message: UserMessage,
    mut pending_pastes: Vec<(String, String)>,
    used: &mut HashSet<String>,
) -> (UserMessage, Vec<(String, String)>) {
    let mut mapping = HashMap::new();
    for (placeholder, text) in &mut pending_pastes {
        if used.insert(placeholder.clone()) {
            continue;
        }

        let base = format!("[Pasted Content {} chars]", text.chars().count());
        let mut suffix = 2;
        let replacement = loop {
            let candidate = format!("{base} #{suffix}");
            if used.insert(candidate.clone()) {
                break candidate;
            }
            suffix += 1;
        };
        mapping.insert(placeholder.clone(), replacement.clone());
        *placeholder = replacement;
    }
    (message.text, message.text_elements) =
        remap_placeholders_in_text(message.text, message.text_elements, &mapping);
    (message, pending_pastes)
}

// When merging multiple queued drafts (e.g., after interrupt), each draft starts numbering
// its attachments at [Image #1]. Reassign placeholder labels based on the attachment list so
// the combined local_image_paths order matches the labels, even if placeholders were moved
// in the text (e.g., [Image #2] appearing before [Image #1]). Apply the same remapping to
// history overrides so restored drafts and rendered transcript entries agree.
fn remap_placeholders_for_message_and_history_record(
    message: UserMessage,
    history_record: UserMessageHistoryRecord,
    next_label: &mut usize,
) -> (UserMessage, UserMessageHistoryRecord) {
    let UserMessage {
        text,
        text_elements,
        local_images,
        remote_image_urls,
        mention_bindings,
    } = message;
    let (mapping, remapped_images) = build_placeholder_mapping(local_images, next_label);
    let (text, text_elements) = remap_placeholders_in_text(text, text_elements, &mapping);
    let history_record = match history_record {
        UserMessageHistoryRecord::Override(history) if !history.text.is_empty() => {
            let (text, text_elements) =
                remap_placeholders_in_text(history.text, history.text_elements, &mapping);
            UserMessageHistoryRecord::Override(UserMessageHistoryOverride {
                text,
                text_elements,
            })
        }
        record => record,
    };

    (
        UserMessage {
            text,
            local_images: remapped_images,
            remote_image_urls,
            text_elements,
            mention_bindings,
        },
        history_record,
    )
}

#[cfg(test)]
pub(super) fn remap_placeholders_for_message(
    message: UserMessage,
    next_label: &mut usize,
) -> UserMessage {
    remap_placeholders_for_message_and_history_record(
        message,
        UserMessageHistoryRecord::UserMessageText,
        next_label,
    )
    .0
}

fn remap_user_messages_with_history_records(
    messages: Vec<(UserMessage, UserMessageHistoryRecord)>,
) -> Vec<(UserMessage, UserMessageHistoryRecord)> {
    let total_remote_images = messages
        .iter()
        .map(|(message, _)| message.remote_image_urls.len())
        .sum::<usize>();
    let mut next_image_label = total_remote_images + 1;
    messages
        .into_iter()
        .map(|(message, history_record)| {
            remap_placeholders_for_message_and_history_record(
                message,
                history_record,
                &mut next_image_label,
            )
        })
        .collect()
}

pub(super) fn merge_user_messages(messages: Vec<UserMessage>) -> UserMessage {
    let messages = remap_user_messages_with_history_records(
        messages
            .into_iter()
            .map(|message| (message, UserMessageHistoryRecord::UserMessageText))
            .collect(),
    );
    merge_remapped_user_messages(messages.into_iter().map(|(message, _)| message))
}

fn merge_remapped_user_messages(messages: impl IntoIterator<Item = UserMessage>) -> UserMessage {
    let mut combined = UserMessage {
        text: String::new(),
        text_elements: Vec::new(),
        local_images: Vec::new(),
        remote_image_urls: Vec::new(),
        mention_bindings: Vec::new(),
    };

    for (idx, message) in messages.into_iter().enumerate() {
        if idx > 0 {
            combined.text.push('\n');
        }
        let UserMessage {
            text,
            text_elements,
            local_images,
            remote_image_urls,
            mention_bindings,
        } = message;
        append_text_with_rebased_elements(
            &mut combined.text,
            &mut combined.text_elements,
            &text,
            text_elements,
        );
        combined.local_images.extend(local_images);
        combined.remote_image_urls.extend(remote_image_urls);
        combined.mention_bindings.extend(mention_bindings);
    }

    combined
}

pub(super) fn user_message_for_restore(
    message: UserMessage,
    history_record: &UserMessageHistoryRecord,
) -> UserMessage {
    match history_record {
        UserMessageHistoryRecord::Override(history) if !history.text.is_empty() => UserMessage {
            text: history.text.clone(),
            text_elements: history.text_elements.clone(),
            ..message
        },
        UserMessageHistoryRecord::Override(_) | UserMessageHistoryRecord::UserMessageText => {
            message
        }
    }
}

pub(super) fn user_message_preview_text(
    message: &UserMessage,
    history_record: Option<&UserMessageHistoryRecord>,
) -> String {
    match history_record {
        Some(UserMessageHistoryRecord::Override(history)) if !history.text.is_empty() => {
            history.text.clone()
        }
        Some(UserMessageHistoryRecord::Override(_))
        | Some(UserMessageHistoryRecord::UserMessageText)
        | None => message.text.clone(),
    }
}

pub(super) fn user_message_display_for_history(
    message: UserMessage,
    history_record: &UserMessageHistoryRecord,
) -> UserMessageDisplay {
    let message = user_message_for_restore(message, history_record);
    ChatWidget::user_message_display_from_parts(
        message.text,
        message.text_elements,
        message
            .local_images
            .into_iter()
            .map(|image| image.path)
            .collect(),
        message.remote_image_urls,
    )
}

pub(super) fn merge_user_messages_with_history_record(
    messages: Vec<(UserMessage, UserMessageHistoryRecord)>,
) -> (UserMessage, UserMessageHistoryRecord) {
    let messages = remap_user_messages_with_history_records(messages);
    let history_record = if messages
        .iter()
        .all(|(_, record)| *record == UserMessageHistoryRecord::UserMessageText)
    {
        UserMessageHistoryRecord::UserMessageText
    } else {
        let mut history_text = String::new();
        let mut history_text_elements = Vec::new();
        let mut history_segment_count = 0usize;
        let mut append_history_segment = |text: &str, text_elements: Vec<TextElement>| {
            if history_segment_count > 0 {
                history_text.push('\n');
            }
            append_text_with_rebased_elements(
                &mut history_text,
                &mut history_text_elements,
                text,
                text_elements,
            );
            history_segment_count += 1;
        };
        for (message, record) in &messages {
            match record {
                UserMessageHistoryRecord::Override(history) if !history.text.is_empty() => {
                    append_history_segment(&history.text, history.text_elements.clone());
                }
                UserMessageHistoryRecord::Override(_) if message.text.is_empty() => {}
                UserMessageHistoryRecord::Override(_)
                | UserMessageHistoryRecord::UserMessageText => {
                    append_history_segment(&message.text, message.text_elements.clone());
                }
            }
        }
        UserMessageHistoryRecord::Override(UserMessageHistoryOverride {
            text: history_text,
            text_elements: history_text_elements,
        })
    };

    (
        merge_remapped_user_messages(messages.into_iter().map(|(message, _)| message)),
        history_record,
    )
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct UserMessageDisplay {
    pub(super) message: String,
    pub(super) remote_image_urls: Vec<String>,
    pub(super) local_images: Vec<PathBuf>,
    pub(super) text_elements: Vec<TextElement>,
}

impl ChatWidget {
    pub(super) fn user_message_display_from_parts(
        message: String,
        text_elements: Vec<TextElement>,
        local_images: Vec<PathBuf>,
        remote_image_urls: Vec<String>,
    ) -> UserMessageDisplay {
        let (message, prompt_request_offset) =
            crate::ide_context::extract_prompt_request_with_offset(&message);
        let prompt_request_end = prompt_request_offset + message.len();
        // Prompt context uses the same delimiter and stripping behavior as the desktop app and IDE
        // extension. The raw user message goes to the agent, but every surface renders only the
        // request after that delimiter, so keep elements inside the visible request and shift their
        // byte ranges to match.
        let text_elements = text_elements
            .into_iter()
            .filter_map(|element| {
                let range = element.byte_range;
                if range.start < prompt_request_offset || range.end > prompt_request_end {
                    return None;
                }

                Some(element.map_range(|range| ByteRange {
                    start: range.start - prompt_request_offset,
                    end: range.end - prompt_request_offset,
                }))
            })
            .collect();

        UserMessageDisplay {
            message: message.to_string(),
            remote_image_urls,
            local_images,
            text_elements,
        }
    }

    /// Build the compare key for a submitted pending steer without invoking the
    /// expensive request-serialization path. Pending steers only need to match the
    /// committed app-server `UserMessage` item emitted after input drains, which
    /// preserves flattened text and total image count.
    pub(super) fn pending_steer_compare_key_from_items(
        items: &[UserInput],
    ) -> PendingSteerCompareKey {
        let mut message = String::new();
        let mut image_count = 0;

        for item in items {
            match item {
                UserInput::Text { text, .. } => message.push_str(text),
                UserInput::Image { .. } | UserInput::LocalImage { .. } => image_count += 1,
                UserInput::Skill { .. } | UserInput::Mention { .. } => {}
            }
        }

        PendingSteerCompareKey {
            message,
            image_count,
        }
    }

    pub(super) fn user_message_display_from_inputs(items: &[UserInput]) -> UserMessageDisplay {
        let mut message = String::new();
        let mut remote_image_urls = Vec::new();
        let mut local_images = Vec::new();
        let mut text_elements = Vec::new();

        for item in items {
            match item {
                UserInput::Text {
                    text,
                    text_elements: current_text_elements,
                    ..
                } => append_text_with_rebased_elements(
                    &mut message,
                    &mut text_elements,
                    text,
                    current_text_elements.iter().map(|element| {
                        let range = element.byte_range.clone();
                        TextElement::new(
                            range.clone().into(),
                            element
                                .placeholder()
                                .or_else(|| text.get(range.start..range.end))
                                .map(str::to_string),
                        )
                    }),
                ),
                UserInput::Image { url, .. } => remote_image_urls.push(url.clone()),
                UserInput::LocalImage { path, .. } => local_images.push(path.clone()),
                UserInput::Skill { .. } | UserInput::Mention { .. } => {}
            }
        }

        Self::user_message_display_from_parts(
            message,
            text_elements,
            local_images,
            remote_image_urls,
        )
    }
}
