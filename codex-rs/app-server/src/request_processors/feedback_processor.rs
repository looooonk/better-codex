use super::*;
#[cfg(target_os = "windows")]
use codex_feedback::WINDOWS_SANDBOX_LOG_ATTACHMENT_FILENAME;

const MAX_FEEDBACK_TREE_THREADS: usize = 8;
// Match the existing feedback ring-buffer budget instead of buffering an unbounded rollout.
const MAX_REDACTED_ROLLOUT_ATTACHMENT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct FeedbackRequestProcessor {
    auth_manager: Arc<AuthManager>,
    thread_manager: Arc<ThreadManager>,
    config: Arc<Config>,
    feedback: CodexFeedback,
    log_db: Option<LogDbLayer>,
    state_db: Option<StateDbHandle>,
}

impl FeedbackRequestProcessor {
    pub(crate) fn new(
        auth_manager: Arc<AuthManager>,
        thread_manager: Arc<ThreadManager>,
        config: Arc<Config>,
        feedback: CodexFeedback,
        log_db: Option<LogDbLayer>,
        state_db: Option<StateDbHandle>,
    ) -> Self {
        Self {
            auth_manager,
            thread_manager,
            config,
            feedback,
            log_db,
            state_db,
        }
    }

    pub(crate) async fn feedback_upload(
        &self,
        params: FeedbackUploadParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.upload_feedback_response(params)
            .await
            .map(|response| Some(response.into()))
    }

