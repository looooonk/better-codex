use super::workspace_requests::workspace_request_id;
use crate::app_server_session::HistoryReader;
use crate::app_server_session::read_thread_history;
use crate::session_transcript::RawReasoningVisibility;
use crate::session_transcript::thread_item_to_transcript_lines;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadRealtimeItemContent;
use codex_app_server_protocol::ThreadRealtimeTranscriptRole;
use codex_app_server_protocol::ThreadTimelineEntry;
use codex_app_server_protocol::ThreadTimelineListParams;
use codex_app_server_protocol::ThreadTimelineListResponse;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_protocol::ThreadId;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::bail;
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

const MAX_EXPORT_BYTES: usize = 64 * 1024 * 1024;

/// Reads the native timeline, retaining turn history for older server compatibility.
pub(super) trait ExportReader: HistoryReader {
    fn timeline(
        &self,
        params: ThreadTimelineListParams,
    ) -> impl std::future::Future<Output = Result<ThreadTimelineListResponse, TypedRequestError>> + Send;
}

pub(super) struct NativeExportReader(pub(super) AppServerRequestHandle);

impl HistoryReader for NativeExportReader {
    fn metadata(
        &self,
        thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<ThreadReadResponse, TypedRequestError>> + Send
    {
        self.0.request_typed(ClientRequest::ThreadRead {
            request_id: workspace_request_id("export-metadata"),
            params: ThreadReadParams {
                thread_id: thread_id.to_string(),
                include_turns: false,
            },
        })
    }

    fn turns(
        &self,
        params: ThreadTurnsListParams,
    ) -> impl std::future::Future<Output = Result<ThreadTurnsListResponse, TypedRequestError>> + Send
    {
        self.0.request_typed(ClientRequest::ThreadTurnsList {
            request_id: workspace_request_id("export-turns"),
            params,
        })
    }
}

impl ExportReader for NativeExportReader {
    fn timeline(
        &self,
        params: ThreadTimelineListParams,
    ) -> impl std::future::Future<Output = Result<ThreadTimelineListResponse, TypedRequestError>> + Send
    {
        self.0.request_typed(ClientRequest::ThreadTimelineList {
            request_id: workspace_request_id("export-timeline"),
            params,
        })
    }
}

pub(super) async fn export(
    client: impl ExportReader,
    thread_id: ThreadId,
    path: PathBuf,
) -> Result<String> {
    let response =
        tokio::time::timeout(Duration::from_secs(/*secs*/ 30), client.metadata(thread_id))
            .await
            .wrap_err("export metadata request timed out")??;
    let thread = response.thread;
    let mut pages = Vec::new();
    let mut cursor = None;
    let mut cursors = HashSet::new();
    let mut bytes = 0;
    let mut legacy = false;
    for _ in 0..100 {
        let response = tokio::time::timeout(
            Duration::from_secs(/*secs*/ 30),
            client.timeline(ThreadTimelineListParams {
                thread_id: thread_id.to_string(),
                cursor: cursor.take(),
                limit: Some(500),
            }),
        )
        .await
        .wrap_err("export history request timed out")?;
        let response = match response {
            Ok(response) => response,
            Err(TypedRequestError::Server { source, .. }) if source.code == -32601 => {
                legacy = true;
                break;
            }
            Err(error) => return Err(error.into()),
        };
        if response.data.len() > 500 {
            bail!("export page exceeded its requested size");
        }
        let mut page = String::new();
        for entry in response.data {
            match entry {
                ThreadTimelineEntry::Item { item, .. } => append_item(&mut page, &thread, &item)?,
                ThreadTimelineEntry::Realtime { item, .. } => match item.content {
                    ThreadRealtimeItemContent::TranscriptSegment { role, text } => {
                        let label = match role {
                            ThreadRealtimeTranscriptRole::User => "You (voice)",
                            ThreadRealtimeTranscriptRole::Assistant => "Assistant (voice)",
                        };
                        page.push_str(&format!("## {label}\n\n{text}\n\n"));
                    }
                    ThreadRealtimeItemContent::RealtimeSessionStarted
                    | ThreadRealtimeItemContent::RealtimeSessionClosed { .. }
                    | ThreadRealtimeItemContent::BemItemPromoted { .. } => {}
                },
                ThreadTimelineEntry::TurnStarted { .. }
                | ThreadTimelineEntry::TurnCompleted { .. } => {}
            }
            if page.len() > MAX_EXPORT_BYTES.saturating_sub(bytes) {
                bail!("conversation exceeds the 64 MiB export limit");
            }
        }
        bytes += page.len();
        if bytes > MAX_EXPORT_BYTES {
            bail!("conversation exceeds the 64 MiB export limit");
        }
        pages.push(page);
        cursor = response.next_cursor;
        let Some(next) = &cursor else {
            break;
        };
        if !cursors.insert(next.clone()) {
            bail!("server returned a repeated export cursor");
        }
    }
    if cursor.is_some() {
        bail!("conversation exceeds the 50,000-entry export limit");
    }
    let mut markdown = format!(
        "# {}\n\n",
        thread
            .name
            .as_deref()
            .unwrap_or("Better Codex conversation")
    );
    if legacy {
        let thread = read_thread_history(client, thread_id).await?;
        for item in thread.turns.iter().flat_map(|turn| &turn.items) {
            append_item(&mut markdown, &thread, item)?;
            if markdown.len() > MAX_EXPORT_BYTES {
                bail!("conversation exceeds the 64 MiB export limit");
            }
        }
    } else {
        for page in pages.into_iter().rev() {
            markdown.push_str(&page);
        }
    }
    if markdown.len() > MAX_EXPORT_BYTES {
        bail!("conversation exceeds the 64 MiB export limit");
    }
    let display = path.display().to_string();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("export destination has no parent directory"))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(markdown.as_bytes())?;
        temporary.flush()?;
        temporary
            .persist_noclobber(&path)
            .wrap_err("could not save export; choose a new filename")?;
        Ok(())
    })
    .await??;
    Ok(format!("Conversation exported to {display}"))
}

