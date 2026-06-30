use super::render::ShellView;
use super::render::TranscriptScrollbarMetrics;
use super::*;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::AdditionalNetworkPermissions;
use codex_app_server_protocol::AppSummary;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigWriteResponse;
use codex_app_server_protocol::ExternalAgentConfigDetectParams;
use codex_app_server_protocol::ExternalAgentConfigDetectResponse;
use codex_app_server_protocol::ExternalAgentConfigImportCompletedNotification;
use codex_app_server_protocol::ExternalAgentConfigImportItemTypeSuccess;
use codex_app_server_protocol::ExternalAgentConfigImportTypeResult;
use codex_app_server_protocol::ExternalAgentConfigMigrationItem;
use codex_app_server_protocol::ExternalAgentConfigMigrationItemType;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ListMcpServerStatusParams;
use codex_app_server_protocol::ListMcpServerStatusResponse;
use codex_app_server_protocol::McpAuthStatus;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestParams;
use codex_app_server_protocol::McpServerOauthLoginParams;
use codex_app_server_protocol::McpServerOauthLoginResponse;
use codex_app_server_protocol::McpServerRefreshResponse;
use codex_app_server_protocol::McpServerStatus;
use codex_app_server_protocol::McpServerStatusDetail;
use codex_app_server_protocol::MergeStrategy;
use codex_app_server_protocol::MigrationDetails;
use codex_app_server_protocol::PermissionsRequestApprovalParams;
use codex_app_server_protocol::PluginAuthPolicy;
use codex_app_server_protocol::PluginAvailability;
use codex_app_server_protocol::PluginInstallParams;
use codex_app_server_protocol::PluginInstallPolicy;
use codex_app_server_protocol::PluginInstallResponse;
use codex_app_server_protocol::PluginInterface;
use codex_app_server_protocol::PluginListParams;
use codex_app_server_protocol::PluginListResponse;
use codex_app_server_protocol::PluginMarketplaceEntry;
use codex_app_server_protocol::PluginSource;
use codex_app_server_protocol::PluginSummary;
use codex_app_server_protocol::PluginUninstallParams;
use codex_app_server_protocol::PluginUninstallResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::SkillMigration;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadGoalClearedNotification;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadGoalUpdatedNotification;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
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
use codex_app_server_protocol::WriteStatus;
use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelServiceTier;
use codex_protocol::openai_models::ReasoningEffort;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::LegacyAppPathString;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
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
fn renders_scrolled_session_list_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    shell.session_list.replace_threads(
        (1..=10)
            .map(|index| {
                let thread_id = test_thread_id(&format!("01900000-0000-7000-8000-{index:012x}"));
                let title = format!("Session {index:02}");
                let preview = format!("Preview for session {index:02}");
                thread_fixture(thread_id, Some(&title), &preview)
            })
            .collect(),
    );
    for _ in 0..7 {
        shell.session_list.move_selection_down();
    }
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 32,
    );

    let rendered = render_shell(&shell, area);

    assert!(rendered.contains("3/10 Session 03"));
    assert!(rendered.contains(">  8/10 Session 08"));
    assert!(!rendered.contains("1/10 Session 01"));
    insta::assert_snapshot!(rendered);
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
    shell.settings.focused = true;
    shell.reasoning_effort = Some(ReasoningEffort::High);
    shell.service_tier = Some("flex".to_string());
    shell.tui_theme = Some("dracula".to_string());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_settings_pages_validation_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Settings;
    shell.settings.focused = true;
    shell
        .settings
        .start_edit(SettingsAction::Theme, "missing-theme".to_string());
    shell
        .settings
        .set_error("unknown syntax theme `missing-theme`");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 32,
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
            (CommandPaletteAction::SwitchModel, true),
            (CommandPaletteAction::ChangePermissions, true),
            (CommandPaletteAction::ResumeThread, true),
            (CommandPaletteAction::ForkThread, true),
            (CommandPaletteAction::ImportExternalAgentConfig, true),
            (CommandPaletteAction::CompactContext, false),
        ]
    );
}

#[tokio::test]
async fn command_palette_opens_native_model_and_permissions_settings() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();

    shell.open_command_palette();
    select_command_palette_action(&mut shell, CommandPaletteAction::SwitchModel);
    shell
        .execute_selected_command_palette_action(&mut backend)
        .await
        .expect("model action should open settings");

    assert_eq!(shell.dashboard_route, DashboardRoute::Settings);
    assert!(shell.settings.focused);
    assert!(shell.settings.editing());

    shell.open_command_palette();
    select_command_palette_action(&mut shell, CommandPaletteAction::ChangePermissions);
    shell
        .execute_selected_command_palette_action(&mut backend)
        .await
        .expect("permissions action should open settings");

    assert_eq!(shell.dashboard_route, DashboardRoute::Settings);
    assert!(shell.settings.focused);
    assert!(!shell.settings.editing());
    let rendered = render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 32,
        ),
    );
    assert!(
        rendered.contains("> approval: on-request"),
        "permissions action should focus approval policy row, got:\n{rendered}"
    );
}

