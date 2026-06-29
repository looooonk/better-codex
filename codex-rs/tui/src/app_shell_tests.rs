use super::render::ShellView;
use super::render::TranscriptScrollbarMetrics;
use super::*;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::AdditionalNetworkPermissions;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestParams;
use codex_app_server_protocol::PermissionsRequestApprovalParams;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadGoalClearedNotification;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadGoalUpdatedNotification;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadStartSource;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::ToolRequestUserInputOption;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::TurnSteerResponse;
use codex_app_server_protocol::UserInput as ApiUserInput;
use codex_protocol::openai_models::ReasoningEffort;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::LegacyAppPathString;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

#[test]
fn renders_first_stage_shell_snapshot() {
    let shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_native_session_list_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    shell.session_list.replace_threads(vec![
        thread_fixture(
            test_thread_id("01900000-0000-7000-8000-000000000501"),
            Some("Refactor dashboard navigation"),
            "Add native routes for sessions and workspace",
        ),
        thread_fixture(
            test_thread_id("01900000-0000-7000-8000-000000000502"),
            None,
            "Investigate approval rendering regression",
        ),
    ]);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 32,
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
fn renders_short_shell_snapshot() {
    let shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 12,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_workspace_roots_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Workspace;
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
fn renders_workspace_git_status_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Workspace;
    shell.workspace_git_status = Some(WorkspaceGitStatus {
        branch: Some("feature/app-shell-dashboard".to_string()),
        changes: workspace::WorkspaceChangeSummary {
            added: 2,
            modified: 5,
            deleted: 1,
            renamed: 1,
            conflicted: 1,
            untracked: 3,
        },
    });
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 48,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_workspace_route_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Workspace;
    shell.workspace_git_status = Some(WorkspaceGitStatus {
        branch: Some("feature/app-shell-dashboard".to_string()),
        changes: workspace::WorkspaceChangeSummary {
            added: 2,
            modified: 5,
            deleted: 1,
            renamed: 1,
            conflicted: 1,
            untracked: 3,
        },
    });
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 34,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_model_runtime_details_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Settings;
    shell.reasoning_effort = Some(ReasoningEffort::High);
    shell.service_tier = Some("flex".to_string());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_rate_limits_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Settings;
    shell.rate_limits = vec![
        codex_app_server_protocol::RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: Some("Codex".to_string()),
            primary: Some(codex_app_server_protocol::RateLimitWindow {
                used_percent: 82,
                window_duration_mins: Some(300),
                resets_at: Some(1_900_000_000),
            }),
            secondary: Some(codex_app_server_protocol::RateLimitWindow {
                used_percent: 18,
                window_duration_mins: Some(10_080),
                resets_at: None,
            }),
            credits: Some(codex_app_server_protocol::CreditsSnapshot {
                has_credits: true,
                unlimited: false,
                balance: Some("$12.34".to_string()),
            }),
            individual_limit: Some(codex_app_server_protocol::SpendControlLimitSnapshot {
                limit: "$100.00".to_string(),
                used: "$25.00".to_string(),
                remaining_percent: 75,
                resets_at: 1_900_000_000,
            }),
            plan_type: None,
            rate_limit_reached_type: None,
        },
        codex_app_server_protocol::RateLimitSnapshot {
            limit_id: Some("secondary".to_string()),
            limit_name: Some("Background".to_string()),
            primary: Some(codex_app_server_protocol::RateLimitWindow {
                used_percent: 95,
                window_duration_mins: Some(60),
                resets_at: None,
            }),
            secondary: None,
            credits: None,
            individual_limit: None,
            plan_type: None,
            rate_limit_reached_type: Some(
                codex_app_server_protocol::RateLimitReachedType::RateLimitReached,
            ),
        },
    ];
    shell.rate_limit_reset_credits = Some(2);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 42,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_context_pressure_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Settings;
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
fn renders_active_turn_status_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = Some("turn-active-1234567890".to_string());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_goal_progress_in_dashboard_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.active_goal = Some(test_thread_goal(
        &shell.thread_id,
        ThreadGoalStatus::Active,
        "Complete the unchecked PLAN.md dashboard progress item",
    ));
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 34,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_active_turn_key_hints_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = Some("turn-active-1234567890".to_string());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 44,
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
fn renders_transcript_selection_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript_selection = Some(2);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_command_palette_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.open_command_palette();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 30,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn command_palette_lists_common_actions() {
    let shell = ShellState::snapshot_fixture();
    let entries = shell.command_palette_entries();

    assert_eq!(
        entries
            .iter()
            .map(|entry| (entry.action, entry.enabled))
            .collect::<Vec<_>>(),
        vec![
            (CommandPaletteAction::CopyTranscript, true),
            (CommandPaletteAction::ClearTranscript, true),
            (CommandPaletteAction::SelectLatestTranscript, true),
            (CommandPaletteAction::ScrollTranscriptTop, true),
            (CommandPaletteAction::ScrollTranscriptBottom, true),
            (CommandPaletteAction::InterruptTurn, false),
            (CommandPaletteAction::SwitchModel, false),
            (CommandPaletteAction::ChangePermissions, false),
            (CommandPaletteAction::ResumeThread, false),
            (CommandPaletteAction::ForkThread, false),
            (CommandPaletteAction::CompactContext, false),
        ]
    );
}

#[test]
fn command_palette_clear_resets_visible_transcript() {
    let mut shell = ShellState::snapshot_fixture();
    shell.streaming_assistant = "streaming".to_string();
    shell.streaming_plan = "plan".to_string();
    shell.select_latest_transcript_item();

    shell.clear_visible_transcript();

    assert_eq!(
        shell.transcript.iter().cloned().collect::<Vec<_>>(),
        vec![TranscriptLine::new(
            TranscriptKind::System,
            "visible transcript cleared"
        )]
    );
    assert_eq!(shell.streaming_assistant, "");
    assert_eq!(shell.streaming_plan, "");
    assert_eq!(shell.transcript_scroll, 0);
    assert_eq!(shell.transcript_selection, None);
}

#[test]
fn dashboard_route_key_mapping_covers_native_routes() {
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL)),
        Some(DashboardRoute::Sessions)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL)),
        Some(DashboardRoute::Workspace)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::CONTROL)),
        Some(DashboardRoute::Settings)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::CONTROL)),
        Some(DashboardRoute::Help)
    );
    assert_eq!(dashboard_route_from_key(key_char('1')), None);
}