    async fn upload_feedback_response(
        &self,
        params: FeedbackUploadParams,
    ) -> Result<FeedbackUploadResponse, JSONRPCErrorError> {
        if !self.config.feedback_enabled {
            return Err(invalid_request(
                "sending feedback is disabled by configuration",
            ));
        }

        let FeedbackUploadParams {
            classification,
            reason,
            thread_id,
            include_logs,
            extra_log_files,
            tags,
        } = params;
        let mut upload_tags = tags.unwrap_or_default();

        let conversation_id = match thread_id.as_deref() {
            Some(thread_id) => match ThreadId::from_string(thread_id) {
                Ok(conversation_id) => Some(conversation_id),
                Err(err) => return Err(invalid_request(format!("invalid thread id: {err}"))),
            },
            None => None,
        };

        if let Some(chatgpt_user_id) = self
            .auth_manager
            .auth_cached()
            .and_then(|auth| auth.get_chatgpt_user_id())
        {
            tracing::info!(target: "feedback_tags", chatgpt_user_id);
        }
        if let Some(account_id) = self
            .auth_manager
            .auth_cached()
            .and_then(|auth| auth.get_account_id())
        {
            tracing::info!(target: "feedback_tags", account_id);
        }
        let snapshot = self.feedback.snapshot(conversation_id);
        let thread_id = snapshot.thread_id.clone();
        let (feedback_thread_ids, sqlite_feedback_logs, state_db_ctx) = if include_logs {
            if let Some(log_db) = self.log_db.as_ref() {
                log_db.flush().await;
            }
            let state_db_ctx = self.state_db.clone();
            let feedback_thread_ids = match conversation_id {
                Some(conversation_id) => match self
                    .thread_manager
                    .list_agent_subtree_thread_ids(conversation_id)
                    .await
                {
                    Ok(thread_ids) => thread_ids,
                    Err(err) => {
                        warn!(
                            "failed to list feedback subtree for thread_id={conversation_id}: {err}"
                        );
                        vec![conversation_id]
                    }
                },
                None => Vec::new(),
            };
            let mut feedback_thread_ids = feedback_thread_ids;
            let original_len = feedback_thread_ids.len();
            if let Some(conversation_id) = conversation_id {
                let mut descendant_thread_ids = feedback_thread_ids
                    .into_iter()
                    .filter(|thread_id| *thread_id != conversation_id)
                    .collect::<Vec<_>>();
                // Thread ids are UUIDv7, so lexicographic order tracks creation time.
                descendant_thread_ids.sort_unstable_by_key(ToString::to_string);
                if original_len > MAX_FEEDBACK_TREE_THREADS {
                    let keep_descendants = MAX_FEEDBACK_TREE_THREADS.saturating_sub(1);
                    let split_index = descendant_thread_ids.len().saturating_sub(keep_descendants);
                    descendant_thread_ids = descendant_thread_ids.split_off(split_index);
                    warn!(
                        "feedback log upload for thread_id={conversation_id:?} truncated from {original_len} threads to root plus {keep_descendants} most recent descendants"
                    );
                }
                feedback_thread_ids = Vec::with_capacity(descendant_thread_ids.len() + 1);
                feedback_thread_ids.push(conversation_id);
                feedback_thread_ids.extend(descendant_thread_ids);
            }
            let sqlite_feedback_logs = if let Some(state_db_ctx) = state_db_ctx.as_ref()
                && !feedback_thread_ids.is_empty()
            {
                let thread_id_texts = feedback_thread_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                let thread_id_refs = thread_id_texts
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                match state_db_ctx
                    .query_feedback_logs_for_threads(&thread_id_refs)
                    .await
                {
                    Ok(logs) if logs.is_empty() => None,
                    Ok(logs) => Some(logs),
                    Err(err) => {
                        let thread_ids = thread_id_texts.join(", ");
                        warn!(
                            "failed to query feedback logs from sqlite for thread_ids=[{thread_ids}]: {err}"
                        );
                        None
                    }
                }
            } else {
                None
            };
            (feedback_thread_ids, sqlite_feedback_logs, state_db_ctx)
        } else {
            (Vec::new(), None, None)
        };

        let mut rollout_attachments = Vec::new();
        let mut attachment_paths = Vec::new();
        let mut seen_attachment_paths = HashSet::new();
        if include_logs {
            for feedback_thread_id in &feedback_thread_ids {
                let Some(rollout_path) = self
                    .resolve_rollout_path(*feedback_thread_id, state_db_ctx.as_ref())
                    .await
                else {
                    continue;
                };
                if seen_attachment_paths.insert(rollout_path.clone())
                    && let Some(attachment) =
                        redacted_rollout_attachment(&rollout_path, /*filename_override*/ None).await
                {
                    rollout_attachments.push(attachment);
                }
            }
            if let Some(conversation_id) = conversation_id
                && let Ok(conversation) = self.thread_manager.get_thread(conversation_id).await
                && let Some(guardian_rollout_path) =
                    conversation.guardian_trunk_rollout_path().await
                && seen_attachment_paths.insert(guardian_rollout_path.clone())
                && let Some(attachment) = redacted_rollout_attachment(
                    &guardian_rollout_path,
                    Some(auto_review_rollout_filename(conversation_id)),
                )
                .await
            {
                rollout_attachments.push(attachment);
            }
            if let Some(sandbox_log_attachment) =
                windows_sandbox_log_attachment(&self.config.codex_home)
                && seen_attachment_paths.insert(sandbox_log_attachment.path.clone())
            {
                attachment_paths.push(sandbox_log_attachment);
            }
        }
        if let Some(extra_log_files) = extra_log_files {
            for extra_log_file in extra_log_files {
                if seen_attachment_paths.insert(extra_log_file.clone()) {
                    attachment_paths.push(FeedbackAttachmentPath {
                        path: extra_log_file,
                        attachment_filename_override: None,
                    });
                }
            }
        }

        let mut extra_attachments = Vec::new();
        if include_logs
            && let Some(doctor_report) =
                super::feedback_doctor_report::doctor_feedback_report(&self.config).await
        {
            extra_attachments.push(doctor_report.attachment);
            for (key, value) in doctor_report.tags {
                upload_tags.entry(key).or_insert(value);
            }
        }
        extra_attachments.extend(rollout_attachments);

        let session_source = self.thread_manager.session_source();

        let upload_result = tokio::task::spawn_blocking(move || {
            let tags = (!upload_tags.is_empty()).then_some(&upload_tags);
            snapshot.upload_feedback(FeedbackUploadOptions {
                classification: &classification,
                reason: reason.as_deref(),
                tags,
                include_logs,
                extra_attachments: &extra_attachments,
                extra_attachment_paths: &attachment_paths,
                session_source: Some(session_source),
                logs_override: sqlite_feedback_logs,
            })
        })
        .await;

        let upload_result = match upload_result {
            Ok(result) => result,
            Err(join_err) => {
                return Err(internal_error(format!(
                    "failed to upload feedback: {join_err}"
                )));
            }
        };

        upload_result.map_err(|err| internal_error(format!("failed to upload feedback: {err}")))?;
        Ok(FeedbackUploadResponse { thread_id })
    }