#[tokio::test]
async fn command_palette_opens_native_session_list_for_resume_and_fork() {
    let session_id = test_thread_id("01900000-0000-7000-8000-000000000601");
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::with_threads(vec![thread_fixture(
        session_id,
        Some("resume from palette"),
        "palette preview",
    )]);

    shell.open_command_palette();
    select_command_palette_action(&mut shell, CommandPaletteAction::ResumeThread);
    shell
        .execute_selected_command_palette_action(&mut backend)
        .await
        .expect("resume action should open sessions");

    assert_eq!(shell.dashboard_route, DashboardRoute::Sessions);
    assert!(shell.session_list.focused);
    assert!(!shell.settings.focused);
    assert!(shell.session_list.selected_is_current(session_id));
    assert!(
        shell.transcript.iter().any(|line| {
            line.kind == TranscriptKind::Status && line.text == "press r to resume selected session"
        }),
        "resume action should leave a keyboard hint"
    );

    shell.open_command_palette();
    select_command_palette_action(&mut shell, CommandPaletteAction::ForkThread);
    shell
        .execute_selected_command_palette_action(&mut backend)
        .await
        .expect("fork action should open sessions");

    assert_eq!(shell.dashboard_route, DashboardRoute::Sessions);
    assert!(shell.session_list.focused);
    assert!(
        shell.transcript.iter().any(|line| {
            line.kind == TranscriptKind::Status && line.text == "press f to fork selected session"
        }),
        "fork action should leave a keyboard hint"
    );
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
            },
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
            },
        ]
    );
}

#[tokio::test]
async fn command_palette_opens_external_agent_import_review() {
    let items = external_agent_items();
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::with_external_agent_items(items.clone());

    shell.open_command_palette();
    select_command_palette_action(&mut shell, CommandPaletteAction::ImportExternalAgentConfig);
    shell
        .execute_selected_command_palette_action(&mut backend)
        .await
        .expect("import action should detect Claude Code setup");

    assert!(shell.pending_external_agent_import.is_some());
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::ExternalAgentConfigDetect {
            include_home: true,
            cwds: Some(vec![PathBuf::from(&shell.cwd)]),
        }]
    );
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 32,
        )
    ));
}

#[tokio::test]
async fn external_agent_import_starts_selected_items_and_reports_completion() {
    let items = external_agent_items();
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::with_external_agent_items(items.clone());

    shell
        .start_external_agent_import_review(&mut backend)
        .await
        .expect("review should open");
    shell
        .handle_external_agent_import_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("selected items should import");

    assert_eq!(shell.pending_external_agent_import, None);
    assert!(
        shell.transcript.iter().any(|line| {
            line.kind == TranscriptKind::Status && line.text.contains("Claude Code import started")
        }),
        "started import should be reported"
    );
    shell
        .handle_app_server_event(
            &mut backend,
            &NoopWorkspaceRunner,
            AppServerEvent::ServerNotification(
                ServerNotification::ExternalAgentConfigImportCompleted(
                    external_agent_import_completed_notification(),
                ),
            ),
        )
        .await
        .expect("completion notification should be handled");

    assert!(
        shell.transcript.iter().any(|line| {
            line.kind == TranscriptKind::Status && line.text.contains("Claude Code import finished")
        }),
        "completed import should be reported"
    );
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ExternalAgentConfigDetect {
                include_home: true,
                cwds: Some(vec![PathBuf::from(&shell.cwd)]),
            },
            RecordedBackendCall::ExternalAgentConfigImport(items),
            RecordedBackendCall::ExternalAgentConfigImportCompletionConsumed,
        ]
    );
}

fn external_agent_items() -> Vec<ExternalAgentConfigMigrationItem> {
    vec![
        ExternalAgentConfigMigrationItem {
            item_type: ExternalAgentConfigMigrationItemType::Config,
            description: "Import settings from Claude Code".to_string(),
            cwd: None,
            details: None,
        },
        ExternalAgentConfigMigrationItem {
            item_type: ExternalAgentConfigMigrationItemType::Skills,
            description: "Import skills from Claude Code".to_string(),
            cwd: Some(PathBuf::from("/repo/better-codex")),
            details: Some(MigrationDetails {
                skills: vec![SkillMigration {
                    name: "review".to_string(),
                }],
                ..MigrationDetails::default()
            }),
        },
    ]
}

fn external_agent_import_completed_notification() -> ExternalAgentConfigImportCompletedNotification
{
    ExternalAgentConfigImportCompletedNotification {
        import_id: "import-1".to_string(),
        item_type_results: vec![ExternalAgentConfigImportTypeResult {
            item_type: ExternalAgentConfigMigrationItemType::Config,
            successes: vec![ExternalAgentConfigImportItemTypeSuccess {
                item_type: ExternalAgentConfigMigrationItemType::Config,
                cwd: None,
                source: Some("Claude Code".to_string()),
                target: Some("config.toml".to_string()),
            }],
            failures: Vec::new(),
        }],
    }
}

fn select_command_palette_action(shell: &mut ShellState, action: CommandPaletteAction) {
    let entries = shell.command_palette_entries();
    let index = entries
        .iter()
        .position(|entry| entry.action == action)
        .expect("palette action should exist");
    let palette = shell
        .command_palette
        .as_mut()
        .expect("command palette should be open");
    for _ in 0..index {
        palette.move_down(&entries);
    }
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
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL)),
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
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('\u{0000}'), KeyModifiers::NONE)),
        Some(DashboardRoute::Workspace)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE)),
        Some(DashboardRoute::Workspace)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('\u{001b}'), KeyModifiers::NONE)),
        Some(DashboardRoute::Settings)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::CONTROL)),
        Some(DashboardRoute::Settings)
    );
    assert_eq!(dashboard_route_from_key(key_char('1')), None);
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        None
    );
}

#[test]
fn dashboard_route_step_key_mapping_covers_alt_arrows() {
    assert_eq!(
        dashboard_route_step_from_key(
            KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
            /*allow_word_motion_fallback*/ false
        ),
        Some(DashboardRouteStep::Previous)
    );
    assert_eq!(
        dashboard_route_step_from_key(
            KeyEvent::new(KeyCode::Right, KeyModifiers::ALT),
            /*allow_word_motion_fallback*/ false
        ),
        Some(DashboardRouteStep::Next)
    );
    assert_eq!(
        dashboard_route_step_from_key(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            /*allow_word_motion_fallback*/ false
        ),
        None
    );
}