#[test]
fn dashboard_route_changes_are_persisted() {
    let codex_home = tempfile::tempdir().expect("create temp codex home");
    let mut shell = ShellState {
        codex_home: codex_home.path().to_path_buf(),
        ..ShellState::snapshot_fixture()
    };

    shell.set_dashboard_route(DashboardRoute::Settings);

    assert_eq!(
        AppShellRouteState::load(codex_home.path()),
        AppShellRouteState {
            route: DashboardRoute::Settings
        }
    );
}

#[test]
fn transcript_selection_moves_between_items() {
    let mut shell = ShellState::snapshot_fixture();
    shell.select_latest_transcript_item();

    assert_eq!(
        shell.selected_transcript_copy_text(),
        Some((TranscriptKind::Diff, "diff 3 files +128 -24"))
    );

    shell.move_transcript_selection_up(2);

    assert_eq!(
        shell.selected_transcript_copy_text(),
        Some((
            TranscriptKind::Plan,
            "1. Build shell\n2. Wire transcript\n3. Render dashboard"
        ))
    );

    shell.move_transcript_selection_down(1);

    assert_eq!(
        shell.selected_transcript_copy_text(),
        Some((TranscriptKind::Tool, "exec just test -p codex-tui"))
    );
}

#[test]
fn copies_selected_transcript_item() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript_selection = Some(1);
    let mut copied = None;

    shell.copy_selected_transcript_with(|text| {
        copied = Some(text.to_string());
        Ok(None)
    });

    assert_eq!(
        copied,
        Some("Create a divergent standalone TUI.".to_string())
    );
    assert_eq!(
        shell.transcript.back(),
        Some(&TranscriptLine::new(
            TranscriptKind::Status,
            "copied you transcript item"
        ))
    );
}

#[test]
fn copies_latest_assistant_without_selection() {
    let mut shell = ShellState::snapshot_fixture();
    let mut copied = None;

    shell.copy_selected_transcript_with(|text| {
        copied = Some(text.to_string());
        Ok(None)
    });

    assert_eq!(
        copied,
        Some("Started a fullscreen app shell backed by app-server turns.".to_string())
    );
    assert_eq!(
        shell.transcript.back(),
        Some(&TranscriptLine::new(
            TranscriptKind::Status,
            "copied codex transcript item"
        ))
    );
}

#[test]
fn thread_goal_notifications_update_dashboard_state() {
    let mut shell = ShellState::snapshot_fixture();
    let goal = test_thread_goal(
        &shell.thread_id,
        ThreadGoalStatus::Paused,
        "Keep the plan visible in the dashboard",
    );

    shell.handle_notification(ServerNotification::ThreadGoalUpdated(
        ThreadGoalUpdatedNotification {
            thread_id: shell.thread_id.to_string(),
            turn_id: Some("turn-1".to_string()),
            goal: goal.clone(),
        },
    ));

    assert_eq!(shell.active_goal, Some(goal));

    shell.handle_notification(ServerNotification::ThreadGoalCleared(
        ThreadGoalClearedNotification {
            thread_id: shell.thread_id.to_string(),
        },
    ));

    assert_eq!(shell.active_goal, None);
}

#[test]
fn duplicate_completed_user_message_is_suppressed() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.push_user("hello from user");

    shell.ingest_completed_item(ThreadItem::UserMessage {
        id: "user-1".to_string(),
        client_id: None,
        content: vec![UserInput::Text {
            text: "hello from user".to_string(),
            text_elements: Vec::new(),
        }],
    });

    assert_eq!(
        shell.transcript.iter().cloned().collect::<Vec<_>>(),
        vec![TranscriptLine::new(TranscriptKind::User, "hello from user")]
    );
}

