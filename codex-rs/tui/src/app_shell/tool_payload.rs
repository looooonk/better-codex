use super::tool_output::ToolOutputBuffer;
use codex_app_server_protocol::DynamicToolCallOutputContentItem;
use codex_app_server_protocol::ImageGenerationFailure;
use codex_app_server_protocol::ImageGenerationItem;
use codex_app_server_protocol::McpToolCallResult;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ImageReference;
use serde_json::Value;
use std::io::Write;

const MAX_CONTENT_BLOCKS: usize = 256;
const MAX_REFERENCE_CHARS: usize = 4_096;
const MAX_JSON_BYTES: usize = 256 * 1_024;

pub(super) enum ArtifactHost {
    Local,
    Remote,
}

pub(super) fn function_output(output: &FunctionCallOutputBody) -> Option<ToolOutputBuffer> {
    let mut body = Payload::default();
    match output {
        FunctionCallOutputBody::Text(text) => body.text(text),
        FunctionCallOutputBody::ContentItems(items) => {
            for item in items.iter().take(MAX_CONTENT_BLOCKS) {
                match item {
                    FunctionCallOutputContentItem::InputText { text } => body.text(text),
                    FunctionCallOutputContentItem::InputImage { image, .. } => match image {
                        ImageReference::Inline { image_url } => body.media("Image", image_url),
                        ImageReference::File { file_id } => body.reference("Image file", file_id),
                    },
                    FunctionCallOutputContentItem::InputAudio { audio_url } => {
                        body.media("Audio", audio_url)
                    }
                    FunctionCallOutputContentItem::EncryptedContent { .. } => {
                        body.text("[Encrypted tool content]")
                    }
                }
            }
            body.omitted(items.len());
        }
    }
    body.finish()
}

pub(super) fn dynamic_output(
    items: &[DynamicToolCallOutputContentItem],
) -> Option<ToolOutputBuffer> {
    let mut body = Payload::default();
    for item in items.iter().take(MAX_CONTENT_BLOCKS) {
        match item {
            DynamicToolCallOutputContentItem::InputText { text } => body.text(text),
            DynamicToolCallOutputContentItem::InputImage { image_url } => {
                body.media("Image", image_url)
            }
            DynamicToolCallOutputContentItem::InputAudio { audio_url } => {
                body.media("Audio", audio_url)
            }
        }
    }
    body.omitted(items.len());
    body.finish()
}