#[test]
fn composer_word_motion_key_mapping_covers_standard_shortcuts() {
    assert_eq!(
        composer_word_motion_from_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL)),
        Some(ComposerWordMotion::Left)
    );
    assert_eq!(
        composer_word_motion_from_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL)),
        Some(ComposerWordMotion::Right)
    );
    assert_eq!(
        composer_word_motion_from_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)),
        Some(ComposerWordMotion::Left)
    );
    assert_eq!(
        composer_word_motion_from_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)),
        Some(ComposerWordMotion::Right)
    );
    assert_eq!(composer_word_motion_from_key(key_char('b')), None);
}

#[test]
fn composer_backspace_key_mapping_covers_modified_shortcuts() {
    assert_eq!(
        composer_backspace_action_from_key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::CONTROL
        )),
        Some(ComposerBackspaceAction::Clear)
    );
    assert_eq!(
        composer_backspace_action_from_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT)),
        Some(ComposerBackspaceAction::DeleteWordLeft)
    );
    assert_eq!(
        composer_backspace_action_from_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        Some(ComposerBackspaceAction::DeleteChar)
    );
    assert_eq!(
        composer_backspace_action_from_key(KeyEvent::new(
            KeyCode::Char('\u{007f}'),
            KeyModifiers::ALT
        )),
        Some(ComposerBackspaceAction::DeleteWordLeft)
    );
    assert_eq!(
        composer_backspace_action_from_key(KeyEvent::new(
            KeyCode::Char('\u{007f}'),
            KeyModifiers::NONE
        )),
        None
    );
}

#[tokio::test]
async fn composer_modified_backspace_shortcuts_delete_expected_text() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.set_text("alpha beta");

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT),
            &config,
            &mut backend,
        )
        .await
        .expect("alt backspace should delete a word");
    assert_eq!(shell.composer.text(), "alpha ");

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .expect("ctrl backspace should clear the composer");
    assert_eq!(shell.composer.text(), "");
    assert_eq!(backend.calls(), Vec::new());
}

#[tokio::test]
async fn composer_backspace_repeat_deletes_continuously() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.set_text("abc");

    for kind in [
        KeyEventKind::Press,
        KeyEventKind::Repeat,
        KeyEventKind::Repeat,
    ] {
        shell
            .handle_key(
                KeyEvent::new_with_kind(KeyCode::Backspace, KeyModifiers::NONE, kind),
                &config,
                &mut backend,
            )
            .await
            .expect("backspace press and repeat should delete");
    }

    assert_eq!(shell.composer.text(), "");
    assert_eq!(backend.calls(), Vec::new());
}

#[cfg(target_os = "macos")]
#[test]
fn dashboard_route_step_matches_macos_option_arrow_fallbacks_only_when_allowed() {
    assert_eq!(
        dashboard_route_step_from_key(
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
            /*allow_word_motion_fallback*/ true
        ),
        Some(DashboardRouteStep::Previous)
    );
    assert_eq!(
        dashboard_route_step_from_key(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT),
            /*allow_word_motion_fallback*/ true
        ),
        Some(DashboardRouteStep::Next)
    );
    assert_eq!(
        dashboard_route_step_from_key(
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
            /*allow_word_motion_fallback*/ false
        ),
        None
    );
    assert_eq!(
        dashboard_route_step_from_key(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT),
            /*allow_word_motion_fallback*/ false
        ),
        None
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn dashboard_route_step_matches_alt_arrows_only_off_macos() {
    assert_eq!(
        dashboard_route_step_from_key(
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
            /*allow_word_motion_fallback*/ true
        ),
        None
    );
    assert_eq!(
        dashboard_route_step_from_key(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT),
            /*allow_word_motion_fallback*/ true
        ),
        None
    );
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
fn tool_transcript_blocks_use_status_accent_colors() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.push_tool_with_status("exec just test -p codex-tui", ToolBlockStatus::Running);
    shell.push_tool_with_status("exec true exit 0", ToolBlockStatus::Success);
    shell.push_tool_with_status("exec false exit 1", ToolBlockStatus::Fail);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
    );

    let buf = render_shell_buffer(&shell, area);

    assert_eq!(
        accent_color_for_row(&buf, area, "exec just test"),
        Some(Color::Cyan)
    );
    assert_eq!(
        accent_color_for_row(&buf, area, "exec true"),
        Some(Color::Green)
    );
    assert_eq!(
        accent_color_for_row(&buf, area, "exec false"),
        Some(Color::Red)
    );
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
fn rendered_transcript_leaves_gap_before_scrollbar() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    for index in 0..40 {
        shell.push_assistant(format!("scrollbar gap transcript row {index}"));
    }
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 24,
    );

    let buf = render_shell_buffer(&shell, area);

    let (x, y) = scrollbar_cell(&buf, area).expect("scrollbar should render");
    assert_eq!(
        buf.cell((x.saturating_sub(1), y))
            .expect("gap cell should exist")
            .symbol(),
        " "
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
fn composer_moves_by_word() {
    let mut composer = ComposerState::default();
    composer.set_text("alpha beta_gamma, delta");

    composer.move_word_left();
    assert_eq!(composer.cursor_position(), (0, 18));

    composer.move_word_left();
    assert_eq!(composer.cursor_position(), (0, 6));

    composer.move_word_right();
    assert_eq!(composer.cursor_position(), (0, 16));

    composer.move_word_right();
    assert_eq!(composer.cursor_position(), (0, 23));
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
    let buf = render_shell_buffer(shell, area);
    buffer_contents(&buf, area)
}

fn render_shell_buffer(shell: &ShellState, area: Rect) -> Buffer {
    let mut buf = Buffer::empty(area);
    ShellView { shell }.render(area, &mut buf);
    buf
}

fn accent_color_for_row(buf: &Buffer, area: Rect, needle: &str) -> Option<Color> {
    for y in area.y..area.bottom() {
        let mut row = String::new();
        for x in area.x..area.right() {
            if let Some(cell) = buf.cell((x, y)) {
                row.push_str(cell.symbol());
            }
        }
        if row.contains(needle) {
            for x in area.x..area.right() {
                let cell = buf.cell((x, y))?;
                if cell.symbol() == "▌" {
                    return cell.style().fg;
                }
            }
        }
    }
    None
}

fn scrollbar_cell(buf: &Buffer, area: Rect) -> Option<(u16, u16)> {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let cell = buf.cell((x, y))?;
            if matches!(cell.symbol(), "┃" | "│") {
                return Some((x, y));
            }
        }
    }
    None
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

