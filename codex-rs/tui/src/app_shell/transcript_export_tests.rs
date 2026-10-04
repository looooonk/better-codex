use super::*;
use pretty_assertions::assert_eq;

#[test]
fn markdown_export_preserves_message_formatting() {
    let thread: Thread = serde_json::from_value(serde_json::json!({
        "id": "01987b77-33b8-76e3-9a7f-a367513be004", "sessionId": "session",
        "preview": "", "ephemeral": false, "modelProvider": "openai", "createdAt": 0,
        "updatedAt": 0, "status": {"type":"idle"}, "cwd": "/workspace", "cliVersion": "0.160.0",
        "source": "cli", "turns": [], "projectId": null
    }))
    .unwrap();
    let item = ThreadItem::AgentMessage {
        id: "reply".to_string(),
        text: "**Done**\n\n```rust\nlet answer = 42;\n```".to_string(),
        phase: None,
        memory_citation: None,
        delivery: None,
        questions: None,
    };
    let mut markdown = String::new();
    append_item(&mut markdown, &thread, &item).unwrap();
    assert_eq!(
        markdown,
        "## Assistant\n\n**Done**\n\n```rust\nlet answer = 42;\n```\n\n"
    );
}

struct Reader {
    pages: std::sync::Mutex<
        std::collections::VecDeque<Result<ThreadTimelineListResponse, TypedRequestError>>,
    >,
    history: Vec<codex_app_server_protocol::Turn>,
}

impl HistoryReader for Reader {
    async fn metadata(
        &self,
        _: ThreadId,
    ) -> Result<codex_app_server_protocol::ThreadReadResponse, TypedRequestError> {
        Ok(serde_json::from_value(serde_json::json!({"thread": {
            "id": "01987b77-33b8-76e3-9a7f-a367513be004", "sessionId": "session",
            "preview": "", "ephemeral": false, "modelProvider": "openai", "createdAt": 0,
            "updatedAt": 0, "status": {"type":"idle"}, "cwd": "/workspace", "cliVersion": "0.160.0",
            "source": "cli", "turns": [], "projectId": null
        }}))
        .unwrap())
    }

    async fn turns(
        &self,
        _: codex_app_server_protocol::ThreadTurnsListParams,
    ) -> Result<codex_app_server_protocol::ThreadTurnsListResponse, TypedRequestError> {
        Ok(codex_app_server_protocol::ThreadTurnsListResponse {
            data: self.history.clone(),
            next_cursor: None,
            backwards_cursor: None,
        })
    }
}

impl ExportReader for Reader {
    async fn timeline(
        &self,
        _: ThreadTimelineListParams,
    ) -> Result<ThreadTimelineListResponse, TypedRequestError> {
        self.pages
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected timeline request")
    }
}

fn page(text: &str, cursor: Option<&str>) -> Result<ThreadTimelineListResponse, TypedRequestError> {
    Ok(ThreadTimelineListResponse {
        data: vec![ThreadTimelineEntry::Item {
            position: 1,
            turn_id: "turn".to_string(),
            item: Box::new(ThreadItem::AgentMessage {
                id: text.to_string(),
                text: text.to_string(),
                phase: None,
                memory_citation: None,
                delivery: None,
                questions: None,
            }),
        }],
        next_cursor: cursor.map(str::to_string),
        active_realtime_session_at_page_start: None,
    })
}

#[tokio::test]
async fn export_orders_pages_and_never_replaces_an_existing_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("conversation.md");
    for attempt in 0..2 {
        let reader = Reader {
            pages: std::sync::Mutex::new(
                [page("New reply", Some("older")), page("Old reply", None)].into(),
            ),
            history: vec![],
        };
        let result = export(reader, ThreadId::new(), path.clone()).await;
        assert_eq!(result.is_ok(), attempt == 0);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# Better Codex conversation\n\n## Assistant\n\nOld reply\n\n## Assistant\n\nNew reply\n\n"
        );
    }
}