    async fn resolve_rollout_path(
        &self,
        conversation_id: ThreadId,
        state_db_ctx: Option<&StateDbHandle>,
    ) -> Option<PathBuf> {
        if let Ok(conversation) = self.thread_manager.get_thread(conversation_id).await
            && let Some(rollout_path) = conversation.rollout_path()
        {
            return Some(rollout_path);
        }

        let state_db_ctx = state_db_ctx?;
        state_db_ctx
            .find_rollout_path_by_id(conversation_id, /*archived_only*/ None)
            .await
            .unwrap_or_else(|err| {
                warn!("failed to resolve rollout path for thread_id={conversation_id}: {err}");
                None
            })
    }
}

async fn redacted_rollout_attachment(
    path: &Path,
    filename_override: Option<String>,
) -> Option<codex_feedback::FeedbackAttachment> {
    let mut reader = match codex_rollout::open_rollout_line_reader(path).await {
        Ok(reader) => reader,
        Err(err) => {
            warn!(path = %path.display(), %err, "failed to open rollout feedback attachment; skipping");
            return None;
        }
    };
    let mut buffer = Vec::new();
    let mut line_number = 0usize;
    loop {
        let line = match reader.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(err) => {
                warn!(path = %path.display(), %err, "failed to read rollout feedback attachment; skipping");
                return None;
            }
        };
        line_number = line_number.saturating_add(1);
        if line.trim().is_empty() {
            continue;
        }
        if line.len().saturating_add(1)
            > MAX_REDACTED_ROLLOUT_ATTACHMENT_BYTES.saturating_sub(buffer.len())
        {
            warn!(path = %path.display(), max_bytes = MAX_REDACTED_ROLLOUT_ATTACHMENT_BYTES, "rollout feedback attachment exceeds byte cap; skipping");
            return None;
        }
        let mut value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(err) => {
                warn!(path = %path.display(), line_number, %err, "malformed rollout feedback attachment; skipping");
                return None;
            }
        };
        codex_rollout::redact_persisted_json(&mut value);
        let mut redacted_line = match serde_json::to_vec(&value) {
            Ok(line) => line,
            Err(err) => {
                warn!(path = %path.display(), line_number, %err, "failed to serialize rollout feedback attachment; skipping");
                return None;
            }
        };
        redacted_line.push(b'\n');
        let Some(new_len) = buffer.len().checked_add(redacted_line.len()) else {
            warn!(path = %path.display(), max_bytes = MAX_REDACTED_ROLLOUT_ATTACHMENT_BYTES, "rollout feedback attachment size overflow; skipping");
            return None;
        };
        if new_len > MAX_REDACTED_ROLLOUT_ATTACHMENT_BYTES {
            warn!(path = %path.display(), max_bytes = MAX_REDACTED_ROLLOUT_ATTACHMENT_BYTES, "rollout feedback attachment exceeds byte cap; skipping");
            return None;
        }
        buffer.extend_from_slice(&redacted_line);
    }

    let filename = filename_override.unwrap_or_else(|| {
        codex_rollout::plain_rollout_path(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "rollout.jsonl".to_string())
    });
    Some(codex_feedback::FeedbackAttachment {
        filename,
        content_type: Some("text/plain".to_string()),
        buffer,
    })
}