fn model_preset_fixture(slug: &str, show_in_picker: bool, service_tiers: &[&str]) -> ModelPreset {
    ModelPreset {
        id: slug.to_string(),
        model: slug.to_string(),
        display_name: slug.to_string(),
        description: format!("{slug} description"),
        default_reasoning_effort: ReasoningEffort::Medium,
        supported_reasoning_efforts: Vec::new(),
        supports_personality: false,
        additional_speed_tiers: Vec::new(),
        service_tiers: service_tiers
            .iter()
            .map(|tier| ModelServiceTier {
                id: (*tier).to_string(),
                name: (*tier).to_string(),
                description: format!("{tier} description"),
            })
            .collect(),
        default_service_tier: None,
        is_default: false,
        upgrade: None,
        show_in_picker,
        availability_nux: None,
        supported_in_api: true,
        input_modalities: Vec::new(),
    }
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

fn mcp_status_fixture<const N: usize>(
    name: &str,
    auth_status: McpAuthStatus,
    tools: [&str; N],
) -> McpServerStatus {
    McpServerStatus {
        name: name.to_string(),
        server_info: None,
        tools: tools
            .into_iter()
            .map(|tool| {
                (
                    tool.to_string(),
                    codex_protocol::mcp::Tool {
                        name: tool.to_string(),
                        title: None,
                        description: None,
                        input_schema: serde_json::json!({}),
                        output_schema: None,
                        annotations: None,
                        icons: None,
                        meta: None,
                    },
                )
            })
            .collect(),
        resources: Vec::new(),
        resource_templates: Vec::new(),
        auth_status,
    }
}

fn plugin_list_response_fixture() -> PluginListResponse {
    PluginListResponse {
        marketplaces: vec![PluginMarketplaceEntry {
            name: "local".to_string(),
            path: Some(test_absolute_path("codex-home/plugins/marketplace.json")),
            interface: None,
            plugins: vec![
                plugin_summary_fixture("plugin-calendar", "Calendar", true, true),
                plugin_summary_fixture("plugin-drive", "Drive", false, false),
            ],
        }],
        marketplace_load_errors: Vec::new(),
        featured_plugin_ids: Vec::new(),
    }
}

fn plugin_summary_fixture(id: &str, name: &str, installed: bool, enabled: bool) -> PluginSummary {
    PluginSummary {
        id: id.to_string(),
        remote_plugin_id: None,
        local_version: None,
        name: name.to_string(),
        share_context: None,
        source: PluginSource::Local {
            path: test_absolute_path(&format!("codex-home/plugins/{id}")),
        },
        installed,
        enabled,
        install_policy: PluginInstallPolicy::Available,
        auth_policy: PluginAuthPolicy::OnUse,
        availability: PluginAvailability::Available,
        interface: Some(PluginInterface {
            display_name: Some(name.to_string()),
            short_description: None,
            long_description: None,
            developer_name: None,
            category: None,
            capabilities: Vec::new(),
            website_url: None,
            privacy_policy_url: None,
            terms_of_service_url: None,
            default_prompt: None,
            brand_color: None,
            composer_icon: None,
            composer_icon_url: None,
            logo: None,
            logo_dark: None,
            logo_url: None,
            logo_url_dark: None,
            screenshots: Vec::new(),
            screenshot_urls: Vec::new(),
        }),
        keywords: Vec::new(),
    }
}

fn mutate_plugin(
    response: &Arc<Mutex<Option<PluginListResponse>>>,
    plugin_key: &str,
    mut update: impl FnMut(&mut PluginSummary),
) {
    let mut response = response.lock().expect("plugin response should lock");
    let Some(response) = response.as_mut() else {
        return;
    };
    for marketplace in &mut response.marketplaces {
        for plugin in &mut marketplace.plugins {
            if plugin.id == plugin_key || plugin.name == plugin_key {
                update(plugin);
                return;
            }
        }
    }
}

fn remove_mcp_status(statuses: &Arc<Mutex<Vec<McpServerStatus>>>, server_name: &str) {
    statuses
        .lock()
        .expect("mcp statuses should lock")
        .retain(|status| status.name != server_name);
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
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::ThreadList {
            archived: Some(false),
            search_term: None,
        }]
    );
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
async fn native_settings_integrations_refresh_mcp_and_plugins() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Settings;
    shell.settings.focused = true;
    shell.settings.focus_action(SettingsAction::McpServers);
    let mut backend = RecordingBackend::with_integrations(
        vec![
            mcp_status_fixture("github", McpAuthStatus::OAuth, ["search", "read"]),
            mcp_status_fixture("linear", McpAuthStatus::NotLoggedIn, ["issue"]),
        ],
        plugin_list_response_fixture(),
    );

    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("mcp inventory should refresh");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("plugins row should be selected");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("plugin inventory should refresh");

    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::McpServerStatusList {
                cursor: None,
                detail: Some(McpServerStatusDetail::ToolsAndAuthOnly),
                thread_id: Some(shell.thread_id.to_string()),
            },
            RecordedBackendCall::PluginList {
                cwd: Some(vec![test_absolute_path("workspace/better-codex")]),
                marketplace_kinds: None,
            },
        ]
    );
    assert_eq!(
        shell.mcp_inventory.label(),
        "2 servers / 3 tools / 1 login needed"
    );
    assert_eq!(shell.plugin_inventory.label(), "1 installed / 2 available");
    let rendered = render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 34,
        ),
    );
    assert!(
        rendered.contains("Integrations"),
        "dashboard should render native integrations panel:\n{rendered}"
    );
}