#[test]
fn completed_agent_message_replaces_matching_stream() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant = "hello from codex".to_string();

    shell.ingest_completed_item(ThreadItem::AgentMessage {
        id: "agent-1".to_string(),
        text: "hello from codex".to_string(),
        phase: None,
        memory_citation: None,
    });
    shell.finish_streaming_assistant();

    assert_eq!(shell.streaming_assistant, "");
    assert_eq!(
        shell.transcript.iter().cloned().collect::<Vec<_>>(),
        vec![TranscriptLine::new(
            TranscriptKind::Assistant,
            "hello from codex"
        )]
    );
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
fn approval_action_keys_cover_full_keyboard_flow() {
    assert_eq!(
        approval_action_from_key(key_char('a')),
        Some(ApprovalAction::Choose(ApprovalChoice::Approve))
    );
    assert_eq!(
        approval_action_from_key(key_char('d')),
        Some(ApprovalAction::Choose(ApprovalChoice::Deny))
    );
    assert_eq!(
        approval_action_from_key(key_char('e')),
        Some(ApprovalAction::Edit)
    );
    assert_eq!(
        approval_action_from_key(key_char('?')),
        Some(ApprovalAction::Explain)
    );
    assert_eq!(
        approval_action_from_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        None
    );
}

#[test]
fn approval_explain_keeps_request_pending_and_writes_audit() {
    let mut shell = ShellState::snapshot_fixture();
    shell.pending_approval = PendingApproval::from_request(&command_approval_request())
        .expect("approval request should be valid");

    shell.explain_pending_approval();

    assert!(shell.pending_approval.is_some());
    assert_eq!(
        shell.transcript.back(),
        Some(&TranscriptLine::new(
            TranscriptKind::Audit,
            "approval explained: Run command: cargo test -p codex-tui - Needs network access - /workspace/better-codex",
        ))
    );
}

#[test]
fn approval_edit_prompt_preserves_existing_composer_draft() {
    let mut shell = ShellState::snapshot_fixture();

    shell.seed_composer_with_edit_prompt("Revise and retry this command:\njust test".to_string());

    assert_eq!(
        shell.composer.text(),
        "Summarize the new shell architecture\n\nRevise and retry this command:\njust test"
    );
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
fn renders_activity_dashboard_panels_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.pending_approval = PendingApproval::from_request(&command_approval_request())
        .expect("approval request should be valid");
    shell.pending_user_input = PendingUserInput::from_request(&tool_user_input_request());
    shell.active_turn_id = Some("turn-background-1234567890".to_string());
    shell.streaming_plan = "1. Route activity into dashboard panels".to_string();
    shell.workspace_status_refresh_due = true;
    shell.subagent_activity = VecDeque::from([
        ToolActivity {
            id: "agent-1".to_string(),
            title: "agent SpawnAgent: 1 targets".to_string(),
            status: "in progress".to_string(),
        },
        ToolActivity {
            id: "agent-2".to_string(),
            title: "subagent Started: review-agent".to_string(),
            status: "active".to_string(),
        },
    ]);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 120, /*height*/ 54,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn narrow_dashboard_overlay_prioritizes_live_activity() {
    let mut shell = ShellState::snapshot_fixture();
    shell.pending_approval = PendingApproval::from_request(&command_approval_request())
        .expect("approval request should be valid");
    shell.workspace_status_refresh_due = true;
    shell.subagent_activity = VecDeque::from([ToolActivity {
        id: "agent-1".to_string(),
        title: "subagent Started: review-agent".to_string(),
        status: "active".to_string(),
    }]);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 40,
    );

    let rendered = render_shell(&shell, area);

    assert!(rendered.contains("Approvals approval Run command: cargo test"));
    assert!(rendered.contains("Background workspace refresh queued"));
    assert!(rendered.contains("Tools in progress exec just test"));
    assert!(rendered.contains("Subagents"));
}

#[test]
fn subagent_items_route_to_subagent_activity() {
    let mut shell = ShellState::snapshot_fixture();
    shell.tool_activity.clear();
    shell.subagent_activity.clear();
    let thread_id = shell.thread_id.to_string();

    shell.handle_notification(ServerNotification::ItemStarted(ItemStartedNotification {
        thread_id,
        turn_id: "turn-1".to_string(),
        started_at_ms: 0,
        item: ThreadItem::CollabAgentToolCall {
            id: "agent-tool-1".to_string(),
            tool: CollabAgentTool::SpawnAgent,
            status: CollabAgentToolCallStatus::InProgress,
            sender_thread_id: "parent-thread".to_string(),
            receiver_thread_ids: vec!["agent-thread".to_string()],
            prompt: Some("Inspect dashboard activity.".to_string()),
            model: Some("gpt-5-codex".to_string()),
            reasoning_effort: None,
            agents_states: Default::default(),
        },
    }));

    assert_eq!(shell.tool_activity, VecDeque::new());
    assert_eq!(
        shell.subagent_activity,
        VecDeque::from([ToolActivity {
            id: "agent-tool-1".to_string(),
            title: "agent SpawnAgent".to_string(),
            status: "in progress".to_string(),
        }])
    );
}

#[test]
fn transcript_newlines_render_as_single_row_breaks() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.composer.clear();
    shell.streaming_assistant.clear();
    shell.push_assistant("- first result\n- second result");
    shell.push_output("line one\nline two\nline three");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 24,
    );

    let rendered = render_shell(&shell, area);

    assert_adjacent_rows(&rendered, "first result", "second result");
    assert_adjacent_rows(&rendered, "line one", "line two");
    assert_adjacent_rows(&rendered, "line two", "line three");
}