fn auto_review_rollout_filename(thread_id: ThreadId) -> String {
    format!("auto-review-rollout-{thread_id}.jsonl")
}

#[cfg(target_os = "windows")]
fn windows_sandbox_log_attachment(codex_home: &Path) -> Option<FeedbackAttachmentPath> {
    let sandbox_log_path = codex_windows_sandbox::current_log_file_path_for_codex_home(codex_home);
    sandbox_log_path
        .is_file()
        .then_some(FeedbackAttachmentPath {
            path: sandbox_log_path,
            attachment_filename_override: Some(WINDOWS_SANDBOX_LOG_ATTACHMENT_FILENAME.to_string()),
        })
}

#[cfg(not(target_os = "windows"))]
fn windows_sandbox_log_attachment(_codex_home: &Path) -> Option<FeedbackAttachmentPath> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::io::Write;

    const SECRET: &str = "example_synthetic_bearer_token_123456";

    #[tokio::test]
    async fn compressed_rollout_attachment_is_redacted_and_remains_valid_jsonl() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let path = temp.path().join("rollout.jsonl.zst");
        let contents = format!(
            "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"function_call\",\"arguments\":\"{{\\\"authorization\\\":\\\"Bearer {SECRET}\\\"}}\",\"call_id\":\"call-1\"}}}}\n"
        );
        let file = std::fs::File::create(&path).expect("create compressed rollout");
        let mut encoder = zstd::stream::write::Encoder::new(file, 1).expect("create encoder");
        encoder
            .write_all(contents.as_bytes())
            .expect("compress rollout");
        encoder.finish().expect("finish compressed rollout");

        let attachment = redacted_rollout_attachment(&path, /*filename_override*/ None)
            .await
            .expect("create attachment");

        let text = String::from_utf8(attachment.buffer).expect("attachment should be utf-8");
        assert!(!text.contains(SECRET));
        assert!(text.contains("[REDACTED_SECRET]"));
        assert_eq!(attachment.filename, "rollout.jsonl");
        for line in text.lines() {
            serde_json::from_str::<serde_json::Value>(line).expect("line should remain valid JSON");
        }
    }

    #[tokio::test]
    async fn oversized_rollout_attachment_is_omitted() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let path = temp.path().join("rollout.jsonl");
        let contents = serde_json::json!({
            "message": "x".repeat(MAX_REDACTED_ROLLOUT_ATTACHMENT_BYTES),
        });
        std::fs::write(&path, format!("{contents}\n")).expect("write oversized rollout");

        assert!(
            redacted_rollout_attachment(&path, /*filename_override*/ None)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn malformed_rollout_attachment_is_omitted_without_raw_fallback() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let path = temp.path().join("rollout.jsonl");
        std::fs::write(&path, format!(r#"{{"token":"{SECRET}""#)).expect("write malformed rollout");

        assert!(
            redacted_rollout_attachment(&path, /*filename_override*/ None)
                .await
                .is_none()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_sandbox_log_attachment_uses_current_log() {
        let codex_home = tempfile::tempdir().expect("create tempdir");
        let sandbox_dir = codex_windows_sandbox::sandbox_dir(codex_home.path());
        std::fs::create_dir_all(&sandbox_dir).expect("create sandbox dir");
        let sandbox_log_path =
            codex_windows_sandbox::current_log_file_path_for_codex_home(codex_home.path());
        std::fs::write(&sandbox_log_path, "sandbox log").expect("write sandbox log");

        let attachment = windows_sandbox_log_attachment(codex_home.path())
            .map(|attachment| (attachment.path, attachment.attachment_filename_override));

        assert_eq!(
            attachment,
            Some((
                sandbox_log_path,
                Some(WINDOWS_SANDBOX_LOG_ATTACHMENT_FILENAME.to_string())
            ))
        );
    }
}