#[tokio::test]
async fn export_does_not_create_a_partial_file_after_repeated_cursor() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("conversation.md");
    let reader = Reader {
        pages: std::sync::Mutex::new(
            [page("one", Some("again")), page("two", Some("again"))].into(),
        ),
        history: vec![],
    };
    assert!(
        export(reader, ThreadId::new(), path.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("repeated export cursor")
    );
    assert!(!path.exists());
}

#[tokio::test]
async fn older_server_export_uses_full_turn_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("conversation.md");
    let items = page("Legacy reply", None)
        .unwrap()
        .data
        .into_iter()
        .filter_map(|entry| match entry {
            ThreadTimelineEntry::Item { item, .. } => Some(*item),
            _ => None,
        })
        .collect();
    let reader = Reader {
        pages: std::sync::Mutex::new(
            [Err(TypedRequestError::Server {
                method: "thread/timeline/list".to_string(),
                source: codex_app_server_protocol::JSONRPCErrorError {
                    code: -32601,
                    message: "method not found".to_string(),
                    data: None,
                },
            })]
            .into(),
        ),
        history: vec![codex_app_server_protocol::Turn {
            id: "turn".to_string(),
            items,
            status: codex_app_server_protocol::TurnStatus::Completed,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            items_view: Default::default(),
        }],
    };
    export(reader, ThreadId::new(), path.clone()).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "# Better Codex conversation\n\n## Assistant\n\nLegacy reply\n\n"
    );
}

#[tokio::test]
async fn export_preserves_tool_details_attachments_and_voice() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("conversation.md");
    let entries = [
        serde_json::json!({"type": "userMessage", "id": "user", "content": [
            {"type": "text", "text": "Inspect this image"},
            {"type": "localImage", "path": "/workspace/image one.png"},
            {"type": "audio", "url": "https://example.com/audio.wav"}
        ]}),
        serde_json::json!({"type": "mcpToolCall", "id": "mcp", "server": "search", "tool": "find", "status": "completed",
            "arguments": {"query": "complete result"}, "result": {"content": [{"type":"text", "text":"full tool response"}], "structuredContent": {"count": 7}}}),
        serde_json::json!({"type": "fileChange", "id": "patch", "status": "completed", "changes": [
            {"path": "src/main.rs", "kind": {"type":"update"}, "diff": "-old\n+new"}
        ]}),
        serde_json::json!({"type": "commandExecution", "id": "command", "command": "cat README.md", "cwd": "/workspace", "status": "completed", "commandActions": [],
            "aggregatedOutput": "# Heading\n```rust\nlet value = 1;\n```", "exitCode": 0}),
        serde_json::json!({"type": "dynamicToolCall", "id": "dynamic", "tool": "paint", "arguments": {"color":"purple"}, "status": "completed", "success": true,
            "contentItems": [{"type": "inputText", "text":"painted"}, {"type":"inputImage", "imageUrl":"https://example.com/image.png"}]}),
    ].into_iter().map(|value| ThreadTimelineEntry::Item {
        position: 1, turn_id: "turn".to_string(), item: Box::new(serde_json::from_value(value).unwrap()),
    }).chain([ThreadTimelineEntry::Realtime {
        position: 2, item: codex_app_server_protocol::ThreadRealtimeItem {
            id: "voice".to_string(), realtime_session_id: "realtime".to_string(),
            content: ThreadRealtimeItemContent::TranscriptSegment { role: ThreadRealtimeTranscriptRole::Assistant, text: "Everything is ready.".to_string() },
        },
    }]).collect();
    let reader = Reader {
        pages: std::sync::Mutex::new(
            [Ok(ThreadTimelineListResponse {
                data: entries,
                next_cursor: None,
                active_realtime_session_at_page_start: None,
            })]
            .into(),
        ),
        history: vec![],
    };
    export(reader, ThreadId::new(), path.clone()).await.unwrap();
    insta::assert_snapshot!(std::fs::read_to_string(path).unwrap());
}