#[test]
fn dashboard_uses_available_width_for_long_values() {
    let mut shell = ShellState::snapshot_fixture();
    shell.model = "gpt-5-codex-dashboard-detail".to_string();
    shell.cwd = "/workspace/better-codex/codex-rs/tui".to_string();
    shell.workspace_git_status = Some(WorkspaceGitStatus {
        branch: Some("feature/dashboard-width-budget".to_string()),
        changes: workspace::WorkspaceChangeSummary::default(),
    });
    shell.tool_activity = VecDeque::from([ToolActivity {
        id: "tool-long".to_string(),
        title: "exec just test -p codex-tui app_shell_tests".to_string(),
        status: "completed".to_string(),
    }]);
    shell.dashboard_route = DashboardRoute::Settings;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 190, /*height*/ 36,
    );

    let rendered = render_shell(&shell, area);

    assert!(rendered.contains("gpt-5-codex-dashboard-detail"));
    shell.dashboard_route = DashboardRoute::Workspace;
    let rendered = render_shell(&shell, area);

    assert!(rendered.contains("/workspace/better-codex/codex-rs/tui"));
    assert!(rendered.contains("feature/dashboard-width-budget"));
    assert!(rendered.contains("exec just test -p codex-tui app_shell_tests"));
}

#[test]
fn dashboard_renders_large_numbers_with_commas() {
    let mut shell = ShellState::snapshot_fixture();
    shell.token_usage = TokenUsage {
        input_tokens: 1_234_567,
        cached_input_tokens: 100_000,
        output_tokens: 234_567,
        reasoning_output_tokens: 12_345,
        total_tokens: 1_469_134,
    };
    shell.model_context_window = Some(2_000_000);
    shell.latest_diff = Some(DiffSummary {
        files: 1_234,
        additions: 56_789,
        removals: 10_011,
    });
    shell.workspace_git_status = Some(WorkspaceGitStatus {
        branch: Some("numbers".to_string()),
        changes: workspace::WorkspaceChangeSummary {
            added: 1_000,
            modified: 2_000,
            deleted: 3_000,
            renamed: 4_000,
            conflicted: 5_000,
            untracked: 6_000,
        },
    });
    shell.dashboard_route = DashboardRoute::Settings;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 130, /*height*/ 48,
    );

    let rendered = render_shell(&shell, area);

    assert!(rendered.contains("total 1,469,134"));
    assert!(rendered.contains("input 1,234,567"));
    assert!(rendered.contains("output 234,567"));
    assert!(rendered.contains("context 2,000,000"));
    shell.dashboard_route = DashboardRoute::Workspace;
    let rendered = render_shell(&shell, area);

    assert!(rendered.contains("1,234 files +56,789 -10,011"));
    assert!(rendered.contains("changes 21,000 files"));
    assert!(rendered.contains("added 1,000"));
    assert!(rendered.contains("untracked 6,000"));
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
fn transcript_selection_page_keys_scroll_without_changing_selection() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript_selection = Some(3);
    shell.transcript_scroll_max.set(20);

    assert_eq!(
        shell.handle_transcript_selection_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)),
        Some(false)
    );
    assert_eq!(shell.transcript_selection, Some(3));
    assert_eq!(shell.transcript_scroll, TRANSCRIPT_PAGE_SCROLL_STEP);

    assert_eq!(
        shell.handle_transcript_selection_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
        Some(false)
    );
    assert_eq!(shell.transcript_selection, Some(3));
    assert_eq!(shell.transcript_scroll, 0);
}

#[test]
fn transcript_scrollbar_metrics_tracks_visible_range() {
    assert_eq!(
        render::transcript_scrollbar_metrics(
            /*total_lines*/ 40, /*visible_count*/ 10, /*visible_from*/ 0,
            /*min_thumb_height*/ 2
        ),
        Some(TranscriptScrollbarMetrics {
            thumb_top: 0,
            thumb_height: 3,
        })
    );
    assert_eq!(
        render::transcript_scrollbar_metrics(
            /*total_lines*/ 40, /*visible_count*/ 10, /*visible_from*/ 30,
            /*min_thumb_height*/ 2
        ),
        Some(TranscriptScrollbarMetrics {
            thumb_top: 7,
            thumb_height: 3,
        })
    );
}

#[test]
fn transcript_scrollbar_metrics_uses_minimum_thumb_height() {
    assert_eq!(
        render::transcript_scrollbar_metrics(
            /*total_lines*/ 1_000, /*visible_count*/ 10, /*visible_from*/ 500,
            /*min_thumb_height*/ 2
        ),
        Some(TranscriptScrollbarMetrics {
            thumb_top: 4,
            thumb_height: 2,
        })
    );
    assert_eq!(
        render::transcript_scrollbar_metrics(
            /*total_lines*/ 8, /*visible_count*/ 10, /*visible_from*/ 0,
            /*min_thumb_height*/ 2
        ),
        None
    );
}