pub(super) fn mcp_output(result: &McpToolCallResult) -> Option<ToolOutputBuffer> {
    let mut body = Payload::default();
    for content in result.content.iter().take(MAX_CONTENT_BLOCKS) {
        match content.get("type").and_then(Value::as_str) {
            Some("text") => body.text(
                content
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            Some("image" | "audio") => {
                let label = if content["type"] == "image" {
                    "Image"
                } else {
                    "Audio"
                };
                let mime = content
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown format");
                body.reference(label, &format!("[inline {mime} attachment]"));
            }
            Some("resource_link") => {
                let label = content
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Resource");
                body.reference(
                    label,
                    content
                        .get("uri")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                );
                if let Some(description) = content.get("description").and_then(Value::as_str) {
                    body.text(description);
                }
            }
            Some("resource") => {
                let resource = &content["resource"];
                if let Some(uri) = resource.get("uri").and_then(Value::as_str) {
                    body.reference("Resource", uri);
                }
                if let Some(text) = resource.get("text").and_then(Value::as_str) {
                    body.text(text);
                }
                if resource.get("blob").is_some() {
                    let mime = resource
                        .get("mimeType")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown format");
                    body.reference("Attachment", mime);
                }
            }
            _ => body.json(content),
        }
    }
    body.omitted(result.content.len());
    if let Some(structured) = &result.structured_content {
        body.text("Structured result:");
        body.json(structured);
    }
    body.finish()
}

pub(super) fn image_generation(
    item: &ImageGenerationItem,
    host: ArtifactHost,
) -> Option<ToolOutputBuffer> {
    let mut body = Payload::default();
    if let Some(prompt) = &item.revised_prompt {
        body.text(prompt);
    }
    if let Some(saved) = &item.saved_path {
        match host {
            ArtifactHost::Local => {
                let reference = url::Url::from_file_path(saved.as_path())
                    .map_or_else(|_| saved.display().to_string(), |url| url.to_string());
                body.reference("Saved to", &reference);
            }
            ArtifactHost::Remote => {
                body.reference("Saved on remote host", &saved.display().to_string())
            }
        }
    } else if !item.result.is_empty() {
        // The extension's result may contain raw base64, never a display-ready message.
        if item.result.starts_with("https://")
            || item.result.starts_with("http://")
            || item.result.starts_with("data:")
        {
            body.media("Image", &item.result);
        } else {
            body.text("[Generated image content; no saved path provided]");
        }
    }
    if item.transparent_background == Some(true) {
        body.text("Transparent background");
    }
    if let Some(ImageGenerationFailure::UsageLimitExceeded {
        limit_id,
        resets_at,
    }) = &item.failure
    {
        body.reference("Image generation usage limit reached", limit_id);
        if let Some(resets_at) = resets_at {
            body.text(&format!("Limit resets at {resets_at} (Unix time)"));
        }
    }
    body.finish()
}

pub(super) fn web_results(results: &[Value]) -> Option<ToolOutputBuffer> {
    let mut body = Payload::default();
    for result in results.iter().take(MAX_CONTENT_BLOCKS) {
        body.json(result);
    }
    body.omitted(results.len());
    body.finish()
}

struct Payload(ToolOutputBuffer);

impl Default for Payload {
    fn default() -> Self {
        Self(ToolOutputBuffer::from(""))
    }
}

impl Payload {
    fn text(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        if !self.0.is_empty() {
            self.0.append("\n");
        }
        self.0.append(text);
    }

    fn reference(&mut self, label: &str, reference: &str) {
        let label: String = label.chars().take(MAX_REFERENCE_CHARS).collect();
        let abbreviated: String = reference.chars().take(MAX_REFERENCE_CHARS).collect();
        let suffix = if abbreviated.len() < reference.len() {
            "... [reference truncated]"
        } else {
            ""
        };
        self.text(&format!("{label}: {abbreviated}{suffix}"));
    }

    fn media(&mut self, label: &str, reference: &str) {
        if reference.starts_with("data:") {
            let format = reference
                .strip_prefix("data:")
                .unwrap_or_default()
                .split([';', ','])
                .next()
                .unwrap_or_default();
            let format: String = format.chars().take(80).collect();
            self.reference(label, &format!("[inline {format} attachment]"));
        } else {
            self.reference(label, reference);
        }
    }

    fn json(&mut self, value: &Value) {
        let mut writer = BoundedJson(Vec::new());
        let truncated = serde_json::to_writer_pretty(&mut writer, value).is_err();
        self.text(&String::from_utf8_lossy(&writer.0));
        if truncated {
            self.text("... structured result truncated ...");
        }
    }

    fn omitted(&mut self, count: usize) {
        if count > MAX_CONTENT_BLOCKS {
            self.text(&format!(
                "... {} additional content blocks omitted ...",
                count - MAX_CONTENT_BLOCKS
            ));
        }
    }

    fn finish(self) -> Option<ToolOutputBuffer> {
        (!self.0.is_empty()).then_some(self.0)
    }
}

struct BoundedJson(Vec<u8>);

impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let remaining = MAX_JSON_BYTES.saturating_sub(self.0.len());
        let count = remaining.min(bytes.len());
        self.0.extend_from_slice(&bytes[..count]);
        if count == 0 && !bytes.is_empty() {
            Err(std::io::Error::other("structured output limit reached"))
        } else {
            Ok(count)
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "tool_payload_tests.rs"]
mod tests;