fn append_item(markdown: &mut String, thread: &Thread, item: &ThreadItem) -> Result<()> {
    match item {
        ThreadItem::AgentMessage {
            text, questions, ..
        } => {
            let text = crate::assistant_message::with_questions(text, questions.as_deref());
            markdown.push_str(&format!("## Assistant\n\n{text}\n\n"));
        }
        ThreadItem::Plan { text, .. } => markdown.push_str(&format!("## Plan\n\n{text}\n\n")),
        ThreadItem::UserMessage { content, .. } => {
            use codex_app_server_protocol::ImageReference;
            use codex_app_server_protocol::UserInput;
            markdown.push_str("## You\n\n");
            for input in content {
                match input {
                    UserInput::Text { text, .. } => {
                        markdown.push_str(text);
                        markdown.push_str("\n\n");
                    }
                    UserInput::Image {
                        image: ImageReference::Inline { url },
                        ..
                    } => append_attachment(markdown, "Image", url),
                    UserInput::Image {
                        image: ImageReference::File { file_id },
                        ..
                    } => {
                        markdown.push_str("Image attachment reference:\n\n");
                        append_code(markdown, "", file_id);
                    }
                    UserInput::LocalImage { path, .. } => {
                        append_attachment(markdown, "Image", &path.display().to_string())
                    }
                    UserInput::Audio { url } => append_attachment(markdown, "Audio", url),
                    UserInput::LocalAudio { path } => {
                        append_attachment(markdown, "Audio", &path.display().to_string())
                    }
                    UserInput::Skill { name, path } => {
                        append_attachment(markdown, name, &path.display().to_string())
                    }
                    UserInput::Mention { name, path } => append_attachment(markdown, name, path),
                }
            }
        }
        ThreadItem::CommandExecution {
            command,
            cwd,
            status,
            aggregated_output,
            exit_code,
            ..
        } => {
            markdown.push_str(&format!(
                "## Command ({status:?})\n\nWorking directory: {cwd}\n\n"
            ));
            append_code(markdown, "sh", command);
            if let Some(output) = aggregated_output {
                append_code(markdown, "text", output);
            }
            if let Some(code) = exit_code {
                markdown.push_str(&format!("Exit code: {code}\n\n"));
            }
        }
        ThreadItem::FileChange {
            changes, status, ..
        } => {
            markdown.push_str(&format!("## File changes ({status:?})\n\n"));
            for change in changes {
                markdown.push_str(&format!("### {} ({:?})\n\n", change.path, change.kind));
                append_code(markdown, "diff", &change.diff);
            }
        }
        ThreadItem::McpToolCall {
            server,
            tool,
            status,
            arguments,
            result,
            error,
            ..
        } => {
            markdown.push_str(&format!(
                "## Tool: {server}/{tool} ({status:?})\n\nArguments:\n\n"
            ));
            append_code(markdown, "json", &serde_json::to_string_pretty(arguments)?);
            if let Some(result) = result {
                markdown.push_str("Result:\n\n");
                append_code(markdown, "json", &serde_json::to_string_pretty(result)?);
            }
            if let Some(error) = error {
                append_code(markdown, "text", &error.message);
            }
        }
        ThreadItem::DynamicToolCall {
            namespace,
            tool,
            status,
            arguments,
            content_items,
            success,
            ..
        } => {
            use codex_app_server_protocol::DynamicToolCallOutputContentItem;
            let name = namespace
                .as_ref()
                .map_or_else(|| tool.clone(), |namespace| format!("{namespace}.{tool}"));
            markdown.push_str(&format!("## Tool: {name} ({status:?})\n\nArguments:\n\n"));
            append_code(markdown, "json", &serde_json::to_string_pretty(arguments)?);
            for content in content_items.iter().flatten() {
                match content {
                    DynamicToolCallOutputContentItem::InputText { text } => {
                        append_code(markdown, "text", text)
                    }
                    DynamicToolCallOutputContentItem::InputImage { image_url } => {
                        append_attachment(markdown, "Image", image_url)
                    }
                    DynamicToolCallOutputContentItem::InputAudio { audio_url } => {
                        append_attachment(markdown, "Audio", audio_url)
                    }
                }
            }
            if let Some(success) = success {
                markdown.push_str(&format!("Success: {success}\n\n"));
            }
        }
        ThreadItem::FunctionCallOutput {
            name,
            namespace,
            output,
            ..
        } => {
            let name = namespace
                .as_ref()
                .map_or_else(|| name.clone(), |namespace| format!("{namespace}.{name}"));
            markdown.push_str(&format!("## Tool output: {name}\n\n"));
            match output {
                codex_protocol::models::FunctionCallOutputBody::Text(text) => {
                    append_code(markdown, "text", text)
                }
                codex_protocol::models::FunctionCallOutputBody::ContentItems(items) => {
                    append_code(markdown, "json", &serde_json::to_string_pretty(items)?)
                }
            }
        }
        ThreadItem::CollabAgentToolCall {
            tool,
            status,
            sender_thread_id,
            receiver_thread_ids,
            prompt,
            model,
            reasoning_effort,
            agents_states,
            ..
        } => {
            markdown.push_str(&format!(
                "## Agent tool: {tool:?} ({status:?})\n\nFrom: {sender_thread_id}\n\nTo: {}\n\n",
                receiver_thread_ids.join(", ")
            ));
            if let Some(model) = model {
                markdown.push_str(&format!("Model: {model}\n\n"));
            }
            if let Some(effort) = reasoning_effort {
                markdown.push_str(&format!("Reasoning: {effort:?}\n\n"));
            }
            if let Some(prompt) = prompt {
                append_code(markdown, "text", prompt);
            }
            if !agents_states.is_empty() {
                append_code(
                    markdown,
                    "json",
                    &serde_json::to_string_pretty(agents_states)?,
                );
            }
        }
        ThreadItem::ImageView { path, .. } => {
            markdown.push_str("## Viewed image\n\n");
            append_attachment(markdown, "Image", &path.to_string());
        }
        ThreadItem::ImageGeneration(item) => {
            markdown.push_str(&format!("## Image generation ({})\n\n", item.status));
            if let Some(prompt) = &item.revised_prompt {
                markdown.push_str(&format!("{prompt}\n\n"));
            }
            if let Some(failure) = &item.failure {
                append_code(markdown, "json", &serde_json::to_string_pretty(failure)?);
            }
            if let Some(path) = &item.saved_path {
                append_attachment(
                    markdown,
                    "Generated image",
                    &path.as_path().display().to_string(),
                );
            } else if !item.result.is_empty() {
                append_code(markdown, "text", &item.result);
            }
        }
        ThreadItem::WebSearch(item) => {
            markdown.push_str(&format!("## Web search\n\n{}\n\n", item.query));
            if let Some(action) = &item.action {
                append_code(markdown, "json", &serde_json::to_string_pretty(action)?);
            }
            if let Some(results) = &item.results {
                append_code(markdown, "json", &serde_json::to_string_pretty(results)?);
            }
        }
        ThreadItem::HookPrompt { .. }
        | ThreadItem::Reasoning { .. }
        | ThreadItem::SubAgentActivity { .. }
        | ThreadItem::Sleep(_)
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. }
        | ThreadItem::ContextCompaction { .. } => {
            let lines =
                thread_item_to_transcript_lines(item, thread, RawReasoningVisibility::Hidden);
            if !lines.is_empty() {
                markdown.push_str("## Activity\n\n");
                for line in lines {
                    markdown.push_str("    ");
                    for span in line.spans {
                        markdown.push_str(&span.content);
                    }
                    markdown.push('\n');
                }
                markdown.push('\n');
            }
        }
    }
    Ok(())
}

fn append_code(markdown: &mut String, language: &str, text: &str) {
    let longest = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    markdown.push_str(&format!("{fence}{language}\n{text}\n{fence}\n\n"));
}

fn append_attachment(markdown: &mut String, label: &str, destination: &str) {
    let label = label.replace(']', "\\]");
    let destination = destination
        .replace('<', "%3C")
        .replace('>', "%3E")
        .replace('\n', "%0A")
        .replace('\r', "%0D");
    markdown.push_str(&format!("[{label}](<{destination}>)\n\n"));
}

#[cfg(test)]
#[path = "transcript_export_tests.rs"]
mod tests;