#[test]
fn context_used_percent_handles_unknown_and_baseline_usage() {
    assert_eq!(
        dashboard::context_used_percent(&TokenUsage::default(), None),
        None
    );
    assert_eq!(
        dashboard::context_used_percent(
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
        dashboard::context_used_percent(
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
fn command_approval_exposes_edit_prompt_and_explanation() {
    let pending = PendingApproval::from_request(&command_approval_request())
        .expect("approval request should be valid")
        .expect("request should be supported");

    assert_eq!(
        (pending.edit_prompt().to_string(), pending.explanation(),),
        (
            "Revise and retry this command:\ncargo test -p codex-tui".to_string(),
            "Run command: cargo test -p codex-tui - Needs network access - /workspace/better-codex"
                .to_string(),
        )
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

fn assert_adjacent_rows(rendered: &str, first: &str, second: &str) {
    let rows = rendered.lines().collect::<Vec<_>>();
    let first_index = rows
        .iter()
        .position(|row| row.contains(first))
        .unwrap_or_else(|| panic!("missing rendered row containing {first:?}"));
    let second_index = rows
        .iter()
        .position(|row| row.contains(second))
        .unwrap_or_else(|| panic!("missing rendered row containing {second:?}"));

    assert_eq!(second_index, first_index + 1);
}

fn key_char(ch: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty())
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

fn test_thread_goal(thread_id: &ThreadId, status: ThreadGoalStatus, objective: &str) -> ThreadGoal {
    ThreadGoal {
        thread_id: thread_id.to_string(),
        objective: objective.to_string(),
        status,
        token_budget: Some(50_000),
        tokens_used: 12_345,
        time_used_seconds: 90,
        created_at: 1_900_000_000,
        updated_at: 1_900_000_090,
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

#[tokio::test]
async fn start_resume_and_fork_route_through_app_shell_backend() {
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let resume_id = test_thread_id("01900000-0000-7000-8000-000000000101");
    let fork_id = test_thread_id("01900000-0000-7000-8000-000000000102");

    let started = start_selected_session(&mut backend, &config, SessionSelection::StartFresh).await;
    let resumed = start_selected_session(
        &mut backend,
        &config,
        SessionSelection::Resume(crate::resume_picker::SessionTarget {
            path: Some(PathBuf::from("/workspace/resume")),
            thread_id: resume_id,
        }),
    )
    .await;
    let forked = start_selected_session(
        &mut backend,
        &config,
        SessionSelection::Fork(crate::resume_picker::SessionTarget {
            path: Some(PathBuf::from("/workspace/fork")),
            thread_id: fork_id,
        }),
    )
    .await;

    assert_eq!(
        started.expect("start should succeed").session.thread_name,
        Some("started".to_string())
    );
    assert_eq!(
        resumed.expect("resume should succeed").session.thread_id,
        resume_id
    );
    assert_eq!(
        forked.expect("fork should succeed").session.forked_from_id,
        Some(fork_id)
    );
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::Start(Some(ThreadStartSource::Startup)),
            RecordedBackendCall::Resume(resume_id),
            RecordedBackendCall::Fork(fork_id),
        ]
    );
}

#[tokio::test]
async fn native_session_list_search_archive_delete_and_rename() {
    let config = test_config().await;
    let session_id = test_thread_id("01900000-0000-7000-8000-000000000301");
    let other_id = test_thread_id("01900000-0000-7000-8000-000000000302");
    let mut shell = ShellState::snapshot_fixture();
    shell.thread_id = session_id;
    shell.session_list.focused = true;
    let mut backend = RecordingBackend::with_threads(vec![
        thread_fixture(session_id, Some("current"), "current preview"),
        thread_fixture(other_id, Some("feature search"), "other preview"),
    ]);

    shell.refresh_session_list(&mut backend).await;
    shell
        .handle_session_list_key(key_char('/'), &config, &mut backend)
        .await
        .expect("search mode should start");
    shell
        .handle_session_list_key(key_char('f'), &config, &mut backend)
        .await
        .expect("search should filter");
    shell
        .handle_session_list_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("search should finish");
    shell
        .handle_session_list_key(key_char('a'), &config, &mut backend)
        .await
        .expect("archive should resolve");
    shell
        .handle_session_list_key(key_char('v'), &config, &mut backend)
        .await
        .expect("archived view should load");
    shell
        .handle_session_list_key(key_char('u'), &config, &mut backend)
        .await
        .expect("unarchive should resolve");
    shell
        .handle_session_list_key(key_char('v'), &config, &mut backend)
        .await
        .expect("active view should reload");
    shell
        .handle_session_list_key(key_char('n'), &config, &mut backend)
        .await
        .expect("rename should start");
    shell
        .handle_session_list_key(key_char('!'), &config, &mut backend)
        .await
        .expect("rename should edit");
    shell
        .handle_session_list_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("rename should resolve");
    shell
        .handle_session_list_key(key_char('d'), &config, &mut backend)
        .await
        .expect("delete should resolve");

    let calls = backend.calls();
    assert!(calls.contains(&RecordedBackendCall::ThreadList {
        archived: Some(false),
        search_term: Some("f".to_string()),
    }));
    assert!(calls.contains(&RecordedBackendCall::Archive(other_id)));
    assert!(calls.contains(&RecordedBackendCall::Unarchive(other_id)));
    assert!(calls.contains(&RecordedBackendCall::SetName {
        thread_id: other_id,
        name: "feature search!".to_string(),
    }));
    assert!(calls.contains(&RecordedBackendCall::Delete(other_id)));
}

#[tokio::test]
async fn native_session_list_resume_and_fork_switch_shell_thread() {
    let config = test_config().await;
    let resume_id = test_thread_id("01900000-0000-7000-8000-000000000401");
    let fork_id = test_thread_id("01900000-0000-7000-8000-000000000402");
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    let mut backend = RecordingBackend::with_threads(vec![
        thread_fixture(resume_id, Some("resume target"), "resume preview"),
        thread_fixture(fork_id, Some("fork target"), "fork preview"),
    ]);

    shell.refresh_session_list(&mut backend).await;
    shell
        .handle_session_list_key(key_char('r'), &config, &mut backend)
        .await
        .expect("resume should resolve");
    assert_eq!(shell.thread_id, resume_id);

    shell.refresh_session_list(&mut backend).await;
    shell.session_list.move_selection_down();
    shell
        .handle_session_list_key(key_char('f'), &config, &mut backend)
        .await
        .expect("fork should resolve");
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
            },
            RecordedBackendCall::Resume(resume_id),
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
            },
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
            },
            RecordedBackendCall::Fork(fork_id),
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
            },
        ]
    );
    assert_eq!(
        shell.thread_name,
        Some("forked".to_string()),
        "fork should replace the active shell session"
    );
}