#[tokio::test]
async fn mcp_management_catalog_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Settings;
    shell.settings.focused = true;
    shell.settings.focus_action(SettingsAction::McpServers);
    let mut backend = RecordingBackend::with_integrations(
        vec![
            mcp_status_fixture("github", McpAuthStatus::NotLoggedIn, ["search", "read"]),
            mcp_status_fixture("linear", McpAuthStatus::BearerToken, ["issue"]),
        ],
        plugin_list_response_fixture(),
    );

    for description in ["mcp inventory should refresh", "mcp manager should open"] {
        shell
            .handle_settings_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &mut backend,
            )
            .await
            .expect(description);
    }

    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 36,
        ),
    ));
}

#[tokio::test]
async fn mcp_management_actions_login_disable_remove_add_and_edit() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Settings;
    shell.settings.focused = true;
    shell.settings.focus_action(SettingsAction::McpServers);
    let mut backend = RecordingBackend::with_integrations(
        vec![
            mcp_status_fixture("github", McpAuthStatus::NotLoggedIn, ["search"]),
            mcp_status_fixture("linear", McpAuthStatus::BearerToken, ["issue"]),
        ],
        plugin_list_response_fixture(),
    );

    for description in ["mcp inventory should refresh", "mcp manager should open"] {
        shell
            .handle_settings_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &mut backend,
            )
            .await
            .expect(description);
    }

    for code in [
        KeyCode::Char('l'),
        KeyCode::Down,
        KeyCode::Char('d'),
        KeyCode::Char('x'),
    ] {
        shell
            .handle_key(
                KeyEvent::new(code, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .expect("mcp action should succeed");
    }

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("add mode should open");
    for ch in r#"docs {"url":"https://example.test/mcp"}"#.chars() {
        shell
            .handle_key(
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .expect("draft char should be accepted");
    }
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("add edit should save");

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("edit mode should open");
    for _ in 0.."docs {}".len() {
        shell
            .handle_key(
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .expect("draft char should delete");
    }
    for ch in r#"docs {"url":"https://example.test/updated"}"#.chars() {
        shell
            .handle_key(
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .expect("draft char should be accepted");
    }
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("edit should save");

    let calls = backend.calls();
    for expected_call in [
        RecordedBackendCall::McpServerOauthLogin {
            name: "github".to_string(),
            thread_id: Some(shell.thread_id.to_string()),
        },
        RecordedBackendCall::McpServerWriteConfig {
            server_name: "linear".to_string(),
            value: serde_json::json!({ "enabled": false }),
            merge_strategy: MergeStrategy::Upsert,
        },
        RecordedBackendCall::McpServerWriteConfig {
            server_name: "linear".to_string(),
            value: serde_json::Value::Null,
            merge_strategy: MergeStrategy::Replace,
        },
        RecordedBackendCall::McpServerWriteConfig {
            server_name: "docs".to_string(),
            value: serde_json::json!({ "url": "https://example.test/mcp" }),
            merge_strategy: MergeStrategy::Replace,
        },
        RecordedBackendCall::McpServerWriteConfig {
            server_name: "docs".to_string(),
            value: serde_json::json!({ "url": "https://example.test/updated" }),
            merge_strategy: MergeStrategy::Replace,
        },
    ] {
        assert!(calls.contains(&expected_call), "{expected_call:?}");
    }
}

#[tokio::test]
async fn plugin_management_catalog_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Settings;
    shell.settings.focused = true;
    shell.settings.focus_action(SettingsAction::Plugins);
    let mut backend =
        RecordingBackend::with_integrations(Vec::new(), plugin_list_response_fixture());

    for description in [
        "plugin inventory should refresh",
        "plugin catalog should open",
    ] {
        shell
            .handle_settings_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &mut backend,
            )
            .await
            .expect(description);
    }

    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 36,
        ),
    ));
}

