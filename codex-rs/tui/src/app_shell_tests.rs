use super::render::ShellView;
use super::*;
use codex_app_server_protocol::AdditionalNetworkPermissions;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestParams;
use codex_app_server_protocol::PermissionsRequestApprovalParams;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ToolRequestUserInputOption;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_protocol::openai_models::ReasoningEffort;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::LegacyAppPathString;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::json;
use std::path::PathBuf;

#[test]
fn renders_first_stage_shell_snapshot() {
    let shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_scrolled_transcript_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.push_status("first checkpoint");
    shell.push_status("second checkpoint");
    shell.push_status("third checkpoint");
    shell.push_status("fourth checkpoint");
    shell.scroll_transcript_up(4);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 16,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_narrow_shell_snapshot() {
    let shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_workspace_roots_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.runtime_workspace_roots = vec![
        AbsolutePathBuf::from_absolute_path_checked("/workspace/better-codex")
            .expect("absolute path should be valid"),
        AbsolutePathBuf::from_absolute_path_checked("/workspace/better-codex/codex-rs")
            .expect("absolute path should be valid"),
        AbsolutePathBuf::from_absolute_path_checked("/tmp/codex-cache")
            .expect("absolute path should be valid"),
        AbsolutePathBuf::from_absolute_path_checked("/opt/extra-worktree")
            .expect("absolute path should be valid"),
    ];
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 42,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_model_runtime_details_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.reasoning_effort = Some(ReasoningEffort::High);
    shell.service_tier = Some("flex".to_string());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_context_pressure_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.token_usage = TokenUsage {
        input_tokens: 150_000,
        cached_input_tokens: 20_000,
        output_tokens: 40_000,
        reasoning_output_tokens: 12_000,
        total_tokens: 190_000,
    };
    shell.model_context_window = Some(200_000);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_markdown_transcript_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.composer.clear();
    shell.streaming_assistant.clear();
    shell.push_assistant(
        "# Result\n\
        - Render `assistant` text as markdown.\n\
        - Preserve local links like [render.rs](/workspace/better-codex/codex-rs/tui/src/app_shell/render.rs:1).\n\
        \n\
        ```rust\n\
        fn transcript() -> &'static str {\n\
            \"markdown\"\n\
        }\n\
        ```\n\
        \n\
        | Area | Status |\n\
        | --- | --- |\n\
        | code | done |\n\
        | table | done |",
    );
    shell.push_plan(
        "1. Keep transcript rendering width-aware.\n\
        2. Leave selection and copy mode for the next slice.",
    );
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 112, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_pending_approval_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.pending_approval = PendingApproval::from_request(&command_approval_request())
        .expect("approval request should be valid");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_pending_user_input_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.pending_user_input = PendingUserInput::from_request(&tool_user_input_request());
    shell.composer.set_text("2");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_pending_mcp_elicitation_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.pending_elicitation = PendingElicitation::from_request(&mcp_url_elicitation_request());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_decision_audit_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.push_decision_audit("approval", "approved", "Command: cargo test -p codex-tui");
    shell.push_decision_audit("elicitation", "declined", "MCP github: URL request");
    shell.push_decision_audit("tool input", "submitted", "Tool input: environment");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_file_change_detail_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.push_diff(file_change_detail(&sample_file_changes()));
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_tool_progress_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.push_tool("mcp progress: indexed 42 files\npreparing search results");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn transcript_scroll_clamps_to_last_rendered_range() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript_scroll_max.set(10);

    shell.scroll_transcript_up(TRANSCRIPT_PAGE_SCROLL_STEP);
    assert_eq!(shell.transcript_scroll, 8);

    shell.scroll_transcript_up(TRANSCRIPT_PAGE_SCROLL_STEP);
    assert_eq!(shell.transcript_scroll, 10);

    shell.scroll_transcript_down(3);
    assert_eq!(shell.transcript_scroll, 7);

    shell.scroll_transcript_to_top();
    assert_eq!(shell.transcript_scroll, 10);

    shell.scroll_transcript_to_bottom();
    assert_eq!(shell.transcript_scroll, 0);
}

#[test]
fn context_used_percent_handles_unknown_and_baseline_usage() {
    assert_eq!(
        render::context_used_percent(&TokenUsage::default(), None),
        None
    );
    assert_eq!(
        render::context_used_percent(
            &TokenUsage {
                total_tokens: 12_000,
                ..TokenUsage::default()
            },
            Some(200_000),
        ),
        Some(0)
    );
}

#[test]
fn context_used_percent_accounts_for_baseline_reserved_tokens() {
    assert_eq!(
        render::context_used_percent(
            &TokenUsage {
                total_tokens: 190_000,
                ..TokenUsage::default()
            },
            Some(200_000),
        ),
        Some(95)
    );
}

#[test]
fn composer_edits_multiline_text_at_cursor() {
    let mut composer = ComposerState::default();
    composer.insert_str("alpha\nbeta");
    composer.move_left();
    composer.move_left();
    composer.insert_char('X');

    assert_eq!(
        (composer.text().to_string(), composer.cursor_position()),
        ("alpha\nbeXta".to_string(), (1, 3))
    );

    composer.move_up_or_recall_history();
    composer.insert_newline();

    assert_eq!(
        (composer.text().to_string(), composer.cursor_position()),
        ("alp\nha\nbeXta".to_string(), (1, 0))
    );
}

#[test]
fn composer_recalls_submission_history_from_draft() {
    let mut composer = ComposerState::default();
    composer.remember_submission("first");
    composer.remember_submission("second");
    composer.set_text("draft");

    composer.move_up_or_recall_history();
    assert_eq!(composer.text(), "second");

    composer.move_up_or_recall_history();
    assert_eq!(composer.text(), "first");

    composer.move_down_or_recall_history();
    assert_eq!(composer.text(), "second");

    composer.move_down_or_recall_history();
    assert_eq!(composer.text(), "draft");
}

#[test]
fn command_approval_serializes_accept_and_deny() {
    let pending = PendingApproval::from_request(&command_approval_request())
        .expect("approval request should be valid")
        .expect("request should be supported");

    assert_eq!(
        pending
            .result(ApprovalChoice::Approve)
            .expect("approval should serialize"),
        json!({ "decision": "accept" })
    );
    assert_eq!(
        pending
            .result(ApprovalChoice::Deny)
            .expect("denial should serialize"),
        json!({ "decision": "decline" })
    );
}

#[test]
fn permissions_approval_serializes_grant_and_empty_deny() {
    let pending = PendingApproval::from_request(&permissions_approval_request())
        .expect("approval request should be valid")
        .expect("request should be supported");

    assert_eq!(
        pending
            .result(ApprovalChoice::Approve)
            .expect("approval should serialize"),
        json!({
            "permissions": {
                "network": { "enabled": true }
            },
            "scope": "turn"
        })
    );
    assert_eq!(
        pending
            .result(ApprovalChoice::Deny)
            .expect("denial should serialize"),
        json!({
            "permissions": {},
            "scope": "turn"
        })
    );
}

#[test]
fn user_input_serializes_free_form_answer() {
    let mut pending = PendingUserInput::from_request(&tool_free_form_user_input_request())
        .expect("request should be supported");

    assert_eq!(
        pending
            .answer_current("Use my staging API key".to_string())
            .expect("answer should serialize"),
        UserInputAdvance::Complete {
            request_id: RequestId::Integer(44),
            result: json!({
                "answers": {
                    "api_key": {
                        "answers": ["user_note: Use my staging API key"]
                    }
                }
            })
        }
    );
}

#[test]
fn user_input_serializes_option_selection() {
    let mut pending = PendingUserInput::from_request(&tool_user_input_request())
        .expect("request should be supported");

    assert_eq!(
        pending
            .answer_current("2".to_string())
            .expect("answer should serialize"),
        UserInputAdvance::Complete {
            request_id: RequestId::Integer(43),
            result: json!({
                "answers": {
                    "environment": {
                        "answers": ["Staging"]
                    }
                }
            })
        }
    );
}

#[test]
fn mcp_elicitation_serializes_accept_decline_and_cancel() {
    let pending = PendingElicitation::from_request(&mcp_url_elicitation_request())
        .expect("request should be supported");

    assert_eq!(
        pending
            .result(ElicitationChoice::Accept)
            .expect("accept should serialize"),
        json!({
            "action": "accept",
            "content": null,
            "_meta": null
        })
    );
    assert_eq!(
        pending
            .result(ElicitationChoice::Decline)
            .expect("decline should serialize"),
        json!({
            "action": "decline",
            "content": null,
            "_meta": null
        })
    );
    assert_eq!(
        pending
            .result(ElicitationChoice::Cancel)
            .expect("cancel should serialize"),
        json!({
            "action": "cancel",
            "content": null,
            "_meta": null
        })
    );
}

#[test]
fn mcp_elicitation_rejects_rich_form_accept_without_content() {
    let pending = PendingElicitation::from_request(&mcp_rich_elicitation_request())
        .expect("request should be supported");

    assert!(pending.result(ElicitationChoice::Accept).is_err());
    assert_eq!(
        pending
            .result(ElicitationChoice::Decline)
            .expect("decline should serialize"),
        json!({
            "action": "decline",
            "content": null,
            "_meta": null
        })
    );
}

#[test]
fn file_change_detail_caps_file_rows() {
    let changes = (0..10)
        .map(|index| FileUpdateChange {
            path: format!("src/file{index}.rs"),
            kind: PatchChangeKind::Add,
            diff: "+line\n".to_string(),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        file_change_detail(&changes),
        "\
diff 10 files +10 -0
  A src/file0.rs
  A src/file1.rs
  A src/file2.rs
  A src/file3.rs
  A src/file4.rs
  A src/file5.rs
  A src/file6.rs
  A src/file7.rs
  ... 2 more"
    );
}

fn render_shell(shell: &ShellState, area: Rect) -> String {
    let mut buf = Buffer::empty(area);
    ShellView { shell }.render(area, &mut buf);
    buffer_contents(&buf, area)
}

fn command_approval_request() -> ServerRequest {
    ServerRequest::CommandExecutionRequestApproval {
        request_id: RequestId::Integer(41),
        params: CommandExecutionRequestApprovalParams {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "exec-1".to_string(),
            started_at_ms: 0,
            approval_id: None,
            environment_id: None,
            reason: Some("Needs network access".to_string()),
            network_approval_context: None,
            command: Some("cargo test -p codex-tui".to_string()),
            cwd: Some(LegacyAppPathString::from_abs_path(&test_absolute_path(
                "workspace/better-codex",
            ))),
            command_actions: None,
            additional_permissions: None,
            proposed_execpolicy_amendment: None,
            proposed_network_policy_amendments: None,
            available_decisions: None,
        },
    }
}

fn permissions_approval_request() -> ServerRequest {
    ServerRequest::PermissionsRequestApproval {
        request_id: RequestId::Integer(42),
        params: PermissionsRequestApprovalParams {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "permissions-1".to_string(),
            environment_id: None,
            started_at_ms: 0,
            cwd: test_absolute_path("workspace/better-codex"),
            reason: Some("Need package registry access".to_string()),
            permissions: codex_app_server_protocol::RequestPermissionProfile {
                network: Some(AdditionalNetworkPermissions {
                    enabled: Some(true),
                }),
                file_system: None,
            },
        },
    }
}

fn tool_user_input_request() -> ServerRequest {
    ServerRequest::ToolRequestUserInput {
        request_id: RequestId::Integer(43),
        params: ToolRequestUserInputParams {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "tool-input-1".to_string(),
            questions: vec![ToolRequestUserInputQuestion {
                id: "environment".to_string(),
                header: "Environment".to_string(),
                question: "Which environment should the tool use?".to_string(),
                is_other: false,
                is_secret: false,
                options: Some(vec![
                    ToolRequestUserInputOption {
                        label: "Production".to_string(),
                        description: "Use the live service".to_string(),
                    },
                    ToolRequestUserInputOption {
                        label: "Staging".to_string(),
                        description: "Use the staging service".to_string(),
                    },
                ]),
            }],
            auto_resolution_ms: None,
        },
    }
}

fn tool_free_form_user_input_request() -> ServerRequest {
    ServerRequest::ToolRequestUserInput {
        request_id: RequestId::Integer(44),
        params: ToolRequestUserInputParams {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "tool-input-2".to_string(),
            questions: vec![ToolRequestUserInputQuestion {
                id: "api_key".to_string(),
                header: "API key".to_string(),
                question: "Which API key should be used?".to_string(),
                is_other: false,
                is_secret: true,
                options: None,
            }],
            auto_resolution_ms: None,
        },
    }
}

fn mcp_url_elicitation_request() -> ServerRequest {
    ServerRequest::McpServerElicitationRequest {
        request_id: RequestId::Integer(45),
        params: McpServerElicitationRequestParams {
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            server_name: "github".to_string(),
            request: McpServerElicitationRequest::Url {
                meta: None,
                message: "Open the GitHub authorization page?".to_string(),
                url: "https://github.com/login/device".to_string(),
                elicitation_id: "auth-1".to_string(),
            },
        },
    }
}

fn mcp_rich_elicitation_request() -> ServerRequest {
    ServerRequest::McpServerElicitationRequest {
        request_id: RequestId::Integer(46),
        params: McpServerElicitationRequestParams {
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            server_name: "payments".to_string(),
            request: McpServerElicitationRequest::OpenAiForm {
                meta: None,
                message: "Collect billing contact details.".to_string(),
                requested_schema: json!({
                    "type": "object",
                    "properties": {
                        "email": { "type": "string" }
                    }
                }),
            },
        },
    }
}

fn sample_file_changes() -> Vec<FileUpdateChange> {
    vec![
        FileUpdateChange {
            path: "src/app.rs".to_string(),
            kind: PatchChangeKind::Update { move_path: None },
            diff: "@@\n-old\n+new\n+extra\n".to_string(),
        },
        FileUpdateChange {
            path: "src/new.rs".to_string(),
            kind: PatchChangeKind::Add,
            diff: "+created\n".to_string(),
        },
        FileUpdateChange {
            path: "src/old.rs".to_string(),
            kind: PatchChangeKind::Delete,
            diff: "-removed\n".to_string(),
        },
        FileUpdateChange {
            path: "src/from.rs".to_string(),
            kind: PatchChangeKind::Update {
                move_path: Some(PathBuf::from("src/to.rs")),
            },
            diff: "@@\n-left\n+right\n".to_string(),
        },
    ]
}

fn test_absolute_path(tail: &str) -> AbsolutePathBuf {
    let path = if cfg!(windows) {
        PathBuf::from(format!(r"C:\{tail}"))
    } else {
        PathBuf::from(format!("/{tail}"))
    };
    AbsolutePathBuf::try_from(path).expect("test path should be absolute")
}

fn buffer_contents(buf: &Buffer, area: Rect) -> String {
    let mut rows = Vec::new();
    for y in area.y..area.bottom() {
        let mut row = String::new();
        for x in area.x..area.right() {
            row.push_str(buf.cell((x, y)).expect("cell should exist").symbol());
        }
        rows.push(row.trim_end().to_string());
    }
    rows.join("\n")
}

#[test]
fn summarizes_unified_diff_for_dashboard() {
    let diff = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,3 @@
-old
+new
+extra
 unchanged
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1 +1 @@
-left
+right
";

    assert_eq!(
        diff_summary_from_unified_diff(diff),
        DiffSummary {
            files: 2,
            additions: 3,
            removals: 2,
        }
    );
}