#[tokio::test]
async fn turn_streaming_approval_interrupt_disconnect_and_shutdown_are_covered() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.streaming_plan.clear();
    shell.active_turn_id = None;
    let mut backend = RecordingBackend::default();
    let workspace_runner = NoopWorkspaceRunner;

    shell
        .submit_prompt(&mut backend, "hello app shell".to_string())
        .await
        .expect("turn submit should succeed");
    shell
        .handle_app_server_event(
            &mut backend,
            &workspace_runner,
            AppServerEvent::ServerNotification(ServerNotification::AgentMessageDelta(
                codex_app_server_protocol::AgentMessageDeltaNotification {
                    thread_id: shell.thread_id.to_string(),
                    turn_id: "turn-submit".to_string(),
                    item_id: "assistant-1".to_string(),
                    delta: "streamed ".to_string(),
                },
            )),
        )
        .await
        .expect("assistant delta should be handled");
    shell
        .handle_app_server_event(
            &mut backend,
            &workspace_runner,
            AppServerEvent::ServerNotification(ServerNotification::TurnCompleted(
                codex_app_server_protocol::TurnCompletedNotification {
                    thread_id: shell.thread_id.to_string(),
                    turn: test_turn("turn-submit", TurnStatus::Completed),
                },
            )),
        )
        .await
        .expect("turn completion should be handled");

    shell
        .handle_app_server_event(
            &mut backend,
            &workspace_runner,
            AppServerEvent::ServerRequest(command_approval_request()),
        )
        .await
        .expect("approval request should be handled");
    shell
        .resolve_pending_approval(&mut backend, ApprovalChoice::Approve)
        .await
        .expect("approval should resolve");

    shell.active_turn_id = Some("turn-interrupt".to_string());
    shell
        .interrupt_active_turn(&mut backend)
        .await
        .expect("interrupt should resolve");
    shell
        .handle_app_server_event(
            &mut backend,
            &workspace_runner,
            AppServerEvent::Disconnected {
                message: "backend closed".to_string(),
            },
        )
        .await
        .expect("disconnect should be handled");

    backend
        .unsubscribe_thread(shell.thread_id)
        .await
        .expect("unsubscribe should be recorded");
    let call_log = backend.call_log();
    super::backend::shutdown_app_shell_backend(backend)
        .await
        .expect("shutdown should be recorded");

    let calls = call_log.lock().expect("call log should lock").clone();
    assert!(calls.iter().any(|call| {
        matches!(
            call,
            RecordedBackendCall::TurnStart {
                prompt,
                thread_id,
                ..
            } if prompt == "hello app shell" && *thread_id == shell.thread_id
        )
    }));
    assert!(calls.contains(&RecordedBackendCall::Resolve(RequestId::Integer(41))));
    assert!(calls.contains(&RecordedBackendCall::Interrupt {
        thread_id: shell.thread_id,
        turn_id: "turn-interrupt".to_string(),
    }));
    assert!(calls.contains(&RecordedBackendCall::Unsubscribe(shell.thread_id)));
    assert!(calls.contains(&RecordedBackendCall::Shutdown));
    assert_eq!(shell.status, "disconnected");
    assert!(
        shell
            .transcript
            .iter()
            .any(|line| line.kind == TranscriptKind::Assistant && line.text == "streamed ")
    );
    assert!(
        shell
            .transcript
            .iter()
            .any(|line| line.kind == TranscriptKind::Error && line.text == "backend closed")
    );
}

#[derive(Clone, Default)]
struct RecordingBackend {
    calls: Arc<Mutex<Vec<RecordedBackendCall>>>,
    threads: Arc<Mutex<Vec<Thread>>>,
}