#[tokio::test]
async fn plugin_management_actions_update_enable_install_auth_and_uninstall() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Settings;
    shell.settings.focused = true;
    shell.settings.focus_action(SettingsAction::Plugins);
    let mut backend =
        RecordingBackend::with_integrations(Vec::new(), plugin_list_response_fixture());
    backend.set_plugin_install_response(PluginInstallResponse {
        auth_policy: PluginAuthPolicy::OnInstall,
        apps_needing_auth: vec![AppSummary {
            id: "gmail".to_string(),
            name: "Gmail".to_string(),
            description: None,
            install_url: None,
            category: None,
        }],
    });

    for description in [
        "plugin inventory should refresh",
        "plugin catalog should open",
    ] {
        shell
            .handle_settings_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &mut backend,
            )
            .await
            .expect(description);
    }
    for (code, description) in [
        (KeyCode::Char('i'), "installed plugin should update"),
        (KeyCode::Char('e'), "installed plugin should disable"),
        (KeyCode::Char('e'), "installed plugin should enable"),
        (KeyCode::Down, "available plugin should be selected"),
        (KeyCode::Enter, "available plugin should install"),
        (KeyCode::Char('u'), "installed plugin should uninstall"),
    ] {
        shell
            .handle_key(
                KeyEvent::new(code, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .expect(description);
    }

    let calls = backend.calls();
    for expected_call in [
        RecordedBackendCall::PluginInstall {
            marketplace_path: Some(test_absolute_path("codex-home/plugins/marketplace.json")),
            remote_marketplace_name: None,
            plugin_name: "Calendar".to_string(),
        },
        RecordedBackendCall::PluginSetEnabled {
            plugin_id: "plugin-calendar".to_string(),
            enabled: false,
        },
        RecordedBackendCall::PluginSetEnabled {
            plugin_id: "plugin-calendar".to_string(),
            enabled: true,
        },
        RecordedBackendCall::PluginInstall {
            marketplace_path: Some(test_absolute_path("codex-home/plugins/marketplace.json")),
            remote_marketplace_name: None,
            plugin_name: "Drive".to_string(),
        },
        RecordedBackendCall::PluginUninstall {
            plugin_id: "plugin-drive".to_string(),
        },
    ] {
        assert!(calls.contains(&expected_call), "{expected_call:?}");
    }
    assert!(
        shell
            .transcript
            .iter()
            .any(|line| line.text.contains("auth required for Gmail")),
        "installing auth-required plugins should report the app auth follow-up"
    );
}

#[tokio::test]
async fn native_settings_pages_write_config_and_validate_edits() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Settings;
    shell.settings.focused = true;
    let mut backend = RecordingBackend::default();

    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("reasoning row should be selected");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("reasoning cycle should persist");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("permissions page should open");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("approval cycle should persist");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("appearance page should open");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("animations row should be selected");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("animations toggle should persist");

    let calls = backend.calls();
    assert!(calls.contains(&RecordedBackendCall::ConfigWrite(vec![
        ("model".to_string(), serde_json::json!("gpt-5-codex"),),
        (
            "model_reasoning_effort".to_string(),
            serde_json::json!("minimal"),
        ),
    ])));
    assert!(calls.contains(&RecordedBackendCall::ThreadSettingsUpdate {
        model: None,
        effort: Some(ReasoningEffort::Minimal),
        service_tier: None,
        approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
    }));
    assert!(calls.contains(&RecordedBackendCall::ConfigWrite(vec![(
        "approval_policy".to_string(),
        serde_json::json!("never"),
    )])));
    assert!(calls.contains(&RecordedBackendCall::ThreadSettingsUpdate {
        model: None,
        effort: None,
        service_tier: None,
        approval_policy: codex_app_server_protocol::AskForApproval::Never,
    }));
    assert!(calls.contains(&RecordedBackendCall::ConfigWrite(vec![(
        "tui.animations".to_string(),
        serde_json::json!(false),
    )])));
    assert_eq!(shell.reasoning_effort, Some(ReasoningEffort::Minimal));
    assert_eq!(
        shell.approval_policy,
        codex_app_server_protocol::AskForApproval::Never
    );
    assert!(!shell.animations);

    shell
        .handle_settings_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), &mut backend)
        .await
        .expect("theme row should be selected");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("theme edit should start");
    for ch in "missing-theme".chars() {
        shell
            .handle_settings_key(key_char(ch), &mut backend)
            .await
            .expect("theme draft should edit");
    }
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("invalid theme should be handled");

    assert_eq!(backend.calls(), calls);
    let rendered = render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 32,
        ),
    );
    assert!(
        rendered.contains("missing-theme"),
        "settings validation should render the invalid theme name"
    );
}

#[tokio::test]
async fn native_settings_cycle_models_and_service_tiers() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Settings;
    shell.settings.focused = true;
    shell.available_models = vec![
        model_preset_fixture("gpt-5-codex", /*show_in_picker*/ true, &[]),
        model_preset_fixture("hidden-model", /*show_in_picker*/ false, &[]),
        model_preset_fixture(
            "gpt-5.5",
            /*show_in_picker*/ true,
            &["fast-tier", "batch-tier"],
        ),
    ];
    let mut backend = RecordingBackend::default();

    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("model should cycle");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("reasoning row should be selected");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("service tier row should be selected");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("service tier should cycle to first tier");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("service tier should cycle to second tier");
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("service tier should cycle to default");

    assert_eq!(shell.model, "gpt-5.5");
    assert_eq!(
        shell.service_tier.as_deref(),
        Some(SERVICE_TIER_DEFAULT_REQUEST_VALUE)
    );
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ConfigWrite(vec![
                ("model".to_string(), json!("gpt-5.5")),
                (
                    "model_reasoning_effort".to_string(),
                    serde_json::Value::Null
                ),
            ]),
            RecordedBackendCall::ThreadSettingsUpdate {
                model: Some("gpt-5.5".to_string()),
                effort: None,
                service_tier: None,
                approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            },
            RecordedBackendCall::ConfigWrite(vec![(
                "service_tier".to_string(),
                json!("fast-tier"),
            )]),
            RecordedBackendCall::ThreadSettingsUpdate {
                model: None,
                effort: None,
                service_tier: Some(Some("fast-tier".to_string())),
                approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            },
            RecordedBackendCall::ConfigWrite(vec![(
                "service_tier".to_string(),
                json!("batch-tier"),
            )]),
            RecordedBackendCall::ThreadSettingsUpdate {
                model: None,
                effort: None,
                service_tier: Some(Some("batch-tier".to_string())),
                approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            },
            RecordedBackendCall::ConfigWrite(vec![(
                "service_tier".to_string(),
                json!(SERVICE_TIER_DEFAULT_REQUEST_VALUE),
            )]),
            RecordedBackendCall::ThreadSettingsUpdate {
                model: None,
                effort: None,
                service_tier: Some(Some(SERVICE_TIER_DEFAULT_REQUEST_VALUE.to_string())),
                approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            },
        ]
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

