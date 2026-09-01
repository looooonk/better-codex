use std::collections::VecDeque;

use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use codex_secrets::redact_secrets;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;

use crate::evidence::GuardianEvidenceEntry;

pub(crate) const MAX_EVIDENCE_ENTRIES: usize = 40;
const MAX_EVIDENCE_ENTRY_TOKENS: usize = 1_000;
const TRUNCATION_MARKER_TOKENS: usize = 16;
const MANUAL_APPROVAL_DEVELOPER_PREFIX: &str =
    "The user has manually approved a specific action that was previously `Rejected`.";

pub(crate) fn bounded_transcript<'a>(
    items: impl IntoIterator<Item = &'a ResponseItem>,
) -> Vec<GuardianEvidenceEntry> {
    let mut entries = VecDeque::with_capacity(MAX_EVIDENCE_ENTRIES);
    for item in items {
        let Some(entry) = transcript_entry(item) else {
            continue;
        };
        if entries.len() == MAX_EVIDENCE_ENTRIES {
            entries.pop_front();
        }
        entries.push_back(entry);
    }
    entries.into()
}

fn transcript_entry(item: &ResponseItem) -> Option<GuardianEvidenceEntry> {
    let (kind, provenance, text) = match item {
        ResponseItem::Message { role, content, .. } => {
            let text = content
                .iter()
                .filter_map(|item| match item {
                    ContentItem::InputText { text } | ContentItem::OutputText { text }
                        if !text.trim().is_empty() =>
                    {
                        Some(text.as_str())
                    }
                    ContentItem::InputText { .. }
                    | ContentItem::OutputText { .. }
                    | ContentItem::InputImage { .. }
                    | ContentItem::InputAudio { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let include = matches!(role.as_str(), "user" | "assistant")
                || role == "developer" && text.starts_with(MANUAL_APPROVAL_DEVELOPER_PREFIX);
            include.then_some(("message", Some(role.as_str()), text))?
        }
        ResponseItem::AgentMessage {
            author, content, ..
        } => (
            "agent_message",
            Some(author.as_str()),
            plaintext_agent_message_content(content)?,
        ),
        ResponseItem::FunctionCall {
            name, arguments, ..
        }
        | ResponseItem::CustomToolCall {
            name,
            input: arguments,
            ..
        } => ("tool_call", Some(name.as_str()), arguments.clone()),
        ResponseItem::FunctionCallOutput { output, .. }
        | ResponseItem::CustomToolCallOutput { output, .. } => {
            ("tool_output", None, output.body.to_text()?)
        }
        ResponseItem::Reasoning {
            summary, content, ..
        } => {
            let text = summary
                .iter()
                .map(|item| match item {
                    ReasoningItemReasoningSummary::SummaryText { text } => text.as_str(),
                })
                .chain(content.iter().flatten().map(|item| match item {
                    ReasoningItemContent::ReasoningText { text }
                    | ReasoningItemContent::Text { text } => text.as_str(),
                }))
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            ("reasoning", None, text)
        }
        ResponseItem::LocalShellCall { action, .. } => (
            "tool_call",
            Some("shell"),
            serde_json::to_string(action).ok()?,
        ),
        ResponseItem::WebSearchCall {
            action: Some(action),
            ..
        } => (
            "tool_call",
            Some("web_search"),
            serde_json::to_string(action).ok()?,
        ),
        ResponseItem::ToolSearchCall { arguments, .. } => (
            "tool_call",
            Some("tool_search"),
            serde_json::to_string(arguments).ok()?,
        ),
        ResponseItem::ToolSearchOutput { tools, .. } => (
            "tool_output",
            Some("tool_search"),
            serde_json::to_string(tools).ok()?,
        ),
        ResponseItem::AdditionalTools { .. }
        | ResponseItem::WebSearchCall { action: None, .. }
        | ResponseItem::ImageGenerationCall { .. }
        | ResponseItem::Compaction { .. }
        | ResponseItem::CompactionTrigger { .. }
        | ResponseItem::ContextCompaction { .. }
        | ResponseItem::Other => return None,
    };
    let text = bounded_redacted_text(text);
    if text.trim().is_empty() {
        return None;
    }
    Some(GuardianEvidenceEntry {
        kind: kind.to_string(),
        provenance: provenance.map(|value| bounded_redacted_text(value.to_string())),
        text,
    })
}

pub(crate) fn bounded_redacted_text(text: String) -> String {
    truncate_text(
        &redact_secrets(text),
        TruncationPolicy::Tokens(
            MAX_EVIDENCE_ENTRY_TOKENS.saturating_sub(TRUNCATION_MARKER_TOKENS),
        ),
    )
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