impl RecordingBackend {
    fn with_threads(threads: Vec<Thread>) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            threads: Arc::new(Mutex::new(threads)),
        }
    }

    fn calls(&self) -> Vec<RecordedBackendCall> {
        self.call_log()
            .lock()
            .expect("call log should lock")
            .clone()
    }

    fn call_log(&self) -> Arc<Mutex<Vec<RecordedBackendCall>>> {
        Arc::clone(&self.calls)
    }

    fn push(&self, call: RecordedBackendCall) {
        self.calls.lock().expect("call log should lock").push(call);
    }
}

#[derive(Debug, Clone, PartialEq)]
enum RecordedBackendCall {
    Start(Option<ThreadStartSource>),
    Resume(codex_protocol::ThreadId),
    Fork(codex_protocol::ThreadId),
    ThreadList {
        archived: Option<bool>,
        search_term: Option<String>,
    },
    Archive(codex_protocol::ThreadId),
    Unarchive(codex_protocol::ThreadId),
    Delete(codex_protocol::ThreadId),
    SetName {
        thread_id: codex_protocol::ThreadId,
        name: String,
    },
    TurnStart {
        thread_id: codex_protocol::ThreadId,
        prompt: String,
        cwd: PathBuf,
        model: String,
    },
    Interrupt {
        thread_id: codex_protocol::ThreadId,
        turn_id: String,
    },
    Resolve(RequestId),
    Reject {
        request_id: RequestId,
        message: String,
    },
    Unsubscribe(codex_protocol::ThreadId),
    Shutdown,
}

impl backend::AppShellBackend for RecordingBackend {
    async fn start_thread_with_session_start_source(
        &mut self,
        _config: &Config,
        session_start_source: Option<ThreadStartSource>,
    ) -> color_eyre::Result<crate::app_server_session::AppServerStartedThread> {
        self.push(RecordedBackendCall::Start(session_start_source));
        Ok(started_thread(
            "started",
            test_thread_id("01900000-0000-7000-8000-000000000201"),
            None,
        ))
    }

    async fn resume_thread(
        &mut self,
        _config: Config,
        thread_id: codex_protocol::ThreadId,
    ) -> color_eyre::Result<crate::app_server_session::AppServerStartedThread> {
        self.push(RecordedBackendCall::Resume(thread_id));
        Ok(started_thread("resumed", thread_id, None))
    }

    async fn fork_thread(
        &mut self,
        _config: Config,
        thread_id: codex_protocol::ThreadId,
    ) -> color_eyre::Result<crate::app_server_session::AppServerStartedThread> {
        self.push(RecordedBackendCall::Fork(thread_id));
        Ok(started_thread(
            "forked",
            test_thread_id("01900000-0000-7000-8000-000000000202"),
            Some(thread_id),
        ))
    }

    async fn thread_list(
        &mut self,
        params: ThreadListParams,
    ) -> color_eyre::Result<ThreadListResponse> {
        self.push(RecordedBackendCall::ThreadList {
            archived: params.archived,
            search_term: params.search_term.clone(),
        });
        let search_term = params.search_term.unwrap_or_default().to_lowercase();
        let data = self
            .threads
            .lock()
            .expect("threads should lock")
            .iter()
            .filter(|thread| {
                search_term.is_empty()
                    || thread
                        .name
                        .as_deref()
                        .unwrap_or(thread.preview.as_str())
                        .to_lowercase()
                        .contains(&search_term)
                    || thread.preview.to_lowercase().contains(&search_term)
            })
            .cloned()
            .collect();
        Ok(ThreadListResponse {
            data,
            next_cursor: None,
            backwards_cursor: None,
        })
    }

    async fn thread_archive(
        &mut self,
        thread_id: codex_protocol::ThreadId,
    ) -> color_eyre::Result<()> {
        self.push(RecordedBackendCall::Archive(thread_id));
        Ok(())
    }

    async fn thread_unarchive(
        &mut self,
        thread_id: codex_protocol::ThreadId,
    ) -> color_eyre::Result<Thread> {
        self.push(RecordedBackendCall::Unarchive(thread_id));
        Ok(thread_fixture(
            thread_id,
            Some("unarchived"),
            "unarchived preview",
        ))
    }

    async fn thread_delete(
        &mut self,
        thread_id: codex_protocol::ThreadId,
    ) -> color_eyre::Result<()> {
        self.push(RecordedBackendCall::Delete(thread_id));
        Ok(())
    }

    async fn thread_set_name(
        &mut self,
        thread_id: codex_protocol::ThreadId,
        name: String,
    ) -> color_eyre::Result<()> {
        self.push(RecordedBackendCall::SetName { thread_id, name });
        Ok(())
    }

    async fn turn_start(
        &mut self,
        params: backend::AppShellTurnStart,
    ) -> color_eyre::Result<TurnStartResponse> {
        let prompt = params
            .items
            .iter()
            .find_map(|item| match item {
                ApiUserInput::Text { text, .. } => Some(text.clone()),
                ApiUserInput::Image { .. }
                | ApiUserInput::LocalImage { .. }
                | ApiUserInput::Skill { .. }
                | ApiUserInput::Mention { .. } => None,
            })
            .unwrap_or_default();
        self.push(RecordedBackendCall::TurnStart {
            thread_id: params.thread_id,
            prompt,
            cwd: params.cwd,
            model: params.model,
        });
        Ok(TurnStartResponse {
            turn: test_turn("turn-submit", TurnStatus::InProgress),
        })
    }

