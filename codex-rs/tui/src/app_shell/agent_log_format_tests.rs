use super::*;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::McpToolCallResult;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadSource;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::UserInput;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SubAgentSource;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::LegacyAppPathString;
use serde_json::json;
use std::path::Path;
use std::path::PathBuf;

#[test]
fn formats_only_digestible_subagent_activity() {
    let thread = forked_agent_thread();
    let lines = thread_to_agent_log_lines(&thread, RawReasoningVisibility::Hidden)
        .expect("full history should render");
    let rendered = plain_text(&lines);

    for hidden in [
        "Original root prompt",
        "Root answer already visible",
        "Persisted details",
        "Source",
        "raw-first-line",
        "RAW DIFF",
        "rawArgument",
        "rawResult",
    ] {
        assert!(
            !rendered.contains(hidden),
            "agent activity should not contain {hidden:?}:\n{rendered}"
        );
    }
    insta::assert_snapshot!("formatted_agent_activity", rendered);
}

fn forked_agent_thread() -> Thread {
    let parent_thread_id =
        ThreadId::from_string("01900000-0000-7000-8000-000000000001").expect("valid parent id");
    let thread_id = "01900000-0000-7000-8000-000000000002";
    Thread {
        id: thread_id.to_string(),
        extra: None,
        session_id: parent_thread_id.to_string(),
        forked_from_id: Some(parent_thread_id.to_string()),
        parent_thread_id: Some(parent_thread_id.to_string()),
        preview: "Original root prompt".to_string(),
        ephemeral: false,
        history_mode: ThreadHistoryMode::Legacy,
        model_provider: "openai".to_string(),
        created_at: 1,
        updated_at: 2,
        recency_at: Some(2),
        status: ThreadStatus::NotLoaded,
        path: None,
        cwd: AbsolutePathBuf::from_absolute_path_checked("/workspace/better-codex")
            .expect("absolute workspace path"),
        cli_version: "test".to_string(),
        source: SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id,
            depth: 1,
            agent_path: Some(AgentPath::try_from("/root/reviewer").expect("valid agent path")),
            agent_nickname: Some("Hypatia".to_string()),
            agent_role: None,
        }),
        thread_source: Some(ThreadSource::Subagent),
        agent_nickname: Some("Hypatia".to_string()),
        agent_role: None,
        git_info: None,
        name: None,
        turns: vec![
            turn(
                "root-turn",
                vec![
                    user_message("root-prompt", "Original root prompt"),
                    agent_message("root-answer", "Root answer already visible"),
                ],
            ),
            Turn {
                id: "agent-turn".to_string(),
                items: vec![
                    ThreadItem::Reasoning {
                        id: "reasoning".to_string(),
                        summary: vec!["Inspecting the relevant rendering path.".to_string()],
                        content: Vec::new(),
                    },
                    ThreadItem::CommandExecution {
                        id: "command".to_string(),
                        command: "just test -p codex-tui".to_string(),
                        cwd: LegacyAppPathString::from_path(Path::new("/workspace/better-codex")),
                        process_id: None,
                        source: CommandExecutionSource::Agent,
                        status: CommandExecutionStatus::Completed,
                        command_actions: Vec::new(),
                        aggregated_output: Some(
                            [
                                "raw-first-line",
                                "building crate 1",
                                "building crate 2",
                                "running tests",
                                "test agent_log_format ... ok",
                                "test agent_log_view ... ok",
                                "test hydration ... ok",
                                "test navigation ... ok",
                                "test snapshots ... ok",
                                "test transcript ... ok",
                                "1316 passed; 0 failed",
                            ]
                            .join("\n"),
                        ),
                        exit_code: Some(0),
                        duration_ms: Some(1_250),
                    },
                    ThreadItem::FileChange {
                        id: "edits".to_string(),
                        changes: vec![
                            FileUpdateChange {
                                path: "tui/src/agent_log.rs".to_string(),
                                kind: PatchChangeKind::Update { move_path: None },
                                diff: "RAW DIFF SHOULD NOT APPEAR".to_string(),
                            },
                            FileUpdateChange {
                                path: "tui/src/old.rs".to_string(),
                                kind: PatchChangeKind::Update {
                                    move_path: Some(PathBuf::from("tui/src/new.rs")),
                                },
                                diff: "RAW DIFF SHOULD NOT APPEAR".to_string(),
                            },
                        ],
                        status: PatchApplyStatus::Completed,
                    },
                    ThreadItem::McpToolCall {
                        id: "tool".to_string(),
                        server: "github".to_string(),
                        tool: "get_pull_request".to_string(),
                        status: McpToolCallStatus::Completed,
                        arguments: json!({"rawArgument": "hidden"}),
                        app_context: None,
                        mcp_app_resource_uri: None,
                        plugin_id: None,
                        result: Some(Box::new(McpToolCallResult {
                            content: vec![json!({"rawResult": "hidden"})],
                            structured_content: None,
                            meta: None,
                        })),
                        error: None,
                        duration_ms: Some(240),
                    },
                    agent_message(
                        "agent-answer",
                        "The agent log now emphasizes outcomes and concise activity.",
                    ),
                ],
                items_view: TurnItemsView::Full,
                status: TurnStatus::Completed,
                error: None,
                started_at: Some(1),
                completed_at: Some(2),
                duration_ms: Some(1_500),
            },
        ],
    }
}

fn turn(id: &str, items: Vec<ThreadItem>) -> Turn {
    Turn {
        id: id.to_string(),
        items,
        items_view: TurnItemsView::Full,
        status: TurnStatus::Completed,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

fn user_message(id: &str, text: &str) -> ThreadItem {
    ThreadItem::UserMessage {
        id: id.to_string(),
        client_id: None,
        content: vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }],
    }
}

fn agent_message(id: &str, text: &str) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: id.to_string(),
        text: text.to_string(),
        phase: None,
        memory_citation: None,
    }
}

fn plain_text(lines: &[Line<'static>]) -> String {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