#[derive(Clone)]
struct RecordingBackend {
    calls: Arc<Mutex<Vec<RecordedBackendCall>>>,
    threads: Arc<Mutex<Vec<Thread>>>,
    mcp_statuses: Arc<Mutex<Vec<McpServerStatus>>>,
    plugin_response: Arc<Mutex<Option<PluginListResponse>>>,
    plugin_install_response: Arc<Mutex<PluginInstallResponse>>,
    external_agent_items: Arc<Mutex<Vec<ExternalAgentConfigMigrationItem>>>,
    external_agent_import_in_progress: Arc<Mutex<bool>>,
    remote_workspace: bool,
    embedded_app_server: bool,
}

impl Default for RecordingBackend {
    fn default() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            threads: Arc::new(Mutex::new(Vec::new())),
            mcp_statuses: Arc::new(Mutex::new(Vec::new())),
            plugin_response: Arc::new(Mutex::new(None)),
            plugin_install_response: Arc::new(Mutex::new(PluginInstallResponse {
                auth_policy: PluginAuthPolicy::OnUse,
                apps_needing_auth: Vec::new(),
            })),
            external_agent_items: Arc::new(Mutex::new(Vec::new())),
            external_agent_import_in_progress: Arc::new(Mutex::new(false)),
            remote_workspace: false,
            embedded_app_server: true,
        }
    }
}

impl RecordingBackend {
    fn with_threads(threads: Vec<Thread>) -> Self {
        Self {
            threads: Arc::new(Mutex::new(threads)),
            ..Self::default()
        }
    }

    fn with_integrations(
        mcp_statuses: Vec<McpServerStatus>,
        plugin_response: PluginListResponse,
    ) -> Self {
        Self {
            mcp_statuses: Arc::new(Mutex::new(mcp_statuses)),
            plugin_response: Arc::new(Mutex::new(Some(plugin_response))),
            ..Self::default()
        }
    }