    async fn turn_interrupt(
        &mut self,
        thread_id: codex_protocol::ThreadId,
        turn_id: String,
    ) -> std::result::Result<(), TypedRequestError> {
        self.push(RecordedBackendCall::Interrupt { thread_id, turn_id });
        Ok(())
    }

    async fn turn_steer(
        &mut self,
        _thread_id: codex_protocol::ThreadId,
        turn_id: String,
        _items: Vec<ApiUserInput>,
    ) -> std::result::Result<TurnSteerResponse, TypedRequestError> {
        Ok(TurnSteerResponse { turn_id })
    }

    async fn resolve_server_request(
        &self,
        request_id: RequestId,
        _result: serde_json::Value,
    ) -> std::io::Result<()> {
        self.push(RecordedBackendCall::Resolve(request_id));
        Ok(())
    }

    async fn reject_server_request(
        &self,
        request_id: RequestId,
        error: JSONRPCErrorError,
    ) -> std::io::Result<()> {
        self.push(RecordedBackendCall::Reject {
            request_id,
            message: error.message,
        });
        Ok(())
    }

    async fn unsubscribe_thread(
        &mut self,
        thread_id: codex_protocol::ThreadId,
    ) -> color_eyre::Result<()> {
        self.push(RecordedBackendCall::Unsubscribe(thread_id));
        Ok(())
    }

    async fn shutdown(self) -> std::io::Result<()> {
        self.push(RecordedBackendCall::Shutdown);
        Ok(())
    }
}

struct NoopWorkspaceRunner;

impl crate::workspace_command::WorkspaceCommandExecutor for NoopWorkspaceRunner {
    fn run(
        &self,
        _command: crate::workspace_command::WorkspaceCommand,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        crate::workspace_command::WorkspaceCommandOutput,
                        crate::workspace_command::WorkspaceCommandError,
                    >,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async {
            Ok(crate::workspace_command::WorkspaceCommandOutput {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            })
        })
    }
}

async fn test_config() -> Config {
    let codex_home = tempfile::tempdir().expect("temp codex home should be created");
    Config::load_default_with_cli_overrides_for_codex_home(
        codex_home.path().to_path_buf(),
        Vec::new(),
    )
    .await
    .expect("test config should load")
}

fn started_thread(
    name: &str,
    thread_id: codex_protocol::ThreadId,
    forked_from_id: Option<codex_protocol::ThreadId>,
) -> crate::app_server_session::AppServerStartedThread {
    crate::app_server_session::AppServerStartedThread {
        session: crate::session_state::ThreadSessionState {
            thread_id,
            forked_from_id,
            fork_parent_title: forked_from_id.map(|_| "parent".to_string()),
            thread_name: Some(name.to_string()),
            model: "gpt-5-codex".to_string(),
            model_provider_id: "openai".to_string(),
            service_tier: None,
            approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer::User,
            permission_profile: codex_protocol::models::PermissionProfile::default(),
            active_permission_profile: None,
            cwd: test_absolute_path("workspace/better-codex"),
            runtime_workspace_roots: vec![test_absolute_path("workspace/better-codex")],
            instruction_source_paths: Vec::new(),
            reasoning_effort: None,
            collaboration_mode: None,
            personality: None,
            message_history: None,
            network_proxy: None,
            rollout_path: None,
        },
        turns: Vec::new(),
    }
}

fn thread_fixture(
    thread_id: codex_protocol::ThreadId,
    name: Option<&str>,
    preview: &str,
) -> Thread {
    Thread {
        id: thread_id.to_string(),
        extra: None,
        session_id: thread_id.to_string(),
        forked_from_id: None,
        parent_thread_id: None,
        preview: preview.to_string(),
        ephemeral: false,
        model_provider: "openai".to_string(),
        created_at: 1_900_000_000,
        updated_at: 1_900_000_100,
        recency_at: Some(1_900_000_100),
        status: ThreadStatus::NotLoaded,
        path: None,
        cwd: test_absolute_path("workspace/better-codex"),
        cli_version: "0.0.0-test".to_string(),
        source: SessionSource::Cli,
        thread_source: None,
        agent_nickname: None,
        agent_role: None,
        git_info: Some(codex_app_server_protocol::GitInfo {
            sha: None,
            branch: Some("main".to_string()),
            origin_url: None,
        }),
        name: name.map(ToString::to_string),
        turns: Vec::new(),
    }
}

fn test_turn(id: &str, status: TurnStatus) -> Turn {
    let is_complete = status != TurnStatus::InProgress;
    Turn {
        id: id.to_string(),
        items: Vec::new(),
        items_view: TurnItemsView::default(),
        status,
        error: None,
        started_at: Some(1),
        completed_at: is_complete.then_some(2),
        duration_ms: is_complete.then_some(1_000),
    }
}

fn test_thread_id(value: &str) -> codex_protocol::ThreadId {
    codex_protocol::ThreadId::from_string(value).expect("test thread id should be valid")
}