    fn with_external_agent_items(items: Vec<ExternalAgentConfigMigrationItem>) -> Self {
        Self {
            external_agent_items: Arc::new(Mutex::new(items)),
            ..Self::default()
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

    fn set_plugin_install_response(&self, response: PluginInstallResponse) {
        *self
            .plugin_install_response
            .lock()
            .expect("plugin install response should lock") = response;
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
    ConfigWrite(Vec<(String, serde_json::Value)>),
    ThreadSettingsUpdate {
        model: Option<String>,
        effort: Option<ReasoningEffort>,
        service_tier: Option<Option<String>>,
        approval_policy: codex_app_server_protocol::AskForApproval,
    },
    McpServerStatusList {
        cursor: Option<String>,
        detail: Option<McpServerStatusDetail>,
        thread_id: Option<String>,
    },
    McpServerOauthLogin {
        name: String,
        thread_id: Option<String>,
    },
    McpServerRefresh,
    McpServerWriteConfig {
        server_name: String,
        value: serde_json::Value,
        merge_strategy: MergeStrategy,
    },
    PluginList {
        cwd: Option<Vec<AbsolutePathBuf>>,
        marketplace_kinds: Option<Vec<codex_app_server_protocol::PluginListMarketplaceKind>>,
    },
    PluginInstall {
        marketplace_path: Option<AbsolutePathBuf>,
        remote_marketplace_name: Option<String>,
        plugin_name: String,
    },
    PluginUninstall {
        plugin_id: String,
    },
    PluginSetEnabled {
        plugin_id: String,
        enabled: bool,
    },
    ExternalAgentConfigDetect {
        include_home: bool,
        cwds: Option<Vec<PathBuf>>,
    },
    ExternalAgentConfigImport(Vec<ExternalAgentConfigMigrationItem>),
    ExternalAgentConfigImportCompletionConsumed,
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

    async fn write_config(
        &mut self,
        edits: Vec<ConfigEdit>,
    ) -> color_eyre::Result<ConfigWriteResponse> {
        self.push(RecordedBackendCall::ConfigWrite(
            edits
                .into_iter()
                .map(|edit| (edit.key_path, edit.value))
                .collect(),
        ));
        Ok(ConfigWriteResponse {
            status: WriteStatus::Ok,
            version: "1".to_string(),
            file_path: test_absolute_path("codex-home/config.toml"),
            overridden_metadata: None,
        })
    }

    async fn thread_settings_update(
        &mut self,
        params: ThreadSettingsUpdateParams,
    ) -> color_eyre::Result<()> {
        self.push(RecordedBackendCall::ThreadSettingsUpdate {
            model: params.model,
            effort: params.effort,
            service_tier: params.service_tier,
            approval_policy: params
                .approval_policy
                .unwrap_or(codex_app_server_protocol::AskForApproval::OnRequest),
        });
        Ok(())
    }

    async fn mcp_server_status_list(
        &mut self,
        params: ListMcpServerStatusParams,
    ) -> color_eyre::Result<ListMcpServerStatusResponse> {
        self.push(RecordedBackendCall::McpServerStatusList {
            cursor: params.cursor,
            detail: params.detail,
            thread_id: params.thread_id,
        });
        Ok(ListMcpServerStatusResponse {
            data: self
                .mcp_statuses
                .lock()
                .expect("mcp statuses should lock")
                .clone(),
            next_cursor: None,
        })
    }

    async fn mcp_server_oauth_login(
        &mut self,
        params: McpServerOauthLoginParams,
    ) -> color_eyre::Result<McpServerOauthLoginResponse> {
        self.push(RecordedBackendCall::McpServerOauthLogin {
            name: params.name,
            thread_id: params.thread_id,
        });
        Ok(McpServerOauthLoginResponse {
            authorization_url: "https://auth.example.test/mcp".to_string(),
        })
    }

    async fn mcp_server_refresh(&mut self) -> color_eyre::Result<McpServerRefreshResponse> {
        self.push(RecordedBackendCall::McpServerRefresh);
        Ok(McpServerRefreshResponse {})
    }

    async fn mcp_server_write_config(
        &mut self,
        server_name: String,
        value: serde_json::Value,
        merge_strategy: MergeStrategy,
    ) -> color_eyre::Result<ConfigWriteResponse> {
        self.push(RecordedBackendCall::McpServerWriteConfig {
            server_name: server_name.clone(),
            value: value.clone(),
            merge_strategy,
        });
        if value.is_null() {
            remove_mcp_status(&self.mcp_statuses, &server_name);
        } else if !self
            .mcp_statuses
            .lock()
            .expect("mcp statuses should lock")
            .iter()
            .any(|status| status.name == server_name)
        {
            self.mcp_statuses
                .lock()
                .expect("mcp statuses should lock")
                .push(mcp_status_fixture(
                    &server_name,
                    McpAuthStatus::Unsupported,
                    [],
                ));
        }
        Ok(ConfigWriteResponse {
            status: WriteStatus::Ok,
            version: "1".to_string(),
            file_path: test_absolute_path("codex-home/config.toml"),
            overridden_metadata: None,
        })
    }

    async fn plugin_list(
        &mut self,
        params: PluginListParams,
    ) -> color_eyre::Result<PluginListResponse> {
        self.push(RecordedBackendCall::PluginList {
            cwd: params.cwds,
            marketplace_kinds: params.marketplace_kinds,
        });
        Ok(self
            .plugin_response
            .lock()
            .expect("plugin response should lock")
            .clone()
            .unwrap_or(PluginListResponse {
                marketplaces: Vec::new(),
                marketplace_load_errors: Vec::new(),
                featured_plugin_ids: Vec::new(),
            }))
    }

    async fn plugin_install(
        &mut self,
        params: PluginInstallParams,
    ) -> color_eyre::Result<PluginInstallResponse> {
        self.push(RecordedBackendCall::PluginInstall {
            marketplace_path: params.marketplace_path.clone(),
            remote_marketplace_name: params.remote_marketplace_name.clone(),
            plugin_name: params.plugin_name.clone(),
        });
        mutate_plugin(&self.plugin_response, &params.plugin_name, |plugin| {
            plugin.installed = true;
            plugin.enabled = true;
        });
        Ok(self
            .plugin_install_response
            .lock()
            .expect("plugin install response should lock")
            .clone())
    }

    async fn plugin_uninstall(
        &mut self,
        params: PluginUninstallParams,
    ) -> color_eyre::Result<PluginUninstallResponse> {
        self.push(RecordedBackendCall::PluginUninstall {
            plugin_id: params.plugin_id.clone(),
        });
        mutate_plugin(&self.plugin_response, &params.plugin_id, |plugin| {
            plugin.installed = false;
            plugin.enabled = false;
        });
        Ok(PluginUninstallResponse {})
    }

    async fn plugin_set_enabled(
        &mut self,
        plugin_id: String,
        enabled: bool,
    ) -> color_eyre::Result<ConfigWriteResponse> {
        self.push(RecordedBackendCall::PluginSetEnabled {
            plugin_id: plugin_id.clone(),
            enabled,
        });
        mutate_plugin(&self.plugin_response, &plugin_id, |plugin| {
            plugin.enabled = enabled;
        });
        Ok(ConfigWriteResponse {
            status: WriteStatus::Ok,
            version: "1".to_string(),
            file_path: test_absolute_path("codex-home/config.toml"),
            overridden_metadata: None,
        })
    }

    fn uses_remote_workspace(&self) -> bool {
        self.remote_workspace
    }

    fn uses_embedded_app_server(&self) -> bool {
        self.embedded_app_server
    }

    fn external_agent_config_import_in_progress(&self) -> bool {
        *self
            .external_agent_import_in_progress
            .lock()
            .expect("import progress should lock")
    }

    async fn external_agent_config_detect(
        &mut self,
        params: ExternalAgentConfigDetectParams,
    ) -> color_eyre::Result<ExternalAgentConfigDetectResponse> {
        self.push(RecordedBackendCall::ExternalAgentConfigDetect {
            include_home: params.include_home,
            cwds: params.cwds,
        });
        Ok(ExternalAgentConfigDetectResponse {
            items: self
                .external_agent_items
                .lock()
                .expect("external agent items should lock")
                .clone(),
        })
    }

    async fn external_agent_config_import(
        &mut self,
        migration_items: Vec<ExternalAgentConfigMigrationItem>,
    ) -> color_eyre::Result<()> {
        *self
            .external_agent_import_in_progress
            .lock()
            .expect("import progress should lock") = true;
        self.push(RecordedBackendCall::ExternalAgentConfigImport(
            migration_items,
        ));
        Ok(())
    }

    fn consume_external_agent_config_import_completion(&self) -> bool {
        let mut in_progress = self
            .external_agent_import_in_progress
            .lock()
            .expect("import progress should lock");
        let should_report = *in_progress;
        *in_progress = false;
        if should_report {
            self.push(RecordedBackendCall::ExternalAgentConfigImportCompletionConsumed);
        }
        should_report
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
