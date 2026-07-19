use super::render::ShellView;
use super::transcript_view::TranscriptCardHit;
use super::transcript_view::TranscriptScrollbarMetrics;
use super::*;
use base64::Engine;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::AccountRateLimitsUpdatedNotification;
use codex_app_server_protocol::AdditionalNetworkPermissions;
use codex_app_server_protocol::AppSummary;
use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::CommandExecOutputDeltaNotification;
use codex_app_server_protocol::CommandExecOutputStream;
use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::CommandExecutionOutputDeltaNotification;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigWriteResponse;
use codex_app_server_protocol::CurrentTimeReadParams;
use codex_app_server_protocol::CurrentTimeReadResponse;
use codex_app_server_protocol::ErrorNotification;
use codex_app_server_protocol::ExternalAgentConfigDetectParams;
use codex_app_server_protocol::ExternalAgentConfigDetectResponse;
use codex_app_server_protocol::ExternalAgentConfigImportCompletedNotification;
use codex_app_server_protocol::ExternalAgentConfigImportItemTypeSuccess;
use codex_app_server_protocol::ExternalAgentConfigImportTypeResult;
use codex_app_server_protocol::ExternalAgentConfigMigrationItem;
use codex_app_server_protocol::ExternalAgentConfigMigrationItemType;
use codex_app_server_protocol::FileChangePatchUpdatedNotification;
use codex_app_server_protocol::GetAccountRateLimitsResponse;
use codex_app_server_protocol::ImageGenerationItem;
use codex_app_server_protocol::ItemCompletedNotification;
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
use codex_app_server_protocol::ModelSafetyBufferingUpdatedNotification;
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
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ServerRequestResolvedNotification;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::SkillMigration;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadActiveFlag;
use codex_app_server_protocol::ThreadArchivedNotification;
use codex_app_server_protocol::ThreadClosedNotification;
use codex_app_server_protocol::ThreadDeletedNotification;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadGoalClearResponse;
use codex_app_server_protocol::ThreadGoalClearedNotification;
use codex_app_server_protocol::ThreadGoalGetResponse;
use codex_app_server_protocol::ThreadGoalSetResponse;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadGoalUpdatedNotification;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadRollbackResponse;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadSettingsUpdatedNotification;
use codex_app_server_protocol::ThreadStartSource;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::ThreadStatusChangedNotification;
use codex_app_server_protocol::ThreadUnarchivedNotification;
use codex_app_server_protocol::ToolRequestUserInputOption;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_app_server_protocol::TurnDiffUpdatedNotification;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::TurnSteerResponse;
use codex_app_server_protocol::UserInput as ApiUserInput;
use codex_app_server_protocol::WebSearchItem;
use codex_app_server_protocol::WriteStatus;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
use codex_protocol::config_types::Settings as CollaborationModeSettings;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelServiceTier;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::LegacyAppPathString;
use itertools::Itertools;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::text::Line;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

const SNAPSHOT_THREAD_ID: &str = "01900000-0000-7000-8000-000000000001";

#[test]
fn renders_first_stage_shell_snapshot() {
    let shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[tokio::test]
async fn renders_aggregated_backend_lag_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_transcript();
    shell.dashboard_visible = false;
    shell.push_user("Run just test -p codex-tui.");
    let mut backend = RecordingBackend::default();
    shell
        .handle_app_server_event(&mut backend, AppServerEvent::Lagged { skipped: 42 })
        .await
        .expect("lag event should be handled");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn remote_thread_status_updates_the_active_shell() {
    let mut shell = ShellState::snapshot_fixture();
    let thread_id = shell.thread_id.to_string();
    let mut observed = Vec::new();
    for status in [
        ThreadStatus::Idle,
        ThreadStatus::SystemError,
        ThreadStatus::Active {
            active_flags: Vec::new(),
        },
        ThreadStatus::Active {
            active_flags: vec![ThreadActiveFlag::WaitingOnApproval],
        },
    ] {
        shell.handle_notification(ServerNotification::ThreadStatusChanged(
            ThreadStatusChangedNotification {
                thread_id: thread_id.clone(),
                status,
            },
        ));
        observed.push(shell.status.clone());
    }

    assert_eq!(observed, ["ready", "error", "thinking", "waiting"]);
}

#[tokio::test]
async fn remote_archive_can_be_unarchived_and_resumed() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    let thread_id = shell.thread_id;
    shell
        .session_list
        .replace_threads(vec![thread_fixture(thread_id, Some("current"), "current")]);
    let mut backend =
        RecordingBackend::with_threads(vec![thread_fixture(thread_id, Some("current"), "current")]);

    shell
        .handle_app_server_event(
            &mut backend,
            AppServerEvent::ServerNotification(ServerNotification::ThreadArchived(
                ThreadArchivedNotification {
                    thread_id: thread_id.to_string(),
                },
            )),
        )
        .await
        .expect("archive notification should be handled");
    assert_eq!(shell.session_list.selected_thread_id(), None);
    assert_eq!(shell.session_unavailable_reason, Some("archived remotely"));

    shell
        .handle_app_server_event(
            &mut backend,
            AppServerEvent::ServerNotification(ServerNotification::ThreadUnarchived(
                ThreadUnarchivedNotification {
                    thread_id: thread_id.to_string(),
                },
            )),
        )
        .await
        .expect("unarchive notification should be handled");
    finish_session_hydration(&mut shell, &backend).await;
    assert_eq!(shell.session_list.selected_thread_id(), Some(thread_id));

    shell.resume_session(&config, &backend, thread_id);
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.session_unavailable_reason, None);
    assert!(
        backend
            .calls()
            .contains(&RecordedBackendCall::Resume(thread_id))
    );
}

#[tokio::test]
async fn remote_delete_closes_active_session_and_blocks_submission() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.active_turn_id = Some("turn-stale".to_string());
    let deleted_id = shell.thread_id;
    let remaining_id = test_thread_id("01900000-0000-7000-8000-000000000902");
    let mut backend = RecordingBackend::with_threads(vec![thread_fixture(
        remaining_id,
        Some("remaining"),
        "available session",
    )]);

    shell
        .handle_app_server_event(
            &mut backend,
            AppServerEvent::ServerNotification(ServerNotification::ThreadDeleted(
                ThreadDeletedNotification {
                    thread_id: deleted_id.to_string(),
                },
            )),
        )
        .await
        .expect("delete notification should be handled");
    finish_session_hydration(&mut shell, &backend).await;
    assert_eq!(
        (
            shell.session_unavailable_reason,
            shell.active_turn_id.as_deref(),
            shell.dashboard_route,
            shell.session_list.focused,
            shell.session_list.selected_thread_id(),
        ),
        (
            Some("deleted remotely"),
            None,
            DashboardRoute::Sessions,
            true,
            Some(remaining_id),
        )
    );
    insta::assert_snapshot!(
        "remote_deleted_active_session",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28
            )
        )
    );

    shell.submit_prompt(&backend, "must not submit".to_string());
    assert!(!backend.calls().iter().any(|call| matches!(
        call,
        RecordedBackendCall::TurnStart { thread_id, .. } if *thread_id == deleted_id
    )));
}

#[test]
fn remote_close_marks_the_active_session_unavailable() {
    let mut shell = ShellState::snapshot_fixture();
    let thread_id = shell.thread_id.to_string();
    shell.handle_notification(ServerNotification::ThreadClosed(ThreadClosedNotification {
        thread_id,
    }));

    assert_eq!(shell.session_unavailable_reason, Some("closed remotely"));
}

#[test]
fn permission_profile_update_refreshes_status_dashboard_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;

    shell.handle_notification(ServerNotification::ThreadSettingsUpdated(
        ThreadSettingsUpdatedNotification {
            thread_id: shell.thread_id.to_string(),
            thread_settings: codex_app_server_protocol::ThreadSettings {
                cwd: test_absolute_path("workspace/locked"),
                approval_policy: codex_app_server_protocol::AskForApproval::Never,
                approvals_reviewer: codex_app_server_protocol::ApprovalsReviewer::User,
                sandbox_policy: codex_app_server_protocol::SandboxPolicy::DangerFullAccess,
                active_permission_profile: Some(
                    codex_app_server_protocol::ActivePermissionProfile::new(":full"),
                ),
                model: "gpt-5.4".to_string(),
                model_provider: "openai".to_string(),
                service_tier: None,
                effort: None,
                summary: None,
                collaboration_mode: *collaboration_mode_fixture("gpt-5.4", None),
                multi_agent_mode: Default::default(),
                personality: None,
            },
        },
    ));

    assert_eq!(
        (&shell.permission_profile, &shell.active_permission_profile,),
        (
            &codex_protocol::models::PermissionProfile::Disabled,
            &Some(codex_protocol::models::ActivePermissionProfile::new(
                ":full"
            )),
        )
    );
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
        ),
    ));
}

#[tokio::test]
async fn current_time_request_resolves_without_disturbing_pending_approval() {
    let mut shell = ShellState::snapshot_fixture();
    shell.pending_approval = PendingApproval::from_request(&command_approval_request())
        .expect("approval request should be valid");
    shell.status = "thinking".to_string();
    let mut backend = RecordingBackend::default();
    let before = chrono::Utc::now().timestamp();

    shell
        .handle_app_server_event(
            &mut backend,
            AppServerEvent::ServerRequest(ServerRequest::CurrentTimeRead {
                request_id: RequestId::Integer(47),
                params: CurrentTimeReadParams {
                    thread_id: shell.thread_id.to_string(),
                },
            }),
        )
        .await
        .expect("current time should resolve");
    let after = chrono::Utc::now().timestamp();
    complete_backend_actions(&mut shell, &backend).await;

    assert!(shell.pending_approval.is_some());
    assert_eq!(shell.status, "thinking");
    let resolved = backend
        .resolved_requests
        .lock()
        .expect("resolved requests should lock")
        .clone();
    let [(request_id, result)] = resolved.as_slice() else {
        panic!("expected one resolved current-time request: {resolved:?}");
    };
    assert_eq!(request_id, &RequestId::Integer(47));
    let response: CurrentTimeReadResponse =
        serde_json::from_value(result.clone()).expect("current-time response should deserialize");
    assert!(
        (before..=after).contains(&response.current_time_at),
        "current time {} should be between {before} and {after}",
        response.current_time_at,
    );
}

#[test]
fn renders_multiline_composer_growth_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell
        .composer
        .set_text("alpha\n\nbravo\n\ncharlie\n\ndelta");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn pasted_tabs_use_visible_composer_indentation_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.insert_pasted_text("alpha\tbeta");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    assert_eq!(
        (shell.composer.text(), shell.composer.submission_text()),
        ("alpha\tbeta", "alpha\tbeta".to_string())
    );
    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn oversized_paste_reports_error_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.set_text("draft stays intact");
    shell.insert_pasted_text(&"x".repeat(composer::MAX_COMPOSER_BYTES));
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    assert_eq!(shell.composer.text(), "draft stays intact");
    insta::assert_snapshot!(render_shell(&shell, area));
}

#[tokio::test]
async fn oversized_prompt_is_not_submitted() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell
        .composer
        .set_text("x".repeat(composer::MAX_COMPOSER_BYTES + 1));
    let mut backend = RecordingBackend::default();

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("oversized prompt should be handled");

    assert_eq!(backend.calls(), Vec::new());
    assert_eq!(
        shell.composer.text().len(),
        composer::MAX_COMPOSER_BYTES + 1
    );
    let expected = composer::input_too_large_message(composer::MAX_COMPOSER_BYTES + 1);
    assert_eq!(
        shell.transcript.back().map(|line| line.text.as_str()),
        Some(expected.as_str())
    );
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
            /*name*/ None,
            "Investigate approval rendering regression",
        ),
    ]);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 32,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[tokio::test]
async fn session_delete_requires_confirmation_and_shows_spawned_descendants_snapshot() {
    let config = test_config().await;
    let root_id = test_thread_id("01900000-0000-7000-8000-000000000511");
    let child_id = test_thread_id("01900000-0000-7000-8000-000000000512");
    let grandchild_id = test_thread_id("01900000-0000-7000-8000-000000000513");
    let root = thread_fixture(root_id, Some("Delete this investigation"), "root");
    let mut child = thread_fixture(child_id, Some("spawned worker"), "child");
    child.parent_thread_id = Some(root_id.to_string());
    let mut grandchild = thread_fixture(grandchild_id, Some("nested worker"), "grandchild");
    grandchild.parent_thread_id = Some(child_id.to_string());
    let mut backend = RecordingBackend::with_threads(vec![root.clone(), child, grandchild]);
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    shell.session_list.replace_threads(vec![root]);

    shell
        .handle_session_list_key(key_char('d'), &config, &mut backend)
        .await
        .expect("delete confirmation should open");
    complete_backend_actions(&mut shell, &backend).await;

    assert!(shell.pending_session_delete.is_some());
    assert!(
        !backend
            .calls()
            .iter()
            .any(|call| matches!(call, RecordedBackendCall::Delete(_)))
    );
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28
        )
    ));

    shell
        .handle_session_delete_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("delete should cancel");
    assert_eq!(shell.pending_session_delete, None);
    assert!(
        !backend
            .calls()
            .iter()
            .any(|call| matches!(call, RecordedBackendCall::Delete(_)))
    );
}

#[test]
fn renders_loading_archived_session_list_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Sessions;
    shell.session_list.focused = true;
    shell.session_list.replace_threads(vec![thread_fixture(
        test_thread_id("01900000-0000-7000-8000-000000000503"),
        Some("active session"),
        "should disappear while archived sessions load",
    )]);
    shell.session_list.set_error("stale active-session error");
    shell.session_list.toggle_archived();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24,
    );

    let rendered = render_shell(&shell, area);

    assert!(rendered.contains("ARCHIVED  0 sessions"), "{rendered}");
    assert!(rendered.contains("loading sessions"), "{rendered}");
    assert!(
        !rendered.contains("stale active-session error"),
        "{rendered}"
    );
    assert!(!rendered.contains("no matching sessions"), "{rendered}");
    insta::assert_snapshot!(rendered);
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

    assert!(rendered.contains("4/10 Session 04"), "{rendered}");
    assert!(rendered.contains("8/10 Session 08"), "{rendered}");
    assert!(!rendered.contains("1/10 Session 01"), "{rendered}");
    insta::assert_snapshot!(rendered);
}

#[test]
fn renders_scrolled_transcript_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.push_status("first checkpoint");
    shell.push_status("second checkpoint");
    shell.push_status("third checkpoint");
    shell.push_status("fourth checkpoint");
    shell.scroll_transcript_up(/*rows*/ 4);
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

    assert_eq!(ShellView { shell: &shell }.input_area(area).width, 78);
    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_running_status_spinner_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = Some("turn-spinner".to_string());
    shell.status_spinner_frame = 2;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn status_spinner_only_runs_during_active_codex_work() {
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = Some("turn-spinner".to_string());
    let running_states = ["thinking", "reasoning", "retrying", "waiting"].map(|status| {
        shell.status = status.to_string();
        shell.status_spinner_active()
    });
    shell.status = "ready".to_string();
    let ready = shell.status_spinner_active();
    shell.animations = false;
    shell.status = "thinking".to_string();
    let animations_disabled = shell.status_spinner_active();

    assert_eq!(
        (running_states, ready, animations_disabled),
        ([true; 4], false, false)
    );
}

fn retrying_error(shell: &ShellState, turn_id: &str) -> ServerNotification {
    ServerNotification::Error(ErrorNotification {
        error: TurnError {
            message: "stream disconnected".to_string(),
            codex_error_info: None,
            additional_details: None,
        },
        will_retry: true,
        thread_id: shell.thread_id.to_string(),
        turn_id: turn_id.to_string(),
    })
}

#[test]
fn active_turn_progress_recovers_retrying_status() {
    let fixture = ShellState::snapshot_fixture();
    let thread_id = fixture.thread_id.to_string();
    let progress = [
        ServerNotification::AgentMessageDelta(
            codex_app_server_protocol::AgentMessageDeltaNotification {
                thread_id: thread_id.clone(),
                turn_id: "turn-active".to_string(),
                item_id: "assistant-1".to_string(),
                delta: "Recovered.".to_string(),
            },
        ),
        ServerNotification::PlanDelta(codex_app_server_protocol::PlanDeltaNotification {
            thread_id: thread_id.clone(),
            turn_id: "turn-active".to_string(),
            item_id: "plan-1".to_string(),
            delta: "Recovered plan".to_string(),
        }),
        ServerNotification::ItemStarted(ItemStartedNotification {
            thread_id: thread_id.clone(),
            turn_id: "turn-active".to_string(),
            started_at_ms: 0,
            item: ThreadItem::AgentMessage {
                id: "assistant-1".to_string(),
                text: String::new(),
                phase: None,
                memory_citation: None,
            },
        }),
        ServerNotification::ItemCompleted(ItemCompletedNotification {
            thread_id,
            turn_id: "turn-active".to_string(),
            completed_at_ms: 1,
            item: ThreadItem::ContextCompaction {
                id: "compaction-1".to_string(),
            },
        }),
    ];

    for notification in progress {
        let mut shell = ShellState::snapshot_fixture();
        shell.active_turn_id = Some("turn-active".to_string());
        shell.handle_notification(retrying_error(&shell, "turn-active"));
        assert_eq!(shell.status, "retrying");

        shell.handle_notification(notification);

        assert_eq!(shell.status, "thinking");
    }

    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = Some("turn-active".to_string());
    shell.handle_notification(retrying_error(&shell, "turn-active"));
    shell.handle_notification(ServerNotification::AgentMessageDelta(
        codex_app_server_protocol::AgentMessageDeltaNotification {
            thread_id: shell.thread_id.to_string(),
            turn_id: "turn-stale".to_string(),
            item_id: "assistant-stale".to_string(),
            delta: "Stale response".to_string(),
        },
    ));

    assert_eq!(shell.status, "retrying");
}

#[test]
fn renders_recovered_retry_status_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.transcript.clear();
    shell.clear_streaming_transcript();
    shell.active_turn_id = Some("turn-active".to_string());
    shell.status_spinner_frame = 2;
    shell.handle_notification(retrying_error(&shell, "turn-active"));
    shell.handle_notification(ServerNotification::AgentMessageDelta(
        codex_app_server_protocol::AgentMessageDeltaNotification {
            thread_id: shell.thread_id.to_string(),
            turn_id: "turn-active".to_string(),
            item_id: "assistant-1".to_string(),
            delta: "Recovered response.".to_string(),
        },
    ));

    assert_eq!(shell.status, "thinking");
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 16,
        ),
    ));
}

#[test]
fn renders_compact_dashboard_overlay_snapshot() {
    let shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 48, /*height*/ 24,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_terminal_too_narrow_snapshot() {
    let shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 39, /*height*/ 16,
    );
    let view = ShellView { shell: &shell };
    let rendered = render_shell(&shell, area);

    assert_eq!(
        (
            view.cursor_position(area),
            view.input_area(area),
            view.pointer_pane_at(area, Position::new(/*x*/ 1, /*y*/ 1)),
        ),
        (None, Rect::default(), None)
    );
    assert!(rendered.contains("Use a larger terminal window."));
    assert!(!rendered.contains("CONVERSATION"));
    insta::assert_snapshot!(rendered);
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
fn renders_output_blocks_as_inset_status_rectangles() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.push_tool_with_status("exec cargo test", ToolBlockStatus::Success);
    shell.push_output_with_status(
        "line 0\nline 1\n\u{1b}[31mline 2\u{1b}[0m\nline 3\twide\nline 4\rline 5\nline 6\nline 7",
        ToolBlockStatus::Success,
    );
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 32,
    );

    let buf = render_shell_buffer(&shell, area);
    let rendered = buffer_contents(&buf, area);
    let tool_row =
        row_containing(&buf, area, "tool exec cargo test").expect("tool row should render");
    let output_row = row_containing(&buf, area, "output ... 4 earlier output lines")
        .expect("output omission row should render");
    let output_tail_row =
        row_containing(&buf, area, "line 7").expect("latest output row should render");
    let tool_accent_x =
        accent_x_for_row(&buf, area, tool_row).expect("tool row should have an accent");
    let output_accent_x =
        accent_x_for_row(&buf, area, output_row).expect("output row should have an accent");

    assert_eq!(output_tail_row, output_row + 3);
    assert!(!rendered.contains("[31m"));
    assert!(!rendered.contains("line 1"));
    assert!(rendered.contains("line 5"));
    assert!(rendered.contains("line 6"));
    assert!(rendered.contains("line 7"));
    assert_eq!(output_accent_x, tool_accent_x + 2);
    assert_eq!(
        rightmost_bg_x_for_row(&buf, area, tool_row, design::palette::SURFACE),
        rightmost_bg_x_for_row(&buf, area, output_row, design::palette::DARK),
    );
    assert_eq!(
        rightmost_bg_x_for_row(&buf, area, output_tail_row, design::palette::DARK),
        rightmost_bg_x_for_row(&buf, area, output_row, design::palette::DARK),
    );
    assert_eq!(
        buf.cell((output_accent_x, output_row))
            .expect("output accent cell should exist")
            .style()
            .fg,
        Some(design::palette::SUCCESS)
    );
    assert_eq!(
        buf.cell((tool_accent_x, tool_row))
            .expect("tool accent cell should exist")
            .style()
            .fg,
        Some(design::palette::SUCCESS)
    );
    let output_label_x =
        row_needle_x(&buf, area, output_row, "output").expect("output label should render");
    assert_eq!(
        buf.cell((output_label_x, output_row))
            .expect("output label cell should exist")
            .style()
            .fg,
        Some(design::palette::TEXT)
    );

    shell.pointer_position = Some(Position::new(output_accent_x, output_row));
    let hovered = render_shell_buffer(&shell, area);
    assert_eq!(
        rightmost_bg_x_for_row(&hovered, area, output_row, design::palette::BORDER),
        rightmost_bg_x_for_row(&buf, area, output_row, design::palette::DARK)
    );
    assert_eq!(
        rightmost_bg_x_for_row(&hovered, area, output_tail_row, design::palette::BORDER),
        rightmost_bg_x_for_row(&buf, area, output_tail_row, design::palette::DARK)
    );
    insta::assert_snapshot!(rendered);
}

#[test]
fn output_transcript_blocks_use_status_accent_colors() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.push_output_with_status("running output", ToolBlockStatus::Running);
    shell.push_output_with_status("successful output", ToolBlockStatus::Success);
    shell.push_output_with_status("failed output", ToolBlockStatus::Fail);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
    );

    let buf = render_shell_buffer(&shell, area);

    assert_eq!(
        accent_color_for_row(&buf, area, "running output"),
        Some(design::palette::CYAN)
    );
    assert_eq!(
        accent_color_for_row(&buf, area, "successful output"),
        Some(design::palette::SUCCESS)
    );
    assert_eq!(
        accent_color_for_row(&buf, area, "failed output"),
        Some(design::palette::ERROR)
    );
}

#[test]
fn renders_compacted_long_output_block_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    let output = (0..=TRANSCRIPT_OUTPUT_HIGH_WATER_LINES)
        .map(|line| format!("cargo build output line {line:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    shell.push_output_with_status(output, ToolBlockStatus::Running);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 20,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_workspace_roots_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
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
    shell.dashboard_route = DashboardRoute::Status;
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
fn renders_status_route_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
    shell.active_goal = Some(test_thread_goal(
        &shell.thread_id,
        ThreadGoalStatus::Active,
        "Complete the dashboard consolidation",
    ));
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
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 60,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn dashboard_routes_keep_session_and_status_panels_separate() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Sessions;
    let panels = dashboard::dashboard_panels(&shell, /*width*/ 80);

    assert_eq!(
        panels
            .iter()
            .map(|panel| panel.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Navigation", "Sessions", "Thread"]
    );

    shell.dashboard_route = DashboardRoute::Status;
    shell.active_goal = Some(test_thread_goal(
        &shell.thread_id,
        ThreadGoalStatus::Active,
        "Keep route ownership explicit",
    ));
    let panels = dashboard::dashboard_panels(&shell, /*width*/ 80);

    assert_eq!(
        panels
            .iter()
            .map(|panel| panel.title.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Navigation",
            "Settings",
            "Goal",
            "Plan",
            "Tools",
            "Edits",
            "Workspace",
            "Tokens",
        ]
    );

    shell.dashboard_route = DashboardRoute::Help;
    let panels = dashboard::dashboard_panels(&shell, /*width*/ 80);

    assert_eq!(
        panels
            .iter()
            .map(|panel| panel.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Navigation", "Keys"]
    );
}

#[test]
fn renders_model_runtime_details_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
    shell.settings.focused = true;
    shell.model = "gpt-5.6-sol".to_string();
    shell.reasoning_effort = Some(ReasoningEffort::Max);
    shell.service_tier = Some("priority".to_string());
    shell.tui_theme = Some("dracula".to_string());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_model_availability_nux_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.streaming_assistant.clear();
    shell.push_system(
        "Our most capable model yet. GPT-5.6 Sol can tackle complex code changes, dig into research, produce polished documents, and take on your most ambitious work. Sol is highly capable at lower reasoning efforts—try starting lower, then turn it up for harder jobs.",
    );
    assert_eq!(
        shell.transcript.back().map(|line| line.kind),
        Some(TranscriptKind::System)
    );
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_settings_pages_validation_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
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
    shell.dashboard_route = DashboardRoute::Status;
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
    shell.dashboard_route = DashboardRoute::Status;
    shell.token_usage = TokenUsage {
        input_tokens: 260_000,
        cached_input_tokens: 40_000,
        output_tokens: 20_000,
        reasoning_output_tokens: 8_000,
        total_tokens: 280_000,
    };
    shell.context_token_usage = shell.token_usage.clone();
    shell.model_context_window = Some(372_000);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 42,
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

    let rendered = render_shell(&shell, area);

    assert!(!rendered.contains("◆ STATUS"), "{rendered}");
    insta::assert_snapshot!(rendered);
}

#[test]
fn renders_goal_progress_in_dashboard_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
    shell.active_goal = Some(test_thread_goal(
        &shell.thread_id,
        ThreadGoalStatus::Active,
        "Complete the unchecked dashboard progress item",
    ));
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 34,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn narrow_dashboard_truncates_long_plan_lines_without_clipping_steps_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
    shell.plan_explanation = Some(
        "This deliberately long plan explanation must stay on one visual dashboard row so every later plan step keeps its measured position."
            .to_string(),
    );
    shell.plan_steps = vec![
        codex_app_server_protocol::TurnPlanStep {
            step: "Inspect measurement".to_string(),
            status: codex_app_server_protocol::TurnPlanStepStatus::Completed,
        },
        codex_app_server_protocol::TurnPlanStep {
            step: "Truncate styled lines".to_string(),
            status: codex_app_server_protocol::TurnPlanStepStatus::Completed,
        },
        codex_app_server_protocol::TurnPlanStep {
            step: "Preserve hit targets".to_string(),
            status: codex_app_server_protocol::TurnPlanStepStatus::InProgress,
        },
        codex_app_server_protocol::TurnPlanStep {
            step: "Verify narrow rendering".to_string(),
            status: codex_app_server_protocol::TurnPlanStepStatus::Pending,
        },
        codex_app_server_protocol::TurnPlanStep {
            step: "Final plan row remains visible".to_string(),
            status: codex_app_server_protocol::TurnPlanStepStatus::Pending,
        },
    ];
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 36,
    );

    let buf = render_shell_buffer(&shell, area);
    let rendered = buffer_contents(&buf, area);
    let explanation_row = row_containing(&buf, area, "This deliberately long plan explanation")
        .expect("truncated plan explanation should render");
    let ellipsis_x = (area.x..area.right())
        .find(|x| buf[(*x, explanation_row)].symbol() == "…")
        .expect("long plan explanation should end with an ellipsis");

    assert!(
        rendered.contains("Final plan row remains visible"),
        "{rendered}"
    );
    assert!(
        buf[(ellipsis_x, explanation_row)]
            .style()
            .add_modifier
            .contains(Modifier::DIM),
        "the ellipsis should preserve the explanation style"
    );
    insta::assert_snapshot!(rendered);
}

#[test]
fn renders_active_turn_key_hints_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = Some("turn-active-1234567890".to_string());
    shell.dashboard_route = DashboardRoute::Help;
    shell.composer.clear();
    queue_messages(
        &mut shell.composer,
        &["first queued follow-up", "second queued follow-up"],
    );
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 44,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_queued_message_editor_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Help;
    shell.composer.clear();
    queue_messages(
        &mut shell.composer,
        &["first queued follow-up", "second queued follow-up"],
    );
    assert!(shell.composer.edit_previous_queued_message());
    shell.composer.insert_str(" with edits");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 44,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn dashboard_shortcut_guides_only_appear_on_help_route() {
    let mut shell = ShellState::snapshot_fixture();
    let shortcut_guides = [
        "ctrl+1 focus",
        "r resume",
        "f fork",
        "u unarchive",
        "a archive",
        "d delete",
        "n rename",
        "/ search",
        "v archived",
        "esc composer",
        "ctrl+3 focus",
        "Enter edit/cycle",
        "Tab page",
    ];
    let centralized_guides = [
        "Ctrl+1 Status  Ctrl+2 Agents",
        "Ctrl+3 Sessions Ctrl+4 Help",
        "Ctrl+N new, mouse click rows",
        "Sessions: Enter focus, j/k move",
        "r resume, f fork, a/u archive",
        "v archived, d delete",
        "n rename, / search",
        "Status: Tab page, Enter select",
        "Selectors: j/k choose, Enter apply",
        "Esc twice to exit",
    ];
    let mut leaked_guides = Vec::new();

    let visibility = DashboardRoute::ALL.map(|route| {
        shell.dashboard_route = route;
        let panels = dashboard::dashboard_panels(&shell, /*width*/ 80);
        let has_keys = panels.iter().any(|panel| panel.title == "Keys");
        let has_route_shortcut = panels
            .iter()
            .find(|panel| panel.title == "Keys")
            .into_iter()
            .flat_map(|panel| &panel.lines)
            .flat_map(|line| &line.spans)
            .any(|span| span.content.contains("Cmd arrows/⌫"));
        if route != DashboardRoute::Help {
            let text = panels
                .iter()
                .flat_map(|panel| &panel.lines)
                .flat_map(|line| &line.spans)
                .map(|span| span.content.as_ref())
                .collect::<String>();
            leaked_guides.extend(
                shortcut_guides
                    .iter()
                    .filter(|guide| text.contains(**guide))
                    .map(|guide| (route, *guide)),
            );
        }
        (route, has_keys, has_route_shortcut)
    });

    assert_eq!(leaked_guides, Vec::new());
    assert_eq!(
        visibility,
        [
            (DashboardRoute::Status, false, false),
            (DashboardRoute::Agents, false, false),
            (DashboardRoute::Sessions, false, false),
            (DashboardRoute::Help, true, true),
        ]
    );
    let help_text = dashboard::dashboard_panels(&shell, /*width*/ 80)
        .into_iter()
        .flat_map(|panel| panel.lines)
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<String>();
    assert_eq!(
        centralized_guides.map(|guide| help_text.contains(guide)),
        [true; 10]
    );
    let active_session_lines = shell
        .session_list
        .lines(/*width*/ 40)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    shell.session_list.toggle_archived();
    let archived_session_lines = shell
        .session_list
        .lines(/*width*/ 40)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();
    assert_eq!(
        (active_session_lines, archived_session_lines),
        (
            vec![
                "○ CLICK TO FOCUS  ACTIVE  0 sessions".to_string(),
                "+ New session  Ctrl+N".to_string(),
                "loading sessions".to_string(),
            ],
            vec![
                "○ CLICK TO FOCUS  ARCHIVED  0 sessions".to_string(),
                "loading sessions".to_string(),
            ],
        )
    );
    assert_eq!(
        shell
            .settings
            .lines(&shell.settings_view(), /*width*/ 40)
            .first()
            .map(line_text),
        Some("  Model  │Permis...│Appear...│Integra...".to_string())
    );
}

#[test]
fn dashboard_shortcut_guides_fit_layout_boundaries() {
    let shell = ShellState::snapshot_fixture();
    let mut selecting = ShellState::snapshot_fixture();
    selecting.select_latest_transcript_item();

    for shell in [&shell, &selecting] {
        for width in [38, 40, 48, 54, 55, 58, 71, 72, 80] {
            for line in super::dashboard_help::key_hint_lines(shell, width) {
                assert!(
                    line.width() <= width,
                    "shortcut help exceeds width {width}: {line}"
                );
            }
        }
    }
}

#[test]
fn dashboard_shortcut_label_variants_snapshot() {
    let shell = ShellState::snapshot_fixture();
    let rendered = |panel_width| {
        super::dashboard_help::key_hint_lines(&shell, panel_width)
            .into_iter()
            .map(|line| line.to_string())
            .join("\n")
    };

    insta::assert_snapshot!(
        "compact_dashboard_shortcut_labels",
        rendered(/*panel_width*/ 60)
    );
    insta::assert_snapshot!(
        "wide_dashboard_shortcut_labels",
        rendered(/*panel_width*/ 80)
    );
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
fn short_command_palette_keeps_the_selection_visible() {
    let mut shell = ShellState::snapshot_fixture();
    shell.open_command_palette();
    let entries = shell.command_palette_entries();
    shell
        .command_palette
        .as_mut()
        .expect("command palette should be open")
        .select_last(&entries);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 12,
    );
    let rendered = render_shell(&shell, area);
    let compact = rendered_text_position(&rendered, "Compact context");

    assert_eq!(
        ShellView { shell: &shell }.command_palette_entry_at(area, compact),
        Some(entries.len().saturating_sub(1))
    );
    insta::assert_snapshot!("short_command_palette", rendered);
}

#[tokio::test]
async fn command_palette_ignores_inside_chrome_and_closes_on_outside_click() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    shell.open_command_palette();
    let panel = command_palette_view::palette_area(area, shell.command_palette_entries().len());
    let content = design::pane_content_rect(panel);
    let title = rendered_text_position(&render_shell(&shell, area), "ACTIONS");
    let blank_row = Position::new(content.x, content.y.saturating_add(1));

    for position in [title, blank_row] {
        shell
            .handle_mouse_click(area, position, &config, &mut backend)
            .await
            .expect("inside palette click should succeed");
        assert!(shell.command_palette.is_some());
    }

    shell
        .handle_mouse_click(
            area,
            Position::new(panel.x.saturating_sub(1), panel.y),
            &config,
            &mut backend,
        )
        .await
        .expect("outside palette click should succeed");

    assert!(shell.command_palette.is_none());
    assert_eq!(backend.calls(), Vec::new());
}

#[test]
fn renders_sessions_dashboard_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Sessions;
    shell.session_list.focused = true;
    shell.session_list.replace_threads(vec![
        thread_fixture(
            test_thread_id("01900000-0000-7000-8000-000000000011"),
            Some("Tokyo Night polish"),
            "Refining the application shell and mouse interactions",
        ),
        thread_fixture(
            test_thread_id("01900000-0000-7000-8000-000000000012"),
            Some("Agent activity"),
            "Building a bounded subagent inspector",
        ),
    ]);

    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 112, /*height*/ 30
        )
    ));
}

#[test]
fn session_rows_use_display_width_for_wide_characters() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Sessions;
    shell.session_list.replace_threads(vec![
        thread_fixture(
            test_thread_id("01900000-0000-7000-8000-000000000021"),
            Some("東京 UI"),
            "鮮やかなテーマを確認する",
        ),
        thread_fixture(
            test_thread_id("01900000-0000-7000-8000-000000000022"),
            Some("検証 🌙"),
            "マウスとキーボードのフロー",
        ),
    ]);
    let width = 46;
    let lines = shell.session_list.lines(width);

    assert_eq!(
        lines
            .iter()
            .map(line_text)
            .filter(|line| line.contains("東京 UI") || line.contains("検証 🌙"))
            .count(),
        2
    );
    assert!(
        lines.iter().all(|line| {
            unicode_width::UnicodeWidthStr::width(line_text(line).as_str()) <= width
        })
    );
}

#[test]
fn renders_agents_dashboard_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Agents;
    shell.agents_focused = true;
    for (thread_id, path) in [
        ("research", "/root/research"),
        ("visual", "/root/research/visual"),
        ("testing", "/root/testing"),
        ("failure", "/root/testing/failure"),
    ] {
        shell
            .agent_activity
            .reduce_completed(&ThreadItem::SubAgentActivity {
                id: format!("activity-{thread_id}"),
                kind: SubAgentActivityKind::Started,
                agent_thread_id: thread_id.to_string(),
                agent_path: path.to_string(),
            });
    }
    shell
        .agent_activity
        .reduce_completed(&ThreadItem::CollabAgentToolCall {
            id: "spawn-agents".to_string(),
            tool: CollabAgentTool::SpawnAgent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: "root-thread".to_string(),
            receiver_thread_ids: vec![
                "research".to_string(),
                "visual".to_string(),
                "testing".to_string(),
                "failure".to_string(),
            ],
            prompt: Some("Review the new TUI flow and visual hierarchy.".to_string()),
            model: Some("gpt-5-codex".to_string()),
            reasoning_effort: Some(ReasoningEffort::High),
            agents_states: [
                ("research", CollabAgentStatus::Running, "Reviewing layout"),
                (
                    "visual",
                    CollabAgentStatus::Completed,
                    "Visual audit complete",
                ),
                (
                    "testing",
                    CollabAgentStatus::Interrupted,
                    "Stopped after review",
                ),
                ("failure", CollabAgentStatus::Errored, "Compact flow failed"),
            ]
            .into_iter()
            .map(|(thread_id, status, message)| {
                (
                    thread_id.to_string(),
                    CollabAgentState {
                        status,
                        message: Some(message.to_string()),
                    },
                )
            })
            .collect(),
        });
    shell.agent_activity.select_thread("failure");

    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 112, /*height*/ 30
        )
    ));
    insta::assert_snapshot!(
        "compact_agents_dashboard",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24
            )
        )
    );
}

#[tokio::test]
async fn agent_log_loads_complete_history_beyond_inspector_caps() {
    let config = test_config().await;
    let child_id = test_thread_id("01900000-0000-7000-8000-000000000091");
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Agents;
    shell.agents_focused = true;
    shell.composer.clear();
    shell
        .agent_activity
        .reduce_completed(&ThreadItem::SubAgentActivity {
            id: "child-started".to_string(),
            kind: SubAgentActivityKind::Started,
            agent_thread_id: child_id.to_string(),
            agent_path: "/root/full-log".to_string(),
        });

    let mut child = thread_fixture(child_id, /*name*/ None, "child log");
    child.turns = (0..13)
        .map(|index| {
            let mut turn = test_turn(&format!("child-turn-{index}"), TurnStatus::Completed);
            let text = if index == 0 {
                "earliest history sentinel".to_string()
            } else if index == 12 {
                format!("long history {} final suffix sentinel", "x".repeat(700))
            } else {
                format!("middle history item {index}")
            };
            turn.items.push(ThreadItem::AgentMessage {
                id: format!("child-message-{index}"),
                text,
                phase: None,
                memory_citation: None,
            });
            if index == 12 {
                turn.items.extend([
                    ThreadItem::AgentMessage {
                        id: "full-log-git-action".to_string(),
                        text: "::git-stage{cwd=\"/workspace/better-codex\"}".to_string(),
                        phase: None,
                        memory_citation: None,
                    },
                    ThreadItem::FileChange {
                        id: "full-log-file-change".to_string(),
                        changes: vec![FileUpdateChange {
                            path: "src/full_log.rs".to_string(),
                            kind: PatchChangeKind::Update { move_path: None },
                            diff: "@@ full diff sentinel @@".to_string(),
                        }],
                        status: codex_app_server_protocol::PatchApplyStatus::Completed,
                    },
                    ThreadItem::McpToolCall {
                        id: "full-log-mcp".to_string(),
                        server: "review-tools".to_string(),
                        tool: "inspect".to_string(),
                        status: codex_app_server_protocol::McpToolCallStatus::Completed,
                        arguments: json!({"path": "full argument sentinel"}),
                        app_context: None,
                        mcp_app_resource_uri: None,
                        plugin_id: None,
                        result: Some(Box::new(codex_app_server_protocol::McpToolCallResult {
                            content: vec![json!({"text": "full result sentinel"})],
                            structured_content: None,
                            meta: None,
                        })),
                        error: None,
                        duration_ms: Some(42),
                    },
                ]);
                turn.status = TurnStatus::Failed;
                turn.error = Some(TurnError {
                    message: "agent failure sentinel".to_string(),
                    codex_error_info: None,
                    additional_details: Some("detailed failure sentinel".to_string()),
                });
            }
            turn
        })
        .collect();
    let mut backend = RecordingBackend::with_threads(vec![child]);

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("opening the selected agent log should succeed");
    assert!(shell.has_pending_agent_log());
    for _ in 0..10 {
        tokio::task::yield_now().await;
        if shell.poll_agent_log().await {
            break;
        }
    }

    let log = shell.agent_log.as_ref().expect("agent log should be open");
    assert!(!log.is_loading());
    assert_eq!(log.error(), None);
    let rendered = log.lines().iter().map(line_text).join("\n");
    assert!(rendered.contains("earliest history sentinel"));
    assert!(rendered.contains("final suffix sentinel"));
    assert!(rendered.contains(&"x".repeat(700)));
    assert!(rendered.contains("full diff sentinel"));
    assert!(rendered.contains("full argument sentinel"));
    assert!(rendered.contains("full result sentinel"));
    assert!(rendered.contains("::git-stage"));
    assert!(rendered.contains("agent failure sentinel"));
    assert!(rendered.contains("detailed failure sentinel"));
    assert!(
        backend
            .calls()
            .contains(&RecordedBackendCall::ThreadReadFull(child_id))
    );

    let other_child_id = test_thread_id("01900000-0000-7000-8000-000000000094");
    shell
        .agent_activity
        .reduce_completed(&ThreadItem::SubAgentActivity {
            id: "other-child-started".to_string(),
            kind: SubAgentActivityKind::Started,
            agent_thread_id: other_child_id.to_string(),
            agent_path: "/root/other-log".to_string(),
        });
    assert!(
        shell
            .agent_activity
            .select_thread(&other_child_id.to_string())
    );
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("reloading the agent log should succeed");
    assert!(shell.has_pending_agent_log());
    assert_eq!(
        backend
            .calls()
            .into_iter()
            .filter(|call| {
                matches!(
                    call,
                    RecordedBackendCall::ThreadReadFull(thread_id) if *thread_id == child_id
                )
            })
            .count(),
        2
    );
    assert!(
        backend
            .calls()
            .iter()
            .all(|call| !matches!(call, RecordedBackendCall::ThreadReadFull(thread_id) if *thread_id == other_child_id))
    );
}

#[tokio::test]
async fn agent_log_reports_full_history_read_failure() {
    let config = test_config().await;
    let child_id = test_thread_id("01900000-0000-7000-8000-000000000092");
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Agents;
    shell.agents_focused = true;
    shell
        .agent_activity
        .reduce_completed(&ThreadItem::SubAgentActivity {
            id: "child-started".to_string(),
            kind: SubAgentActivityKind::Started,
            agent_thread_id: child_id.to_string(),
            agent_path: "/root/missing-log".to_string(),
        });
    let backend = RecordingBackend::default();

    shell.open_selected_agent_log(&config, &backend);
    for _ in 0..10 {
        tokio::task::yield_now().await;
        if shell.poll_agent_log().await {
            break;
        }
    }

    let log = shell
        .agent_log
        .as_ref()
        .expect("error popup should remain open");
    assert!(
        log.error()
            .is_some_and(|error| error.contains("was not found"))
    );
    assert!(log.lines().is_empty());
}

#[tokio::test]
async fn agent_log_rejects_partial_turn_history() {
    let config = test_config().await;
    let child_id = test_thread_id("01900000-0000-7000-8000-000000000093");
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Agents;
    shell.agents_focused = true;
    shell
        .agent_activity
        .reduce_completed(&ThreadItem::SubAgentActivity {
            id: "child-started".to_string(),
            kind: SubAgentActivityKind::Started,
            agent_thread_id: child_id.to_string(),
            agent_path: "/root/partial-log".to_string(),
        });
    let mut child = thread_fixture(child_id, /*name*/ None, "partial log");
    let mut turn = test_turn("summary-turn", TurnStatus::Completed);
    turn.items_view = TurnItemsView::Summary;
    child.turns.push(turn);
    let backend = RecordingBackend::with_threads(vec![child]);

    shell.open_selected_agent_log(&config, &backend);
    for _ in 0..10 {
        tokio::task::yield_now().await;
        if shell.poll_agent_log().await {
            break;
        }
    }

    let log = shell
        .agent_log
        .as_ref()
        .expect("partial-history error should remain visible");
    assert!(
        log.error()
            .is_some_and(|error| error.contains("contains only a summary"))
    );
    assert!(log.lines().is_empty());
}

#[tokio::test]
async fn replacing_session_discards_pending_agent_log_result() {
    let config = test_config().await;
    let child_id = test_thread_id("01900000-0000-7000-8000-000000000095");
    let mut shell = ShellState::snapshot_fixture();
    shell
        .agent_activity
        .reduce_completed(&ThreadItem::SubAgentActivity {
            id: "child-started".to_string(),
            kind: SubAgentActivityKind::Started,
            agent_thread_id: child_id.to_string(),
            agent_path: "/root/stale-log".to_string(),
        });
    let backend = RecordingBackend::with_threads(vec![thread_fixture(
        child_id,
        /*name*/ None,
        "stale agent log",
    )]);

    shell.open_selected_agent_log(&config, &backend);
    assert!(shell.has_pending_agent_log());
    shell.replace_started_session(started_thread(
        "replacement",
        test_thread_id("01900000-0000-7000-8000-000000000096"),
        /*forked_from_id*/ None,
    ));
    tokio::task::yield_now().await;

    assert!(shell.agent_log.is_none());
    assert!(!shell.poll_agent_log().await);
}

#[test]
fn renders_status_dashboard_at_wide_and_compact_sizes() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
    shell.settings.focused = true;

    insta::assert_snapshot!(
        "wide_status",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 112, /*height*/ 30
            )
        )
    );
    insta::assert_snapshot!(
        "compact_status",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24
            )
        )
    );
}

#[test]
fn renders_model_selector_at_wide_and_compact_sizes() {
    let mut shell = ShellState::snapshot_fixture();
    shell.available_models = (0..12)
        .map(|index| {
            model_preset_fixture(
                &format!("gpt-5-{index}"),
                /*show_in_picker*/ true,
                ReasoningEffort::Medium,
                &[ReasoningEffort::Low, ReasoningEffort::Medium],
                &["fast"],
            )
        })
        .collect();
    shell.model = "gpt-5-0".to_string();
    shell.open_model_selector();

    insta::assert_snapshot!(
        "wide_model_selector",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 30
            )
        )
    );
    insta::assert_snapshot!(
        "compact_model_selector",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 48, /*height*/ 16
            )
        )
    );
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
            (CommandPaletteAction::NewSession, true),
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
async fn command_palette_starts_a_new_session() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.open_command_palette();
    select_command_palette_action(&mut shell, CommandPaletteAction::NewSession);
    let mut backend = RecordingBackend::default();

    shell
        .execute_selected_command_palette_action(&config, &mut backend)
        .await
        .expect("new session action should start a session");

    assert_eq!(
        backend.calls().first(),
        Some(&RecordedBackendCall::Start(Some(ThreadStartSource::Clear)))
    );
    assert!(shell.command_palette.is_none());
    assert!(!shell.session_list.focused);
}

#[tokio::test]
async fn command_palette_opens_native_model_and_permissions_settings() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();

    shell.dashboard_scroll.set(8);
    shell.open_command_palette();
    select_command_palette_action(&mut shell, CommandPaletteAction::SwitchModel);
    shell
        .execute_selected_command_palette_action(&config, &mut backend)
        .await
        .expect("model action should open settings");

    assert_eq!(shell.dashboard_route, DashboardRoute::Status);
    assert_eq!(shell.dashboard_scroll.get(), 0);
    assert!(shell.settings.focused);
    assert!(shell.selector.is_some());

    shell.dashboard_scroll.set(8);
    shell.open_command_palette();
    select_command_palette_action(&mut shell, CommandPaletteAction::ChangePermissions);
    shell
        .execute_selected_command_palette_action(&config, &mut backend)
        .await
        .expect("permissions action should open settings");

    assert_eq!(shell.dashboard_route, DashboardRoute::Status);
    assert_eq!(shell.dashboard_scroll.get(), 0);
    assert!(shell.settings.focused);
    assert!(shell.selector.is_some());
    let rendered = render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 32,
        ),
    );
    assert!(
        rendered.contains("Select approval policy"),
        "permissions action should open approval policy selector, got:\n{rendered}"
    );
}

#[tokio::test]
async fn command_palette_opens_native_session_list_for_resume_and_fork() {
    let config = test_config().await;
    let session_id = test_thread_id("01900000-0000-7000-8000-000000000601");
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::with_threads(vec![thread_fixture(
        session_id,
        Some("resume from palette"),
        "palette preview",
    )]);

    shell.dashboard_route = DashboardRoute::Sessions;
    shell.dashboard_scroll.set(8);
    shell.open_command_palette();
    select_command_palette_action(&mut shell, CommandPaletteAction::ResumeThread);
    shell
        .execute_selected_command_palette_action(&config, &mut backend)
        .await
        .expect("resume action should open sessions");
    finish_session_hydration(&mut shell, &backend).await;

    assert_eq!(shell.dashboard_route, DashboardRoute::Sessions);
    assert_eq!(shell.dashboard_scroll.get(), 0);
    assert!(shell.session_list.focused);
    assert!(!shell.settings.focused);
    assert!(shell.session_list.selected_is_current(session_id));
    assert!(
        shell.transcript.iter().any(|line| {
            line.kind == TranscriptKind::Status && line.text == "press r to resume selected session"
        }),
        "resume action should leave a keyboard hint"
    );

    shell.dashboard_scroll.set(8);
    shell.open_command_palette();
    select_command_palette_action(&mut shell, CommandPaletteAction::ForkThread);
    shell
        .execute_selected_command_palette_action(&config, &mut backend)
        .await
        .expect("fork action should open sessions");
    finish_session_hydration(&mut shell, &backend).await;

    assert_eq!(shell.dashboard_route, DashboardRoute::Sessions);
    assert_eq!(shell.dashboard_scroll.get(), 0);
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
                cursor: None,
            },
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: None,
            },
        ]
    );
}

#[tokio::test]
async fn command_palette_opens_external_agent_import_review() {
    let config = test_config().await;
    let items = external_agent_items();
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::with_external_agent_items(items.clone());

    shell.open_command_palette();
    select_command_palette_action(&mut shell, CommandPaletteAction::ImportExternalAgentConfig);
    shell
        .execute_selected_command_palette_action(&config, &mut backend)
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
async fn short_management_modal_keeps_keyboard_selection_visible() {
    let mut shell = ShellState::snapshot_fixture();
    let mut response = plugin_list_response_fixture();
    response.marketplaces[0].plugins = (0..12)
        .map(|index| {
            plugin_summary_fixture(
                &format!("plugin-{index:02}"),
                &format!("Plugin {index:02}"),
                /*installed*/ index % 2 == 0,
                /*enabled*/ index % 2 == 0,
            )
        })
        .collect();
    shell.plugin_catalog = Some(response);
    shell.open_plugin_management();
    let mut backend = RecordingBackend::default();
    for _ in 0..11 {
        shell
            .handle_plugin_management_key(key_char('j'), &mut backend)
            .await
            .expect("plugin selection should move");
    }
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 12,
    );
    let rendered = render_shell(&shell, area);

    assert!(rendered.contains("Plugin 11"));
    insta::assert_snapshot!("short_plugin_management", rendered);
}

#[tokio::test]
async fn interactive_requests_preempt_management_overlays() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::with_external_agent_items(external_agent_items());
    shell
        .start_external_agent_import_review(&mut backend)
        .await
        .expect("external agent review should open");
    assert!(shell.pending_external_agent_import.is_some());

    shell
        .handle_app_server_event(
            &mut backend,
            AppServerEvent::ServerRequest(command_approval_request()),
        )
        .await
        .expect("approval request should preempt the management overlay");

    assert!(shell.pending_approval.is_some());
    assert!(shell.pending_external_agent_import.is_none());
    assert!(shell.command_palette.is_none());
    assert!(shell.selector.is_none());
}

#[tokio::test]
async fn concurrent_interactive_requests_wait_and_resolve_in_order() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.dashboard_visible = false;
    let mut backend = RecordingBackend::default();

    for request in [
        command_approval_request(),
        tool_user_input_request(),
        mcp_url_elicitation_request(),
    ] {
        shell
            .handle_app_server_event(&mut backend, AppServerEvent::ServerRequest(request))
            .await
            .expect("interactive request should be accepted");
    }

    assert!(shell.pending_approval.is_some());
    assert_eq!(
        shell
            .queued_interactive_requests
            .iter()
            .map(PendingInteractiveRequest::request_id)
            .collect::<Vec<_>>(),
        vec![RequestId::Integer(43), RequestId::Integer(45)]
    );
    assert_eq!(backend.calls(), Vec::new());
    insta::assert_snapshot!(
        "queued_interactive_requests",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
            ),
        )
    );

    shell
        .resolve_pending_approval(&backend, /*option_index*/ 0, None)
        .expect("approval should resolve");
    complete_backend_actions(&mut shell, &backend).await;
    assert!(shell.pending_user_input.is_some());
    assert_eq!(shell.queued_interactive_requests.len(), 1);

    shell.composer.set_text("2");
    shell
        .resolve_pending_user_input(&mut backend)
        .await
        .expect("tool input should resolve");
    assert!(shell.pending_elicitation.is_some());
    assert!(shell.queued_interactive_requests.is_empty());

    shell
        .resolve_pending_elicitation(&mut backend, ElicitationChoice::Accept)
        .await
        .expect("elicitation should resolve");

    assert!(shell.pending_approval.is_none());
    assert!(shell.pending_elicitation.is_none());
    assert!(shell.pending_user_input.is_none());
    assert!(shell.queued_interactive_requests.is_empty());
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::Resolve(RequestId::Integer(41)),
            RecordedBackendCall::Resolve(RequestId::Integer(43)),
            RecordedBackendCall::Resolve(RequestId::Integer(45)),
        ]
    );
}

#[tokio::test]
async fn resolved_notifications_remove_only_the_matching_interactive_request() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    for request in [
        tool_user_input_request(),
        command_approval_request(),
        mcp_url_elicitation_request(),
    ] {
        shell
            .handle_app_server_event(&mut backend, AppServerEvent::ServerRequest(request))
            .await
            .expect("interactive request should be accepted");
    }
    shell.composer.set_text("stale answer");

    for request_id in [RequestId::Integer(41), RequestId::Integer(999)] {
        shell.handle_notification(ServerNotification::ServerRequestResolved(
            ServerRequestResolvedNotification {
                thread_id: shell.thread_id.to_string(),
                request_id,
            },
        ));
    }

    assert!(shell.pending_user_input.is_some());
    assert_eq!(shell.composer.text(), "stale answer");
    assert_eq!(
        shell
            .queued_interactive_requests
            .iter()
            .map(PendingInteractiveRequest::request_id)
            .collect::<Vec<_>>(),
        vec![RequestId::Integer(45)]
    );

    shell.handle_notification(ServerNotification::ServerRequestResolved(
        ServerRequestResolvedNotification {
            thread_id: shell.thread_id.to_string(),
            request_id: RequestId::Integer(43),
        },
    ));

    assert!(shell.pending_user_input.is_none());
    assert!(shell.pending_elicitation.is_some());
    assert!(shell.composer.text().is_empty());
    assert!(shell.queued_interactive_requests.is_empty());

    shell.handle_notification(ServerNotification::ServerRequestResolved(
        ServerRequestResolvedNotification {
            thread_id: shell.thread_id.to_string(),
            request_id: RequestId::Integer(45),
        },
    ));

    assert!(shell.pending_approval.is_none());
    assert!(shell.pending_elicitation.is_none());
    assert!(shell.pending_user_input.is_none());
    assert_eq!(backend.calls(), Vec::new());
}

#[tokio::test]
async fn interactive_requests_from_replaced_sessions_are_rejected() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell
        .agent_activity
        .reduce_completed(&ThreadItem::SubAgentActivity {
            id: "historical-agent".to_string(),
            kind: SubAgentActivityKind::Started,
            agent_thread_id: "01900000-0000-7000-8000-000000000099".to_string(),
            agent_path: "/root/historical".to_string(),
        });
    let mut request = command_approval_request();
    let ServerRequest::CommandExecutionRequestApproval { params, .. } = &mut request else {
        panic!("command approval fixture should keep its request type");
    };
    params.thread_id = "01900000-0000-7000-8000-000000000099".to_string();

    shell
        .handle_app_server_event(&mut backend, AppServerEvent::ServerRequest(request))
        .await
        .expect("stale approval should be rejected");

    assert_eq!(shell.pending_approval, None);
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::Reject {
            request_id: RequestId::Integer(41),
            message: "interactive request belongs to inactive thread 01900000-0000-7000-8000-000000000099".to_string(),
        }]
    );
}

#[test]
fn notifications_from_historical_inactive_agents_are_ignored() {
    let mut shell = ShellState::snapshot_fixture();
    let thread_id = "01900000-0000-7000-8000-000000000098";
    shell
        .agent_activity
        .reduce_completed(&ThreadItem::SubAgentActivity {
            id: "historical-agent".to_string(),
            kind: SubAgentActivityKind::Started,
            agent_thread_id: thread_id.to_string(),
            agent_path: "/root/historical".to_string(),
        });
    let before = shell.agent_activity.agent(thread_id).cloned();

    shell.handle_notification(ServerNotification::AgentMessageDelta(
        codex_app_server_protocol::AgentMessageDeltaNotification {
            thread_id: thread_id.to_string(),
            turn_id: "stale-turn".to_string(),
            item_id: "stale-message".to_string(),
            delta: "old session output".to_string(),
        },
    ));

    assert_eq!(shell.agent_activity.agent(thread_id), before.as_ref());
}

#[test]
fn notifications_create_an_authorized_agent_before_history_catches_up() {
    let mut shell = ShellState::snapshot_fixture();
    let thread_id = "01900000-0000-7000-8000-000000000097";
    shell.active_agent_thread_ids.insert(thread_id.to_string());

    shell.handle_notification(ServerNotification::AgentMessageDelta(
        codex_app_server_protocol::AgentMessageDeltaNotification {
            thread_id: thread_id.to_string(),
            turn_id: "live-turn".to_string(),
            item_id: "live-message".to_string(),
            delta: "live output".to_string(),
        },
    ));

    let agent = shell
        .agent_activity
        .agent(thread_id)
        .expect("authorized agent should be created");
    assert_eq!(agent.latest_message.as_deref(), Some("live output"));
    assert_eq!(agent.status, agent_activity::AgentLifecycleStatus::Running);
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

#[tokio::test]
async fn clear_slash_command_resets_visible_transcript_without_submitting_turn() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.streaming_assistant = "streaming".to_string();
    shell.streaming_plan = "plan".to_string();
    shell.composer.set_text("/clear");

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("clear command should be handled locally");

    assert_eq!(
        shell.transcript.iter().cloned().collect::<Vec<_>>(),
        vec![TranscriptLine::new(
            TranscriptKind::System,
            "visible transcript cleared"
        )]
    );
    assert_eq!(shell.streaming_assistant, "");
    assert_eq!(shell.streaming_plan, "");
    assert_eq!(shell.composer.text(), "");
    assert_eq!(shell.transcript_scroll, 0);
    assert_eq!(shell.transcript_selection, None);
    assert_eq!(backend.calls(), Vec::new());

    shell.composer.move_up_or_recall_history();
    assert_eq!(shell.composer.text(), "/clear");
}

#[tokio::test]
async fn exit_slash_command_requests_shell_exit_without_submitting_turn() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.set_text("/exit");

    let should_exit = shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("exit command should be handled locally");

    assert!(should_exit);
    assert_eq!(shell.composer.text(), "");
    assert_eq!(backend.calls(), Vec::new());

    shell.composer.move_up_or_recall_history();
    assert_eq!(shell.composer.text(), "/exit");
}

#[tokio::test]
async fn exit_keys_require_confirmation_while_ctrl_c_interrupts_immediately() {
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    let mut shell = ShellState::snapshot_fixture();

    assert!(
        !shell
            .handle_key(ctrl_c, &config, &mut backend)
            .await
            .expect("first Ctrl+C should arm exit")
    );
    assert!(shell.exit_confirmation_pending);
    assert!(
        shell
            .handle_key(ctrl_c, &config, &mut backend)
            .await
            .expect("second Ctrl+C should exit")
    );

    let mut shell = ShellState::snapshot_fixture();
    assert!(
        !shell
            .handle_key(esc, &config, &mut backend)
            .await
            .expect("first Esc should arm exit")
    );
    assert!(shell.exit_confirmation_pending);
    assert!(
        shell
            .handle_key(esc, &config, &mut backend)
            .await
            .expect("second Esc should exit")
    );

    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = Some("turn-active".to_string());
    assert!(
        !shell
            .handle_key(ctrl_c, &config, &mut backend)
            .await
            .expect("Ctrl+C should interrupt the active turn")
    );
    assert!(!shell.exit_confirmation_pending);
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::Interrupt {
            thread_id: shell.thread_id,
            turn_id: "turn-active".to_string(),
        }]
    );
}

#[tokio::test]
async fn pointer_activity_clears_exit_confirmation() {
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let mut shell = ShellState::snapshot_fixture();
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("Esc should arm exit confirmation");
    assert!(shell.exit_confirmation_pending);

    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let input = ShellView { shell: &shell }.input_area(area);
    shell
        .handle_mouse_click(
            area,
            Position::new(input.x.saturating_add(2), input.y.saturating_add(2)),
            &config,
            &mut backend,
        )
        .await
        .expect("composer click should succeed");

    assert!(!shell.exit_confirmation_pending);
}

#[tokio::test]
async fn shell_operator_executes_through_workspace_runner_without_submitting_turn() {
    let config = test_config().await;
    let runner = Arc::new(RecordingWorkspaceRunner::new(
        crate::workspace_command::WorkspaceCommandOutput {
            exit_code: 0,
            stdout: "hello\n".to_string(),
            stderr: "warning\n".to_string(),
        },
    ));
    let mut shell = ShellState::snapshot_fixture();
    shell.workspace_command_runner = Some(runner.clone());
    shell.composer.set_text("! printf hello");
    let transcript_len = shell.transcript.len();
    let mut backend = RecordingBackend::default();

    let should_exit = shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("shell command should execute");

    assert!(!should_exit);
    assert!(shell.has_pending_shell_command());
    finish_pending_shell_command(&mut shell).await;
    assert_eq!(
        runner.commands(),
        vec![
            ShellCommand::parse("! printf hello")
                .expect("shell command should parse")
                .workspace_command(std::path::Path::new(&shell.cwd))
        ]
    );
    assert_eq!(
        shell
            .transcript
            .iter()
            .skip(transcript_len)
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            TranscriptLine::new(TranscriptKind::Tool, "! printf hello exit 0")
                .tool_status(ToolBlockStatus::Success),
            TranscriptLine::output(
                "hello\nwarning\n",
                ToolBlockStatus::Success,
                shell
                    .transcript
                    .back()
                    .and_then(|line| line.item_id.clone())
                    .expect("local output should have a stable id"),
            ),
        ]
    );
    assert_eq!(shell.composer.text(), "");
    assert_eq!(backend.calls(), Vec::new());
}

#[tokio::test]
async fn shell_operator_remains_interactive_while_command_is_running() {
    let config = test_config().await;
    let (runner, _gate) =
        RecordingWorkspaceRunner::blocked(crate::workspace_command::WorkspaceCommandOutput {
            exit_code: 0,
            stdout: "finished\n".to_string(),
            stderr: String::new(),
        });
    let runner = Arc::new(runner);
    let mut shell = ShellState::snapshot_fixture();
    shell.workspace_command_runner = Some(runner.clone());
    shell.composer.set_text("! long-running-task");
    let transcript_len = shell.transcript.len();
    let mut backend = RecordingBackend::default();

    tokio::time::timeout(
        Duration::from_secs(/*secs*/ 1),
        shell.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        ),
    )
    .await
    .expect("starting a shell command must not block the event loop")
    .expect("shell command should start");

    assert!(shell.has_pending_shell_command());
    assert_eq!(shell.status, "running shell command");
    assert_eq!(shell.transcript.len(), transcript_len);
    for panel_width in [50, 60, 80] {
        let help = super::dashboard_help::key_hint_lines(&shell, panel_width)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            help.contains("cancel"),
            "running-command help should explain cancellation at width {panel_width}:\n{help}"
        );
    }
    shell.dashboard_route = DashboardRoute::Help;
    insta::assert_snapshot!(
        "running_shell_command",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
            )
        )
    );

    shell.composer.set_text("! second-task");
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("second shell command should be rejected without blocking");
    assert_eq!(shell.composer.text(), "! second-task");
    assert!(shell.block_session_switch_if_busy());
    assert_eq!(
        shell.transcript.back(),
        Some(&TranscriptLine::new(
            TranscriptKind::Status,
            "finish or cancel the shell command before switching sessions",
        ))
    );

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .expect("Ctrl+C should request shell command cancellation");
    assert_eq!(shell.status, "cancelling shell command");

    let should_exit = shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .expect("repeated Ctrl+C should remain scoped to the shell command");
    assert!(!should_exit);
    assert!(!shell.exit_confirmation_pending);

    finish_pending_shell_command(&mut shell).await;
    assert!(!shell.has_pending_shell_command());
    assert_eq!(shell.status, "shell command cancelled");
    assert_eq!(
        shell.transcript.back(),
        Some(
            &TranscriptLine::new(TranscriptKind::Tool, "! long-running-task cancelled")
                .tool_status(ToolBlockStatus::Fail)
        )
    );
    assert_eq!(runner.run_process_ids(), runner.terminate_process_ids());
    assert_eq!(runner.run_process_ids().len(), 1);
    assert!(shell.workspace_status_refresh_due);

    assert!(!shell.poll_session_hydration(&backend).await);
    finish_session_hydration(&mut shell, &backend).await;
    assert!(!shell.workspace_status_refresh_due);
    assert_eq!(runner.commands().len(), 2);
}

async fn finish_pending_shell_command(shell: &mut ShellState) {
    tokio::time::timeout(Duration::from_secs(/*secs*/ 1), async {
        loop {
            let _changed = shell.poll_shell_command().await;
            if !shell.has_pending_shell_command() {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("shell command should finish");
}

#[tokio::test]
async fn goal_slash_command_sets_and_shows_thread_goal() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.set_text("/goal Ship the standalone shell");

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("goal set command should be handled locally");

    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::GoalSet {
            thread_id: shell.thread_id,
            objective: Some("Ship the standalone shell".to_string()),
            status: Some(ThreadGoalStatus::Active),
            token_budget: None,
        }]
    );
    assert_eq!(
        shell.active_goal,
        Some(ThreadGoal {
            token_budget: None,
            ..test_thread_goal(
                &shell.thread_id,
                ThreadGoalStatus::Active,
                "Ship the standalone shell"
            )
        })
    );
    assert_eq!(shell.composer.text(), "");

    shell.composer.set_text("/goal");
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("goal show command should be handled locally");

    assert!(backend.calls().contains(&RecordedBackendCall::GoalGet {
        thread_id: shell.thread_id,
    }));
    assert!(shell.transcript.iter().any(|line| {
        line.kind == TranscriptKind::Status
            && line
                .text
                .contains("goal active. Objective: Ship the standalone shell")
    }));
    assert!(
        !backend
            .calls()
            .iter()
            .any(|call| matches!(call, RecordedBackendCall::TurnStart { .. })),
        "goal slash commands should not submit turns"
    );

    shell.dashboard_route = DashboardRoute::Status;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    insta::assert_snapshot!(render_shell(&shell, area));
}

#[tokio::test]
async fn goal_slash_command_pauses_resumes_and_clears_thread_goal() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    *backend.active_goal.lock().expect("goal should lock") = Some(test_thread_goal(
        &shell.thread_id,
        ThreadGoalStatus::Active,
        "Keep iterating",
    ));

    for command in ["/goal pause", "/goal resume", "/goal clear"] {
        shell.composer.set_text(command);
        shell
            .handle_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .unwrap_or_else(|err| panic!("{command} should be handled locally: {err}"));
    }

    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::GoalSet {
                thread_id: shell.thread_id,
                objective: None,
                status: Some(ThreadGoalStatus::Paused),
                token_budget: None,
            },
            RecordedBackendCall::GoalSet {
                thread_id: shell.thread_id,
                objective: None,
                status: Some(ThreadGoalStatus::Active),
                token_budget: None,
            },
            RecordedBackendCall::GoalClear {
                thread_id: shell.thread_id,
            },
        ]
    );
    assert_eq!(shell.active_goal, None);
    assert!(
        shell
            .transcript
            .iter()
            .any(|line| line.kind == TranscriptKind::Status && line.text == "goal cleared")
    );
}

#[test]
fn dashboard_route_key_mapping_covers_native_routes() {
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL)),
        Some(DashboardRoute::Status)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL)),
        Some(DashboardRoute::Agents)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL)),
        Some(DashboardRoute::Agents)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::CONTROL)),
        Some(DashboardRoute::Sessions)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::CONTROL)),
        Some(DashboardRoute::Help)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('5'), KeyModifiers::CONTROL)),
        None
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('\u{0000}'), KeyModifiers::NONE)),
        Some(DashboardRoute::Agents)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE)),
        Some(DashboardRoute::Agents)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Char('\u{001b}'), KeyModifiers::NONE)),
        Some(DashboardRoute::Sessions)
    );
    assert_eq!(
        dashboard_route_from_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::CONTROL)),
        Some(DashboardRoute::Sessions)
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

fn set_composer_cursor(composer: &mut ComposerState, marked_text: &str) {
    let (before, after) = marked_text
        .split_once('|')
        .expect("marked composer text should contain a cursor");
    composer.set_text(format!("{before}{after}"));
    for _ in after.chars() {
        composer.move_left();
    }
}

fn composer_cursor_text(composer: &ComposerState) -> String {
    let mut text = composer.text().to_string();
    text.insert(composer.cursor(), '|');
    text
}

#[tokio::test]
async fn text_input_shortcuts_work_in_message_and_tool_input_panes() {
    let config = test_config().await;
    let cases = [
        (
            "alpha\nbeta ga|mma",
            KeyCode::Left,
            KeyModifiers::SUPER,
            "alpha\n|beta gamma",
        ),
        (
            "alpha\nbeta ga|mma",
            KeyCode::Right,
            KeyModifiers::SUPER,
            "alpha\nbeta gamma|",
        ),
        (
            "alpha\nbeta ga|mma",
            KeyCode::Backspace,
            KeyModifiers::SUPER,
            "alpha\n|mma",
        ),
        (
            "alpha\nbeta ga|mma",
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
            "alpha\n|beta gamma",
        ),
        (
            "alpha\nbeta ga|mma",
            KeyCode::Char('e'),
            KeyModifiers::CONTROL,
            "alpha\nbeta gamma|",
        ),
        (
            "alpha\nbeta ga|mma",
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
            "alpha\n|mma",
        ),
        (
            "naive beta_gamma, \u{4e16}\u{754c}| tail",
            KeyCode::Left,
            KeyModifiers::ALT,
            "naive beta_gamma, |\u{4e16}\u{754c} tail",
        ),
        (
            "naive beta_gamma, \u{4e16}\u{754c}| tail",
            KeyCode::Left,
            KeyModifiers::CONTROL,
            "naive beta_gamma, |\u{4e16}\u{754c} tail",
        ),
        (
            "naive |beta_gamma, \u{4e16}\u{754c} tail",
            KeyCode::Right,
            KeyModifiers::ALT,
            "naive beta_gamma|, \u{4e16}\u{754c} tail",
        ),
        (
            "naive |beta_gamma, \u{4e16}\u{754c} tail",
            KeyCode::Right,
            KeyModifiers::CONTROL,
            "naive beta_gamma|, \u{4e16}\u{754c} tail",
        ),
        (
            "naive beta_gamma, \u{4e16}\u{754c}| tail",
            KeyCode::Backspace,
            KeyModifiers::ALT,
            "naive beta_gamma, | tail",
        ),
        (
            "naive beta_gamma, \u{4e16}\u{754c}| tail",
            KeyCode::Backspace,
            KeyModifiers::CONTROL,
            "naive beta_gamma, | tail",
        ),
    ];

    for tool_input in [false, true] {
        for (before, code, modifiers, expected) in cases {
            let mut shell = ShellState::snapshot_fixture();
            let mut backend = RecordingBackend::default();
            if tool_input {
                shell.pending_user_input =
                    PendingUserInput::from_request(&tool_user_input_request());
            }
            set_composer_cursor(&mut shell.composer, before);

            shell
                .handle_key(KeyEvent::new(code, modifiers), &config, &mut backend)
                .await
                .expect("text editing shortcut should be handled");

            assert_eq!(composer_cursor_text(&shell.composer), expected);
            assert_eq!(shell.pending_user_input.is_some(), tool_input);
            assert_eq!(backend.calls(), Vec::new());
        }
    }
}

#[tokio::test]
async fn command_backspace_message_result_snapshot() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    set_composer_cursor(&mut shell.composer, "first line\nsecond |line");

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .expect("command backspace should be handled");

    assert_eq!(backend.calls(), Vec::new());
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
        )
    ));
}

#[tokio::test]
async fn tool_input_renders_cursor_and_tracks_shortcuts() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.pending_user_input = PendingUserInput::from_request(&tool_user_input_request());
    set_composer_cursor(&mut shell.composer, "alpha |beta");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let before = render_shell(&shell, area);
    assert!(before.contains("alpha ▏beta"));

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .expect("command left should be handled");

    let after = render_shell(&shell, area);
    assert!(after.contains("▏alpha beta"));
    assert!(!after.contains("alpha ▏beta"));
    assert_eq!(backend.calls(), Vec::new());
    insta::assert_snapshot!("tool_input_command_left_cursor", after);
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

#[tokio::test]
async fn printable_character_repeat_inserts_continuously_in_composer_inputs() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    let repeat =
        KeyEvent::new_with_kind(KeyCode::Char('x'), KeyModifiers::NONE, KeyEventKind::Repeat);
    shell.composer.set_text("x");

    for _ in 0..2 {
        shell
            .handle_key(repeat, &config, &mut backend)
            .await
            .expect("composer repeat should insert text");
    }

    assert_eq!(
        (
            shell.composer.text(),
            shell.pending_user_input.is_some(),
            backend.calls(),
        ),
        ("xxx", false, Vec::new())
    );

    shell.composer.clear();
    shell.pending_user_input = PendingUserInput::from_request(&tool_user_input_request());
    for _ in 0..2 {
        shell
            .handle_key(repeat, &config, &mut backend)
            .await
            .expect("tool input repeat should insert text");
    }

    assert_eq!(
        (
            shell.composer.text(),
            shell.pending_user_input.is_some(),
            backend.calls(),
        ),
        ("xx", true, Vec::new())
    );
}

#[tokio::test]
async fn printable_character_repeat_reaches_text_entry_overlays() {
    let config = test_config().await;
    let repeat =
        KeyEvent::new_with_kind(KeyCode::Char('x'), KeyModifiers::NONE, KeyEventKind::Repeat);
    let mut backend = RecordingBackend::default();

    let mut sessions = ShellState::snapshot_fixture();
    sessions.composer.clear();
    sessions.dashboard_route = DashboardRoute::Sessions;
    sessions.session_list.focused = true;
    sessions.session_list.start_search();
    sessions
        .handle_key(repeat, &config, &mut backend)
        .await
        .expect("session search repeat should insert text");
    sessions.session_list.stop_search();
    assert_eq!(
        (
            sessions.session_list.first_page_params().search_term,
            sessions.composer.text(),
        ),
        (Some("x".to_string()), "")
    );

    let mut settings = ShellState::snapshot_fixture();
    settings.composer.clear();
    settings.dashboard_route = DashboardRoute::Status;
    settings.settings.focused = true;
    settings
        .settings
        .start_edit(SettingsAction::Theme, String::new());
    settings
        .handle_key(repeat, &config, &mut backend)
        .await
        .expect("settings repeat should insert text");
    assert_eq!(
        (settings.settings.take_edit(), settings.composer.text(),),
        (Some((SettingsAction::Theme, "x".to_string())), "")
    );

    let mut mcp = ShellState::snapshot_fixture();
    mcp.composer.clear();
    mcp.mcp_catalog = Some(ListMcpServerStatusResponse {
        data: vec![mcp_status_fixture(
            "github",
            McpAuthStatus::NotLoggedIn,
            ["search"],
        )],
        next_cursor: None,
    });
    mcp.open_mcp_management();
    mcp.handle_key(key_char('a'), &config, &mut backend)
        .await
        .expect("MCP add mode should open");
    mcp.handle_key(repeat, &config, &mut backend)
        .await
        .expect("MCP edit repeat should insert text");
    let mcp_lines = mcp
        .pending_mcp_management
        .as_ref()
        .expect("MCP manager should remain open")
        .lines();
    let mcp_draft = mcp_lines[3]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert_eq!(
        (mcp_draft, mcp.composer.text(), backend.calls()),
        ("x▏".to_string(), "", Vec::new())
    );
}

#[tokio::test]
async fn paste_reaches_text_entry_overlays() {
    let config = test_config().await;
    let mut backend = RecordingBackend::default();

    let mut search = ShellState::snapshot_fixture();
    search.composer.clear();
    search.dashboard_route = DashboardRoute::Sessions;
    search.session_list.focused = true;
    search.session_list.start_search();
    search.insert_pasted_text("alpha beta");
    search.session_list.stop_search();
    assert_eq!(
        (
            search.session_list.first_page_params().search_term,
            search.composer.text(),
        ),
        (Some("alpha beta".to_string()), "")
    );

    let mut rename = ShellState::snapshot_fixture();
    rename.composer.clear();
    rename.dashboard_route = DashboardRoute::Sessions;
    rename.session_list.focused = true;
    rename.session_list.start_rename();
    let previous_name = rename.session_list.rename_draft().unwrap_or_default();
    rename.insert_pasted_text("pasted");
    assert_eq!(
        (rename.session_list.rename_draft(), rename.composer.text()),
        (Some(format!("{previous_name}pasted")), "")
    );

    let mut settings = ShellState::snapshot_fixture();
    settings.composer.clear();
    settings.dashboard_route = DashboardRoute::Status;
    settings.settings.focused = true;
    settings
        .settings
        .start_edit(SettingsAction::Theme, String::new());
    settings.insert_pasted_text("solarized");
    assert_eq!(
        (settings.settings.take_edit(), settings.composer.text()),
        (Some((SettingsAction::Theme, "solarized".to_string())), "")
    );

    let mut mcp = ShellState::snapshot_fixture();
    mcp.composer.clear();
    mcp.mcp_catalog = Some(ListMcpServerStatusResponse {
        data: vec![mcp_status_fixture(
            "github",
            McpAuthStatus::NotLoggedIn,
            ["search"],
        )],
        next_cursor: None,
    });
    mcp.open_mcp_management();
    mcp.handle_key(key_char('a'), &config, &mut backend)
        .await
        .expect("MCP add mode should open");
    mcp.insert_pasted_text("github {\"enabled\":true}");
    let mcp_lines = mcp
        .pending_mcp_management
        .as_ref()
        .expect("MCP manager should remain open")
        .lines();
    let mcp_draft = mcp_lines[3]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert_eq!(
        (mcp_draft, mcp.composer.text(), backend.calls()),
        ("github {\"enabled\":true}▏".to_string(), "", Vec::new())
    );
}

#[tokio::test]
async fn editing_shortcuts_reach_dashboard_and_mcp_text_inputs() {
    let config = test_config().await;
    let mut backend = RecordingBackend::default();

    let mut search = ShellState::snapshot_fixture();
    search.dashboard_route = DashboardRoute::Sessions;
    search.session_list.focused = true;
    search.session_list.start_search();
    for ch in "alpha beta".chars() {
        search
            .handle_key(key_char(ch), &config, &mut backend)
            .await
            .expect("search text should be entered");
    }
    search
        .handle_key(
            KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .expect("search cursor should move by word");
    search
        .handle_key(key_char('X'), &config, &mut backend)
        .await
        .expect("search text should insert at the cursor");
    assert!(
        search
            .session_list
            .lines(/*width*/ 80)
            .iter()
            .any(|line| line_text(line).contains("alpha X▏beta"))
    );
    search.session_list.stop_search();
    assert_eq!(
        search.session_list.first_page_params().search_term,
        Some("alpha Xbeta".to_string())
    );

    let mut rename = ShellState::snapshot_fixture();
    rename.dashboard_route = DashboardRoute::Sessions;
    rename.session_list.focused = true;
    let long_session_word = "segment".repeat(12);
    let session_name = format!("alpha {long_session_word} beta");
    rename.session_list.replace_threads(vec![thread_fixture(
        rename.thread_id,
        Some(&session_name),
        "preview",
    )]);
    rename.session_list.start_rename();
    rename
        .handle_key(
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .expect("rename should delete the previous word");
    assert!(
        rename
            .session_list
            .lines(/*width*/ 80)
            .iter()
            .any(|line| line_text(line).contains("…") && line_text(line).contains('▏'))
    );
    insta::assert_snapshot!(
        "session_rename_cursor",
        render_shell(
            &rename,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
            )
        )
    );
    assert_eq!(
        rename.session_list.take_rename_draft(),
        Some(format!("alpha {long_session_word}"))
    );

    let mut settings = ShellState::snapshot_fixture();
    settings.dashboard_route = DashboardRoute::Status;
    settings.settings.focused = true;
    settings
        .settings
        .start_edit(SettingsAction::Theme, "alpha beta".to_string());
    settings
        .handle_key(
            KeyEvent::new(KeyCode::Left, KeyModifiers::SUPER),
            &config,
            &mut backend,
        )
        .await
        .expect("settings cursor should move to line start");
    settings
        .handle_key(key_char('X'), &config, &mut backend)
        .await
        .expect("settings text should insert at the cursor");
    let settings_view = settings.settings_view();
    assert!(
        settings
            .settings
            .lines(&settings_view, /*width*/ 80)
            .iter()
            .any(|line| line_text(line).contains("X▏alpha beta"))
    );
    assert_eq!(
        settings.settings.take_edit(),
        Some((SettingsAction::Theme, "Xalpha beta".to_string()))
    );

    let mut mcp = ShellState::snapshot_fixture();
    mcp.mcp_catalog = Some(ListMcpServerStatusResponse {
        data: vec![mcp_status_fixture(
            "github",
            McpAuthStatus::NotLoggedIn,
            ["search"],
        )],
        next_cursor: None,
    });
    mcp.open_mcp_management();
    mcp.handle_key(key_char('a'), &config, &mut backend)
        .await
        .expect("MCP add mode should open");
    let long_mcp_word = "endpoint".repeat(14);
    let mcp_text = format!("alpha {long_mcp_word} beta");
    for ch in mcp_text.chars() {
        mcp.handle_key(key_char(ch), &config, &mut backend)
            .await
            .expect("MCP text should be entered");
    }
    mcp.handle_key(
        KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
        &config,
        &mut backend,
    )
    .await
    .expect("MCP cursor should move by word");
    mcp.handle_key(key_char('X'), &config, &mut backend)
        .await
        .expect("MCP text should insert at the cursor");
    let draft = line_text(
        &mcp.pending_mcp_management
            .as_ref()
            .expect("MCP manager should remain open")
            .lines()[3],
    );
    assert_eq!(draft, format!("alpha {long_mcp_word} X▏beta"));
    assert_eq!(backend.calls(), Vec::new());
    insta::assert_snapshot!(
        "mcp_edit_cursor",
        render_shell(
            &mcp,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
            )
        )
    );
}

#[tokio::test]
async fn option_arrow_on_empty_message_does_not_change_dashboard_route() {
    let config = test_config().await;
    for code in [KeyCode::Left, KeyCode::Right] {
        let mut shell = ShellState::snapshot_fixture();
        let mut backend = RecordingBackend::default();
        shell.composer.clear();
        shell.dashboard_route = DashboardRoute::Status;

        shell
            .handle_key(
                KeyEvent::new(code, KeyModifiers::ALT),
                &config,
                &mut backend,
            )
            .await
            .expect("empty message word motion should be handled");

        assert_eq!(
            (
                shell.dashboard_route,
                shell.composer.cursor_position(),
                backend.calls(),
            ),
            (DashboardRoute::Status, (0, 0), Vec::new())
        );
    }
}

#[test]
fn action_mode_keys_require_unmodified_input_and_ignore_repeated_actions() {
    let cases = [
        (KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE), true),
        (
            KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat),
            true,
        ),
        (
            KeyEvent::new_with_kind(KeyCode::Char('d'), KeyModifiers::NONE, KeyEventKind::Repeat),
            false,
        ),
        (
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
            false,
        ),
        (KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT), false),
        (
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::SUPER),
            false,
        ),
    ];

    for (key, expected) in cases {
        assert_eq!(is_unmodified_action_key(key), expected, "key: {key:?}");
    }
}

#[tokio::test]
async fn modified_keys_do_not_trigger_focused_session_settings_or_plugin_actions() {
    let config = test_config().await;
    let modifiers = [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
    ];

    let session_id = test_thread_id("01900000-0000-7000-8000-000000000398");
    let mut sessions = ShellState::snapshot_fixture();
    sessions.dashboard_route = DashboardRoute::Sessions;
    sessions.session_list.focused = true;
    sessions.session_list.replace_threads(vec![thread_fixture(
        session_id,
        Some("keep this session"),
        "modifier guard",
    )]);
    let mut session_backend = RecordingBackend::default();
    for modifier in modifiers {
        sessions
            .handle_key(
                KeyEvent::new(KeyCode::Char('a'), modifier),
                &config,
                &mut session_backend,
            )
            .await
            .expect("modified archive key should be consumed");
        assert_eq!(sessions.session_list.selected_thread_id(), Some(session_id));
        assert_eq!(session_backend.calls(), Vec::new());
    }
    sessions
        .handle_key(
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
            &config,
            &mut session_backend,
        )
        .await
        .expect("Ctrl+P should remain available from a focused action mode");
    assert!(sessions.command_palette.is_some());
    sessions.command_palette = None;
    sessions
        .handle_key(key_char('a'), &config, &mut session_backend)
        .await
        .expect("plain archive key should work");
    assert!(
        session_backend
            .calls()
            .contains(&RecordedBackendCall::Archive(session_id))
    );

    let mut settings = ShellState::snapshot_fixture();
    settings.dashboard_route = DashboardRoute::Status;
    settings.settings.focused = true;
    settings.settings.focus_action(SettingsAction::Animations);
    let initial_animations = settings.animations;
    let mut settings_backend = RecordingBackend::default();
    for modifier in modifiers {
        settings
            .handle_key(
                KeyEvent::new(KeyCode::Enter, modifier),
                &config,
                &mut settings_backend,
            )
            .await
            .expect("modified activation key should be consumed");
        assert_eq!(settings.animations, initial_animations);
        assert_eq!(settings_backend.calls(), Vec::new());
    }
    settings
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut settings_backend,
        )
        .await
        .expect("plain activation key should work");
    complete_backend_actions(&mut settings, &settings_backend).await;
    assert_eq!(settings.animations, !initial_animations);
    assert!(matches!(
        settings_backend.calls().as_slice(),
        [RecordedBackendCall::ConfigWrite(_)]
    ));

    let mut back_tab = ShellState::snapshot_fixture();
    back_tab.dashboard_route = DashboardRoute::Status;
    back_tab.settings.focused = true;
    let mut back_tab_backend = RecordingBackend::default();
    back_tab
        .handle_key(
            KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
            &config,
            &mut back_tab_backend,
        )
        .await
        .expect("terminal-encoded Shift+BackTab should change settings pages");
    assert_eq!(
        back_tab.settings.selected_action(),
        SettingsAction::McpServers
    );
    assert_eq!(back_tab_backend.calls(), Vec::new());

    let mut plugins = ShellState::snapshot_fixture();
    plugins.plugin_catalog = Some(plugin_list_response_fixture());
    plugins.open_plugin_management();
    let initial_plugin_state = plugins.pending_plugin_management.clone();
    let mut plugin_backend = RecordingBackend::default();
    for modifier in modifiers {
        plugins
            .handle_key(
                KeyEvent::new(KeyCode::Char('u'), modifier),
                &config,
                &mut plugin_backend,
            )
            .await
            .expect("modified uninstall key should be consumed");
        assert_eq!(plugins.pending_plugin_management, initial_plugin_state);
        assert_eq!(plugin_backend.calls(), Vec::new());
    }
    plugins
        .handle_key(key_char('u'), &config, &mut plugin_backend)
        .await
        .expect("plain uninstall key should work");
    assert!(
        plugin_backend
            .calls()
            .iter()
            .any(|call| matches!(call, RecordedBackendCall::PluginUninstall { .. }))
    );
}

#[tokio::test]
async fn modified_keys_do_not_trigger_mcp_safety_or_import_actions() {
    let config = test_config().await;

    let mut mcp = ShellState::snapshot_fixture();
    mcp.mcp_catalog = Some(ListMcpServerStatusResponse {
        data: vec![mcp_status_fixture(
            "github",
            McpAuthStatus::NotLoggedIn,
            ["search"],
        )],
        next_cursor: None,
    });
    mcp.open_mcp_management();
    let initial_mcp_state = mcp.pending_mcp_management.clone();
    let mut mcp_backend = RecordingBackend::default();
    mcp.handle_key(
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        &config,
        &mut mcp_backend,
    )
    .await
    .expect("Ctrl+U should not remove the selected MCP server");
    assert_eq!(mcp.pending_mcp_management, initial_mcp_state);
    assert_eq!(mcp_backend.calls(), Vec::new());
    mcp.handle_key(key_char('u'), &config, &mut mcp_backend)
        .await
        .expect("plain remove key should work");
    assert!(mcp_backend.calls().iter().any(|call| matches!(
        call,
        RecordedBackendCall::McpServerWriteConfig {
            value: serde_json::Value::Null,
            merge_strategy: MergeStrategy::Replace,
            ..
        }
    )));

    let mut safety = ShellState::snapshot_fixture();
    let mut safety_backend = RecordingBackend::default();
    safety.submit_prompt(&safety_backend, "Explain the request".to_string());
    complete_backend_actions(&mut safety, &safety_backend).await;
    safety.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
        safety_buffering_notification(
            &safety,
            "turn-submit",
            /*show_buffering_ui*/ true,
            Some("faster-model"),
        ),
    ));
    let initial_safety_calls = safety_backend.calls();
    let initial_safety_transcript = safety.transcript.clone();
    safety
        .handle_key(
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
            &config,
            &mut safety_backend,
        )
        .await
        .expect("Ctrl+R should not retry a safety-buffered turn");
    assert!(safety.safety_buffering_modal_lines().is_some());
    assert_eq!(safety.transcript, initial_safety_transcript);
    assert_eq!(safety_backend.calls(), initial_safety_calls);
    safety
        .handle_key(key_char('r'), &config, &mut safety_backend)
        .await
        .expect("plain retry key should work");
    assert!(safety.safety_buffering_modal_lines().is_none());
    assert!(
        safety_backend
            .calls()
            .iter()
            .any(|call| matches!(call, RecordedBackendCall::Rollback { .. }))
    );

    let items = external_agent_items();
    let mut import = ShellState::snapshot_fixture();
    let mut import_backend = RecordingBackend::with_external_agent_items(items.clone());
    import
        .start_external_agent_import_review(&mut import_backend)
        .await
        .expect("external agent import review should open");
    let initial_import_state = import.pending_external_agent_import.clone();
    let initial_import_calls = import_backend.calls();
    import
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            &config,
            &mut import_backend,
        )
        .await
        .expect("modified Enter should not start import");
    assert_eq!(import.pending_external_agent_import, initial_import_state);
    assert_eq!(import_backend.calls(), initial_import_calls);
    import
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut import_backend,
        )
        .await
        .expect("plain Enter should start import");
    assert!(
        import_backend
            .calls()
            .contains(&RecordedBackendCall::ExternalAgentConfigImport(items))
    );
}

#[tokio::test]
async fn modified_enter_does_not_commit_editors_or_command_palette_actions() {
    let config = test_config().await;
    let modifiers = [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
        KeyModifiers::SHIFT,
    ];

    let session_id = test_thread_id("01900000-0000-7000-8000-000000000397");
    let mut sessions = ShellState::snapshot_fixture();
    sessions.dashboard_route = DashboardRoute::Sessions;
    sessions.session_list.focused = true;
    sessions.session_list.replace_threads(vec![thread_fixture(
        session_id,
        Some("rename draft"),
        "modified Enter guard",
    )]);
    let mut session_backend = RecordingBackend::default();
    sessions
        .handle_key(key_char('n'), &config, &mut session_backend)
        .await
        .expect("rename mode should open");
    for modifier in modifiers {
        sessions
            .handle_key(
                KeyEvent::new(KeyCode::Enter, modifier),
                &config,
                &mut session_backend,
            )
            .await
            .expect("modified Enter should not commit a session rename");
        assert!(sessions.session_list.renaming());
        assert_eq!(sessions.session_list.selected_title(), Some("rename draft"));
        assert_eq!(session_backend.calls(), Vec::new());
    }

    sessions
        .handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &config,
            &mut session_backend,
        )
        .await
        .expect("plain Esc should cancel rename mode");
    sessions
        .handle_key(key_char('/'), &config, &mut session_backend)
        .await
        .expect("search mode should open");
    sessions
        .handle_key(key_char('x'), &config, &mut session_backend)
        .await
        .expect("search text should be accepted");
    for modifier in modifiers {
        sessions
            .handle_key(
                KeyEvent::new(KeyCode::Enter, modifier),
                &config,
                &mut session_backend,
            )
            .await
            .expect("modified Enter should not stop search mode");
        assert!(sessions.session_list.search_active());
    }
    sessions
        .handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::ALT),
            &config,
            &mut session_backend,
        )
        .await
        .expect("modified Esc should not cancel search mode");
    assert!(sessions.session_list.search_active());
    sessions
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut session_backend,
        )
        .await
        .expect("plain Enter should stop search mode");
    assert_eq!(
        sessions.session_list.first_page_params().search_term,
        Some("x".to_string())
    );

    let mut settings = ShellState::snapshot_fixture();
    settings.dashboard_route = DashboardRoute::Status;
    settings.settings.focused = true;
    settings
        .settings
        .start_edit(SettingsAction::Theme, "theme-draft".to_string());
    let mut settings_backend = RecordingBackend::default();
    for modifier in modifiers {
        settings
            .handle_key(
                KeyEvent::new(KeyCode::Enter, modifier),
                &config,
                &mut settings_backend,
            )
            .await
            .expect("modified Enter should not commit a settings edit");
        assert!(settings.settings.editing());
        assert_eq!(settings_backend.calls(), Vec::new());
    }

    let mut mcp = ShellState::snapshot_fixture();
    mcp.mcp_catalog = Some(ListMcpServerStatusResponse {
        data: vec![mcp_status_fixture(
            "github",
            McpAuthStatus::NotLoggedIn,
            ["search"],
        )],
        next_cursor: None,
    });
    mcp.open_mcp_management();
    let mut mcp_backend = RecordingBackend::default();
    mcp.handle_key(key_char('e'), &config, &mut mcp_backend)
        .await
        .expect("MCP edit mode should open");
    let initial_mcp_state = mcp.pending_mcp_management.clone();
    for modifier in modifiers {
        mcp.handle_key(
            KeyEvent::new(KeyCode::Enter, modifier),
            &config,
            &mut mcp_backend,
        )
        .await
        .expect("modified Enter should not commit an MCP edit");
        assert_eq!(mcp.pending_mcp_management, initial_mcp_state);
        assert_eq!(mcp_backend.calls(), Vec::new());
    }

    let mut palette = ShellState::snapshot_fixture();
    palette.open_command_palette();
    select_command_palette_action(&mut palette, CommandPaletteAction::ResumeThread);
    let initial_palette_state = palette.command_palette.clone();
    let initial_route = palette.dashboard_route;
    let mut palette_backend = RecordingBackend::default();
    for modifier in modifiers {
        palette
            .handle_key(
                KeyEvent::new(KeyCode::Enter, modifier),
                &config,
                &mut palette_backend,
            )
            .await
            .expect("modified Enter should not execute a command palette action");
        assert_eq!(palette.command_palette, initial_palette_state);
        assert_eq!(palette.dashboard_route, initial_route);
        assert_eq!(palette_backend.calls(), Vec::new());
    }
}

#[tokio::test]
async fn repeated_action_keys_do_not_toggle_submit_or_delete() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.set_text("prompt");

    for key in [
        KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Repeat),
        KeyEvent::new_with_kind(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
            KeyEventKind::Repeat,
        ),
    ] {
        shell
            .handle_key(key, &config, &mut backend)
            .await
            .expect("repeated shortcut should be ignored");
    }

    let other_thread_id = test_thread_id("01900000-0000-7000-8000-000000000399");
    shell.session_list.replace_threads(vec![thread_fixture(
        other_thread_id,
        Some("do not delete"),
        "repeat guard",
    )]);
    shell.dashboard_route = DashboardRoute::Sessions;
    shell.session_list.focused = true;
    shell
        .handle_key(
            KeyEvent::new_with_kind(KeyCode::Char('d'), KeyModifiers::NONE, KeyEventKind::Repeat),
            &config,
            &mut backend,
        )
        .await
        .expect("repeated list action should be ignored");

    assert_eq!(
        (
            shell.dashboard_visible,
            shell.composer.text(),
            shell.session_list.selected_thread_id(),
            backend.calls(),
        ),
        (true, "prompt", Some(other_thread_id), Vec::new())
    );
}

#[test]
fn dashboard_route_step_matches_alt_arrow_fallbacks_only_when_allowed() {
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

#[test]
fn new_shell_defaults_to_status_model_regardless_of_legacy_route_state() {
    let codex_home = tempfile::tempdir().expect("create temp codex home");
    std::fs::write(
        codex_home.path().join("app-shell-state.json"),
        b"{\"route\":\"sessions\"}",
    )
    .expect("write legacy route state");
    let started = started_thread(
        "new session",
        test_thread_id("01900000-0000-7000-8000-000000000702"),
        /*forked_from_id*/ None,
    );
    let shell = ShellState::new(
        started.session,
        "fallback-model".to_string(),
        Vec::new(),
        codex_home.path().to_path_buf(),
        /*tui_theme*/ None,
        /*animations*/ true,
        /*show_tooltips*/ true,
        /*max_concurrent_threads_per_session*/ 4,
    );

    assert_eq!(
        (shell.dashboard_route, shell.settings.selected_action()),
        (DashboardRoute::Status, SettingsAction::Model)
    );
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28
        )
    ));
}

#[test]
fn transcript_selection_moves_between_items() {
    let mut shell = ShellState::snapshot_fixture();
    shell.select_latest_transcript_item();

    assert_eq!(
        shell.selected_transcript_copy_text(),
        Some((TranscriptKind::Diff, "3 files +128 -24"))
    );

    shell.move_transcript_selection_up(/*rows*/ 2);

    assert_eq!(
        shell.selected_transcript_copy_text(),
        Some((
            TranscriptKind::Plan,
            "1. Build shell\n2. Wire transcript\n3. Render dashboard"
        ))
    );

    shell.move_transcript_selection_down(/*rows*/ 1);

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

    shell.ingest_completed_item(
        ThreadItem::UserMessage {
            id: "user-1".to_string(),
            client_id: None,
            content: vec![UserInput::Text {
                text: "hello from user".to_string(),
                text_elements: Vec::new(),
            }],
        },
        CompletedItemOrigin::Live,
    );

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

    shell.ingest_completed_item(
        ThreadItem::AgentMessage {
            id: "agent-1".to_string(),
            text: "hello from codex".to_string(),
            phase: None,
            memory_citation: None,
        },
        CompletedItemOrigin::Live,
    );
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
fn completed_reasoning_hides_empty_generated_summary_parts() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();

    shell.ingest_completed_item(
        ThreadItem::Reasoning {
            id: "reasoning-1".to_string(),
            summary: vec![
                "**Checking the first thing**\n\n<!-- -->".to_string(),
                "**Checking the second thing**\n\n<!-- -->".to_string(),
            ],
            content: vec!["raw reasoning must not replace an empty summary".to_string()],
        },
        CompletedItemOrigin::Live,
    );

    assert_eq!(shell.transcript, VecDeque::new());
}

#[test]
fn completed_reasoning_uses_summary_without_interleaving_raw_content() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();

    shell.ingest_completed_item(
        ThreadItem::Reasoning {
            id: "reasoning-1".to_string(),
            summary: vec![
                "**Plan**\n\ndone".to_string(),
                "**Checking tests**\n\n<!-- -->".to_string(),
            ],
            content: vec![
                "raw reasoning one".to_string(),
                "raw reasoning two".to_string(),
            ],
        },
        CompletedItemOrigin::Live,
    );

    assert_eq!(
        shell.transcript,
        VecDeque::from([TranscriptLine::new(
            TranscriptKind::Status,
            "reasoning: done",
        )])
    );
    insta::assert_snapshot!(shell.transcript[0].text, @"reasoning: done");
}

#[test]
fn completed_extension_items_render_as_successful_tools() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.tool_activity.clear();

    shell.ingest_completed_item(
        ThreadItem::WebSearch(WebSearchItem {
            id: "web-1".to_string(),
            query: "latest protocol changes".to_string(),
            action: None,
        }),
        CompletedItemOrigin::Live,
    );
    shell.ingest_completed_item(
        ThreadItem::ImageGeneration(ImageGenerationItem {
            id: "image-1".to_string(),
            status: "completed".to_string(),
            revised_prompt: None,
            result: String::new(),
            saved_path: None,
        }),
        CompletedItemOrigin::Live,
    );

    assert_eq!(
        shell.tool_activity,
        VecDeque::from([
            ToolActivity {
                id: "web-1".to_string(),
                title: "web search: latest protocol changes".to_string(),
                status: "completed".to_string(),
            },
            ToolActivity {
                id: "image-1".to_string(),
                title: "image generation".to_string(),
                status: "completed".to_string(),
            },
        ])
    );
    assert_eq!(
        shell.transcript,
        VecDeque::from([
            TranscriptLine::new(TranscriptKind::Tool, "web search: latest protocol changes")
                .tool_status(ToolBlockStatus::Success)
                .item_id("web-1"),
            TranscriptLine::new(TranscriptKind::Tool, "image generation")
                .tool_status(ToolBlockStatus::Success)
                .item_id("image-1"),
        ])
    );
}

#[tokio::test]
async fn safety_buffering_retry_rolls_back_and_resubmits_without_duplicate_transcript() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.transcript.clear();

    shell.submit_prompt(&backend, "Explain the request".to_string());
    complete_backend_actions(&mut shell, &backend).await;
    shell.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
        safety_buffering_notification(
            &shell,
            "turn-submit",
            /*show_buffering_ui*/ true,
            Some("faster-model"),
        ),
    ));

    shell
        .handle_key(key_char('r'), &config, &mut backend)
        .await
        .expect("retry key should be handled");

    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::TurnStart {
                thread_id: shell.thread_id,
                prompt: "Explain the request".to_string(),
                cwd: PathBuf::from("/workspace/better-codex"),
                model: "gpt-5-codex".to_string(),
                effort: None,
                collaboration_mode: None,
            },
            RecordedBackendCall::Interrupt {
                thread_id: shell.thread_id,
                turn_id: "turn-submit".to_string(),
            },
            RecordedBackendCall::Rollback {
                thread_id: shell.thread_id,
                num_turns: 1,
            },
            RecordedBackendCall::TurnStart {
                thread_id: shell.thread_id,
                prompt: "Explain the request".to_string(),
                cwd: PathBuf::from("/workspace/better-codex"),
                model: "faster-model".to_string(),
                effort: Some(ReasoningEffort::Low),
                collaboration_mode: None,
            },
        ]
    );
    assert_eq!(
        shell.transcript,
        VecDeque::from([TranscriptLine::new(
            TranscriptKind::User,
            "Explain the request",
        )])
    );
    assert_eq!(shell.active_turn_id.as_deref(), Some("turn-submit"));
    assert!(shell.safety_buffering_modal_lines().is_none());
}

#[tokio::test]
async fn renders_safety_buffering_retry_modal_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    let backend = RecordingBackend::default();

    shell.submit_prompt(&backend, "Explain the request".to_string());
    complete_backend_actions(&mut shell, &backend).await;
    shell.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
        safety_buffering_notification(
            &shell,
            "turn-submit",
            /*show_buffering_ui*/ true,
            Some("faster-model"),
        ),
    ));

    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
        ),
    ));
}

#[test]
fn safety_buffering_filters_stale_updates_and_cleans_up_when_streaming_starts() {
    let shell = ShellState::snapshot_fixture();
    let thread_id = shell.thread_id.to_string();
    let streaming_notifications = [
        ServerNotification::AgentMessageDelta(
            codex_app_server_protocol::AgentMessageDeltaNotification {
                thread_id: thread_id.clone(),
                turn_id: "turn-active".to_string(),
                item_id: "assistant-1".to_string(),
                delta: "response".to_string(),
            },
        ),
        ServerNotification::PlanDelta(codex_app_server_protocol::PlanDeltaNotification {
            thread_id: thread_id.clone(),
            turn_id: "turn-active".to_string(),
            item_id: "plan-1".to_string(),
            delta: "plan".to_string(),
        }),
        ServerNotification::ReasoningSummaryTextDelta(
            codex_app_server_protocol::ReasoningSummaryTextDeltaNotification {
                thread_id: thread_id.clone(),
                turn_id: "turn-active".to_string(),
                item_id: "reasoning-1".to_string(),
                delta: "summary".to_string(),
                summary_index: 0,
            },
        ),
        ServerNotification::ReasoningTextDelta(
            codex_app_server_protocol::ReasoningTextDeltaNotification {
                thread_id,
                turn_id: "turn-active".to_string(),
                item_id: "reasoning-1".to_string(),
                delta: "reasoning".to_string(),
                content_index: 0,
            },
        ),
    ];

    for streaming_notification in streaming_notifications {
        let mut shell = ShellState::snapshot_fixture();
        shell.active_turn_id = Some("turn-active".to_string());

        let mut stale_thread = safety_buffering_notification(
            &shell,
            "turn-active",
            /*show_buffering_ui*/ true,
            Some("faster-model"),
        );
        stale_thread.thread_id = "other-thread".to_string();
        shell.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
            stale_thread,
        ));
        shell.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
            safety_buffering_notification(
                &shell,
                "turn-stale",
                /*show_buffering_ui*/ true,
                Some("faster-model"),
            ),
        ));
        assert!(shell.safety_buffering_modal_lines().is_none());

        shell.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
            safety_buffering_notification(
                &shell,
                "turn-active",
                /*show_buffering_ui*/ true,
                Some("faster-model"),
            ),
        ));
        assert!(shell.safety_buffering_modal_lines().is_some());
        assert_eq!(shell.status, "waiting");

        shell.handle_notification(streaming_notification);
        assert!(shell.safety_buffering_modal_lines().is_none());
        assert_ne!(shell.status, "waiting");

        shell.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
            safety_buffering_notification(
                &shell,
                "turn-active",
                /*show_buffering_ui*/ true,
                Some("faster-model"),
            ),
        ));
        assert!(shell.safety_buffering_modal_lines().is_none());
    }
}

#[test]
fn safety_buffering_hide_and_turn_completion_clear_the_modal() {
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = Some("turn-active".to_string());
    shell.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
        safety_buffering_notification(
            &shell,
            "turn-active",
            /*show_buffering_ui*/ true,
            /*faster_model*/ None,
        ),
    ));
    assert!(shell.safety_buffering_modal_lines().is_some());

    shell.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
        safety_buffering_notification(
            &shell,
            "turn-active",
            /*show_buffering_ui*/ false,
            /*faster_model*/ None,
        ),
    ));
    assert!(shell.safety_buffering_modal_lines().is_none());
    assert_eq!(shell.status, "thinking");

    shell.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
        safety_buffering_notification(
            &shell,
            "turn-active",
            /*show_buffering_ui*/ true,
            /*faster_model*/ None,
        ),
    ));
    shell.handle_notification(ServerNotification::TurnCompleted(
        codex_app_server_protocol::TurnCompletedNotification {
            thread_id: shell.thread_id.to_string(),
            turn: test_turn("turn-active", TurnStatus::Completed),
        },
    ));
    assert!(shell.safety_buffering_modal_lines().is_none());
    assert_eq!(shell.status, "ready");
}

#[test]
fn turn_completion_adds_one_separator_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.active_turn_id = Some("turn-separator".to_string());
    shell.push_assistant("Turn complete.");

    for _ in 0..2 {
        shell.handle_notification(ServerNotification::TurnCompleted(
            codex_app_server_protocol::TurnCompletedNotification {
                thread_id: shell.thread_id.to_string(),
                turn: test_turn("turn-separator", TurnStatus::Completed),
            },
        ));
    }

    assert_eq!(
        shell.transcript,
        VecDeque::from([
            TranscriptLine::new(TranscriptKind::Assistant, "Turn complete."),
            TranscriptLine::new(TranscriptKind::Separator, ""),
        ])
    );
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20
        )
    ));
}

#[tokio::test]
async fn ctrl_d_hides_dashboard_and_reclaims_layout_snapshot() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    let mut backend = RecordingBackend::default();
    let toggle = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);

    assert!(
        !shell
            .handle_key(toggle, &config, &mut backend)
            .await
            .expect("Ctrl+D should hide the dashboard")
    );
    assert!(!shell.dashboard_visible);
    assert!(!shell.session_list.focused);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24,
    );
    assert_eq!(ShellView { shell: &shell }.input_area(area).width, 78);
    let rendered = render_shell(&shell, area);
    assert!(!rendered.contains("Navigation"));
    insta::assert_snapshot!(rendered);

    assert!(
        !shell
            .handle_key(toggle, &config, &mut backend)
            .await
            .expect("Ctrl+D should restore the dashboard")
    );
    assert!(shell.dashboard_visible);
    assert_eq!(backend.calls(), Vec::new());
}

#[tokio::test]
async fn dashboard_header_button_toggles_in_sidebar_and_overlay_layouts() {
    let config = test_config().await;
    for (area, is_overlay) in [
        (
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
            ),
            false,
        ),
        (
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24,
            ),
            true,
        ),
    ] {
        let mut shell = ShellState::snapshot_fixture();
        shell.session_list.focused = true;
        shell.settings.focused = true;
        shell.agents_focused = true;
        let mut backend = RecordingBackend::default();
        let button_positions = |shell: &ShellState| {
            (area.x..area.right())
                .flat_map(|x| (area.y..area.bottom()).map(move |y| Position::new(x, y)))
                .filter(|position| {
                    ShellView { shell }.header_control_at(area, *position)
                        == Some(header::HeaderControl::Dashboard)
                })
                .collect::<Vec<_>>()
        };
        let visible_buttons = button_positions(&shell);
        let button = visible_buttons
            .first()
            .copied()
            .expect("dashboard button should be visible");
        let visible_input_width = ShellView { shell: &shell }.input_area(area).width;

        assert_eq!(visible_input_width == area.width, is_overlay);
        shell
            .handle_mouse_click(area, button, &config, &mut backend)
            .await
            .expect("dashboard button should hide the dashboard");

        assert_eq!(
            (
                shell.dashboard_visible,
                shell.session_list.focused,
                shell.settings.focused,
                shell.agents_focused,
                ShellView { shell: &shell }.input_area(area).width,
            ),
            (false, false, false, false, area.width)
        );
        assert_eq!(button_positions(&shell), visible_buttons);

        shell
            .handle_mouse_click(area, button, &config, &mut backend)
            .await
            .expect("dashboard button should restore the dashboard");

        assert_eq!(
            (
                shell.dashboard_visible,
                shell.session_list.focused,
                shell.settings.focused,
                shell.agents_focused,
                ShellView { shell: &shell }.input_area(area).width,
            ),
            (true, false, false, false, visible_input_width)
        );
        assert_eq!(backend.calls(), Vec::new());
    }
}

#[tokio::test]
async fn ctrl_number_tabs_focus_only_after_selecting_route_snapshot() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    let ctrl_status = KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL);
    let ctrl_sessions = KeyEvent::new(KeyCode::Char('3'), KeyModifiers::CONTROL);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    shell
        .handle_key(ctrl_status, &config, &mut backend)
        .await
        .expect("Ctrl+1 should select status");

    assert_eq!(
        (
            shell.dashboard_route,
            shell.session_list.focused,
            shell.settings.focused,
        ),
        (DashboardRoute::Status, false, false)
    );
    assert!(ShellView { shell: &shell }.cursor_position(area).is_some());
    insta::assert_snapshot!(render_shell(&shell, area));

    shell
        .handle_key(ctrl_status, &config, &mut backend)
        .await
        .expect("Ctrl+1 should focus selected status settings");

    assert_eq!(
        (
            shell.dashboard_route,
            shell.session_list.focused,
            shell.settings.focused,
        ),
        (DashboardRoute::Status, false, true)
    );
    assert_eq!(ShellView { shell: &shell }.cursor_position(area), None);

    shell
        .handle_key(ctrl_sessions, &config, &mut backend)
        .await
        .expect("Ctrl+3 should select sessions");

    assert_eq!(
        (
            shell.dashboard_route,
            shell.session_list.focused,
            shell.settings.focused,
        ),
        (DashboardRoute::Sessions, false, false)
    );
    assert!(ShellView { shell: &shell }.cursor_position(area).is_some());

    shell
        .handle_key(ctrl_sessions, &config, &mut backend)
        .await
        .expect("Ctrl+3 should focus selected sessions");

    assert_eq!(
        (
            shell.dashboard_route,
            shell.session_list.focused,
            shell.settings.focused,
        ),
        (DashboardRoute::Sessions, true, false)
    );
    assert_eq!(ShellView { shell: &shell }.cursor_position(area), None);
}

#[tokio::test]
async fn blocked_session_list_refresh_does_not_block_dashboard_input() {
    let config = test_config().await;
    let session_id = test_thread_id("01900000-0000-7000-8000-000000000798");
    let gate = Arc::new(tokio::sync::Semaphore::new(/*permits*/ 0));
    let mut backend = RecordingBackend {
        threads: Arc::new(Mutex::new(vec![thread_fixture(
            session_id,
            Some("background session"),
            "loaded after input",
        )])),
        thread_list_gate: Some(gate.clone()),
        ..RecordingBackend::default()
    };
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
    let ctrl_sessions = KeyEvent::new(KeyCode::Char('3'), KeyModifiers::CONTROL);
    let ctrl_status = KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL);

    tokio::time::timeout(Duration::from_secs(/*secs*/ 1), async {
        shell
            .handle_key(ctrl_sessions, &config, &mut backend)
            .await
            .expect("Ctrl+3 should start loading sessions");
        shell
            .handle_key(ctrl_sessions, &config, &mut backend)
            .await
            .expect("repeated Ctrl+3 should coalesce with the pending load");
        shell
            .handle_key(ctrl_status, &config, &mut backend)
            .await
            .expect("other dashboard input should stay responsive");
    })
    .await
    .expect("dashboard input should not wait for thread/list");

    assert!(shell.has_pending_session_hydration());
    assert_eq!(shell.dashboard_route, DashboardRoute::Status);
    gate.add_permits(/*n*/ 1);
    finish_session_hydration(&mut shell, &backend).await;

    assert_eq!(shell.session_list.selected_thread_id(), Some(session_id));
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::ThreadList {
            archived: Some(false),
            search_term: None,
            cursor: None,
        }]
    );
}

#[tokio::test]
async fn empty_enter_focuses_the_active_interactive_dashboard() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.clear();

    for route in [
        DashboardRoute::Sessions,
        DashboardRoute::Agents,
        DashboardRoute::Status,
    ] {
        shell.dashboard_route = route;
        shell.session_list.focused = false;
        shell.agents_focused = false;
        shell.settings.focused = false;

        shell
            .handle_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .expect("empty Enter should focus the active dashboard");

        assert_eq!(
            shell.session_list.focused,
            route == DashboardRoute::Sessions
        );
        assert_eq!(shell.agents_focused, route == DashboardRoute::Agents);
        assert_eq!(shell.settings.focused, route == DashboardRoute::Status);
    }
    assert_eq!(backend.calls(), Vec::new());
}

#[tokio::test]
async fn dashboard_tab_clicks_select_routes_without_focusing_panels() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    let config = test_config().await;

    for area in [
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
        ),
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24,
        ),
    ] {
        shell.dashboard_route = DashboardRoute::Sessions;
        let rendered = render_shell(&shell, area);
        assert!(!rendered.contains("Navigation"));
        assert!(!rendered.contains("Alt+Left/Right"));
        let (tab_row_index, tab_row) = rendered
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("Sessions") && line.contains("Agents"))
            .expect("all dashboard tabs should share a visible row");
        let tab_row_index = u16::try_from(tab_row_index).unwrap_or(u16::MAX);

        for (route, label) in [
            (DashboardRoute::Status, "Status"),
            (DashboardRoute::Agents, "Agents"),
            (DashboardRoute::Sessions, "Sessions"),
            (DashboardRoute::Help, "Help"),
        ] {
            shell.session_list.focused = true;
            shell.settings.focused = true;
            shell.agents_focused = true;
            let label_start = tab_row
                .find(label)
                .expect("dashboard tab label should be visible");
            let position = Position::new(
                area.x.saturating_add(
                    u16::try_from(
                        tab_row[..label_start].chars().count() + label.chars().count() / 2,
                    )
                    .unwrap_or(u16::MAX),
                ),
                area.y.saturating_add(tab_row_index),
            );

            shell
                .handle_mouse_click(area, position, &config, &mut backend)
                .await
                .expect("tab click should succeed");

            assert_eq!(
                (
                    shell.dashboard_route,
                    shell.session_list.focused,
                    shell.settings.focused,
                    shell.agents_focused,
                ),
                (route, false, false, false)
            );
            if area.width == 100 && route == DashboardRoute::Status {
                assert!(ShellView { shell: &shell }.cursor_position(area).is_some());
                insta::assert_snapshot!(
                    "dashboard_status_tab_click_without_focus",
                    render_shell(&shell, area)
                );
            }
        }
    }
}

#[tokio::test]
async fn clicking_the_composer_returns_focus_from_dashboard_panels() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.session_list.focused = true;
    shell.settings.focused = true;
    shell.agents_focused = true;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let input = ShellView { shell: &shell }.input_area(area);

    shell
        .handle_mouse_click(
            area,
            Position::new(input.x.saturating_add(2), input.y.saturating_add(2)),
            &config,
            &mut backend,
        )
        .await
        .expect("composer click should succeed");

    assert_eq!(
        (
            shell.session_list.focused,
            shell.settings.focused,
            shell.agents_focused,
            shell.transcript_selection,
        ),
        (false, false, false, None)
    );
    assert!(ShellView { shell: &shell }.cursor_position(area).is_some());
}

#[tokio::test]
async fn clicking_new_session_starts_a_clear_session_and_focuses_the_composer() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.session_list.focused = true;
    let mut backend = RecordingBackend::default();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let buf = render_shell_buffer(&shell, area);
    let row =
        row_containing(&buf, area, "+ New session").expect("new session action should be visible");
    let x = row_needle_x(&buf, area, row, "+ New session")
        .expect("new session action should have an x position");

    shell
        .handle_mouse_click(area, Position::new(x, row), &config, &mut backend)
        .await
        .expect("new session click should succeed");

    assert_eq!(
        backend.calls().first(),
        Some(&RecordedBackendCall::Start(Some(ThreadStartSource::Clear)))
    );
    assert!(!shell.session_list.focused);
    assert!(ShellView { shell: &shell }.cursor_position(area).is_some());
}

#[test]
fn pointer_hover_uses_existing_header_hit_geometry() {
    let mut shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let position = (area.x..area.right())
        .flat_map(|x| (area.y..area.bottom()).map(move |y| Position::new(x, y)))
        .find(|position| {
            ShellView { shell: &shell }.header_control_at(area, *position)
                == Some(header::HeaderControl::Model)
        })
        .expect("model chip should be visible");
    shell.pointer_position = Some(position);

    let buf = render_shell_buffer(&shell, area);

    assert_eq!(buf[position].style().bg, Some(design::palette::BORDER));
}

#[test]
fn dashboard_button_hover_uses_its_hit_geometry_in_both_states() {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    for dashboard_visible in [true, false] {
        let mut shell = ShellState::snapshot_fixture();
        shell.dashboard_visible = dashboard_visible;
        let position = (area.x..area.right())
            .flat_map(|x| (area.y..area.bottom()).map(move |y| Position::new(x, y)))
            .find(|position| {
                ShellView { shell: &shell }.header_control_at(area, *position)
                    == Some(header::HeaderControl::Dashboard)
            })
            .expect("dashboard button should be visible");
        shell.pointer_position = Some(position);

        assert_eq!(
            render_shell_buffer(&shell, area)[position].style().bg,
            Some(design::palette::BORDER),
            "dashboard_visible {dashboard_visible}"
        );
    }
}

#[test]
fn settings_tab_hover_uses_content_only_geometry() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let baseline = render_shell_buffer(&shell, area);
    let label_y =
        row_containing(&baseline, area, "Permissions").expect("settings tabs should be visible");
    let pointer = Position::new(
        row_needle_x(&baseline, area, label_y, "Permissions")
            .expect("permissions tab should be visible"),
        label_y,
    );
    let panel_position = ShellView { shell: &shell }
        .dashboard_panel_position_at(area, pointer, "Settings")
        .expect("pointer should be inside the settings panel");
    let strip_x = pointer
        .x
        .saturating_sub(u16::try_from(panel_position.column).unwrap_or(u16::MAX));
    let columns = settings::SettingsTabs::new(panel_position.width)
        .column_range(settings::SettingsPage::Permissions)
        .expect("permissions tab should have a content range");

    shell.pointer_position = Some(pointer);
    let hovered = render_shell_buffer(&shell, area);
    let actual_backgrounds = (0..panel_position.width)
        .map(|column| {
            hovered[(
                strip_x.saturating_add(u16::try_from(column).unwrap_or(u16::MAX)),
                label_y,
            )]
                .style()
                .bg
        })
        .collect::<Vec<_>>();
    let mut expected_backgrounds = (0..panel_position.width)
        .map(|column| {
            baseline[(
                strip_x.saturating_add(u16::try_from(column).unwrap_or(u16::MAX)),
                label_y,
            )]
                .style()
                .bg
        })
        .collect::<Vec<_>>();
    expected_backgrounds[columns.clone()].fill(Some(design::palette::BORDER));
    assert_eq!(actual_backgrounds, expected_backgrounds);

    let underline_y = label_y.saturating_add(1);
    let actual_foregrounds = (0..panel_position.width)
        .map(|column| {
            hovered[(
                strip_x.saturating_add(u16::try_from(column).unwrap_or(u16::MAX)),
                underline_y,
            )]
                .style()
                .fg
        })
        .collect::<Vec<_>>();
    let mut expected_foregrounds = (0..panel_position.width)
        .map(|column| {
            baseline[(
                strip_x.saturating_add(u16::try_from(column).unwrap_or(u16::MAX)),
                underline_y,
            )]
                .style()
                .fg
        })
        .collect::<Vec<_>>();
    expected_foregrounds[columns].fill(Some(design::palette::FOCUS));
    assert_eq!(actual_foregrounds, expected_foregrounds);
}

#[test]
fn mouse_wheel_routes_to_the_pane_under_the_pointer_in_both_layouts() {
    for area in [
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 16,
        ),
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 16,
        ),
    ] {
        let mut shell = ShellState::snapshot_fixture();
        shell.dashboard_route = DashboardRoute::Status;
        for index in 0..40 {
            shell.push_assistant(format!("scrollable response {index}"));
        }
        render_shell(&shell, area);
        let transcript = position_in(area, |position| {
            ShellView { shell: &shell }.pointer_pane_at(area, position)
                == Some(render::PointerPane::Transcript)
        });
        let dashboard = position_in(area, |position| {
            ShellView { shell: &shell }.dashboard_route_at(area, position)
                == Some(DashboardRoute::Status)
        });

        shell.handle_mouse_scroll(area, transcript, tui::MouseScrollDirection::Up);
        shell.handle_mouse_scroll(area, dashboard, tui::MouseScrollDirection::Down);
        let input = ShellView { shell: &shell }.input_area(area);
        shell.handle_mouse_scroll(
            area,
            Position::new(input.x, input.y),
            tui::MouseScrollDirection::Down,
        );

        assert_eq!(
            (shell.transcript_scroll, shell.dashboard_scroll.get()),
            (3, 3)
        );
        assert_eq!(
            ShellView { shell: &shell }.pointer_pane_at(area, dashboard),
            Some(render::PointerPane::Dashboard)
        );

        shell.pending_elicitation =
            PendingElicitation::from_request(&mcp_url_elicitation_request());
        shell.handle_mouse_scroll(area, transcript, tui::MouseScrollDirection::Down);
        assert_eq!(
            (shell.transcript_scroll, shell.dashboard_scroll.get()),
            (3, 3)
        );
    }
}

#[test]
fn scrolled_dashboard_keeps_tabs_fixed_in_both_layouts_snapshot() {
    for area in [
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 16,
        ),
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 16,
        ),
    ] {
        let mut shell = ShellState::snapshot_fixture();
        shell.dashboard_route = DashboardRoute::Status;
        let before = render_shell(&shell, area);
        let tab = position_in(area, |position| {
            ShellView { shell: &shell }.dashboard_route_at(area, position)
                == Some(DashboardRoute::Status)
        });
        let fixed_tabs = before
            .lines()
            .skip(usize::from(tab.y))
            .take(2)
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        for _ in 0..20 {
            shell.handle_mouse_scroll(area, tab, tui::MouseScrollDirection::Down);
        }
        let bottom = shell.dashboard_scroll.get();
        assert!(bottom > 0);
        shell.handle_mouse_scroll(area, tab, tui::MouseScrollDirection::Down);
        assert_eq!(shell.dashboard_scroll.get(), bottom);

        let after = render_shell(&shell, area);
        assert_eq!(
            after
                .lines()
                .skip(usize::from(tab.y))
                .take(2)
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            fixed_tabs
        );
        assert!(!after.contains("SETTINGS"), "{after}");
        assert!(after.contains("TOKENS"), "{after}");
        if area.width == 100 {
            insta::assert_snapshot!("scrolled_dashboard_sidebar", after);
        }

        for _ in 0..20 {
            shell.handle_mouse_scroll(area, tab, tui::MouseScrollDirection::Up);
        }
        assert_eq!(shell.dashboard_scroll.get(), 0);
    }
}

#[test]
fn dashboard_scroll_reclamps_when_the_viewport_grows() {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 16,
    );
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
    let tab = position_in(area, |position| {
        ShellView { shell: &shell }.dashboard_route_at(area, position)
            == Some(DashboardRoute::Status)
    });

    for _ in 0..20 {
        shell.handle_mouse_scroll(area, tab, tui::MouseScrollDirection::Down);
    }
    assert!(shell.dashboard_scroll.get() > 0);

    render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 64,
        ),
    );

    assert_eq!(shell.dashboard_scroll.get(), 0);
}

#[test]
fn dashboard_overlay_takes_pointer_precedence_over_transcript_cards() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.transcript.clear();
    shell.clear_streaming_transcript();
    shell.push_output_with_status("compile output", ToolBlockStatus::Running);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24,
    );
    let card_position = (area.x..area.right())
        .rev()
        .flat_map(|x| (area.y..area.bottom()).map(move |y| Position::new(x, y)))
        .find(|position| {
            ShellView { shell: &shell }
                .transcript_card_at(area, *position)
                .is_some()
        })
        .expect("output card should extend beneath the dashboard overlay");

    shell.dashboard_visible = true;
    let view = ShellView { shell: &shell };

    assert_eq!(
        (
            view.pointer_pane_at(area, card_position),
            view.transcript_card_at(area, card_position),
        ),
        (Some(render::PointerPane::Dashboard), None)
    );
}

#[test]
fn mouse_wheel_uses_scrolled_dashboard_panel_geometry() {
    for area in [
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 16,
        ),
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 16,
        ),
    ] {
        let mut shell = ShellState::snapshot_fixture();
        shell.dashboard_route = DashboardRoute::Status;
        render_shell(&shell, area);
        let tab = position_in(area, |position| {
            ShellView { shell: &shell }.dashboard_route_at(area, position)
                == Some(DashboardRoute::Status)
        });
        shell.handle_mouse_scroll(area, tab, tui::MouseScrollDirection::Down);
        let outer_scroll = shell.dashboard_scroll.get();
        assert!(outer_scroll > 0);
        render_shell(&shell, area);
        let settings = position_in(area, |position| {
            ShellView { shell: &shell }
                .dashboard_panel_position_at(area, position, "Settings")
                .is_some()
        });

        shell.handle_mouse_scroll(area, settings, tui::MouseScrollDirection::Down);

        assert_eq!(
            (
                shell.settings.selected_action(),
                shell.dashboard_scroll.get(),
            ),
            (SettingsAction::ReasoningEffort, outer_scroll)
        );
        for _ in 0..20 {
            shell.handle_mouse_scroll(area, settings, tui::MouseScrollDirection::Down);
        }
        let last_action = shell.settings.selected_action();
        shell.handle_mouse_scroll(area, settings, tui::MouseScrollDirection::Down);
        assert_eq!(
            (
                shell.settings.selected_action(),
                shell.dashboard_scroll.get()
            ),
            (last_action, outer_scroll)
        );
        shell.settings.start_edit(last_action, "draft".to_string());
        shell.handle_mouse_scroll(area, settings, tui::MouseScrollDirection::Up);
        assert_eq!(
            (
                shell.settings.selected_action(),
                shell.dashboard_scroll.get(),
                shell.settings.editing(),
            ),
            (last_action, outer_scroll, true)
        );
        assert!(!shell.settings.focused);
        shell.set_dashboard_route(DashboardRoute::Help);
        assert_eq!(shell.dashboard_scroll.get(), 0);
    }
}

#[test]
fn session_and_agent_wheels_stay_nested_in_both_layouts() {
    for area in [
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 16,
        ),
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 16,
        ),
    ] {
        let mut sessions = ShellState::snapshot_fixture();
        sessions.dashboard_route = DashboardRoute::Sessions;
        sessions.session_list.replace_threads(
            (1..=10)
                .map(|index| {
                    thread_fixture(
                        test_thread_id(&format!("01900000-0000-7000-8000-{index:012x}")),
                        Some(&format!("Session {index:02}")),
                        "nested wheel fixture",
                    )
                })
                .collect(),
        );
        render_shell(&sessions, area);
        let tab = position_in(area, |position| {
            ShellView { shell: &sessions }.dashboard_route_at(area, position)
                == Some(DashboardRoute::Sessions)
        });
        sessions.handle_mouse_scroll(area, tab, tui::MouseScrollDirection::Down);
        let outer_scroll = sessions.dashboard_scroll.get();
        render_shell(&sessions, area);
        let body = position_in(area, |position| {
            ShellView { shell: &sessions }
                .dashboard_panel_position_at(area, position, "Sessions")
                .is_some()
        });
        sessions.handle_mouse_scroll(area, body, tui::MouseScrollDirection::Down);
        assert_eq!(
            (
                sessions.session_list.selected_title(),
                sessions.dashboard_scroll.get(),
            ),
            (Some("Session 02"), outer_scroll)
        );
        for _ in 0..20 {
            sessions.handle_mouse_scroll(area, body, tui::MouseScrollDirection::Down);
        }
        sessions.handle_mouse_scroll(area, body, tui::MouseScrollDirection::Down);
        assert_eq!(
            (
                sessions.session_list.selected_title(),
                sessions.dashboard_scroll.get(),
            ),
            (Some("Session 10"), outer_scroll)
        );
        sessions.session_list.start_rename();
        sessions.handle_mouse_scroll(area, body, tui::MouseScrollDirection::Up);
        assert_eq!(
            (
                sessions.session_list.selected_title(),
                sessions.dashboard_scroll.get(),
                sessions.session_list.renaming(),
            ),
            (Some("Session 10"), outer_scroll, true)
        );

        let mut agents = ShellState::snapshot_fixture();
        agents.dashboard_route = DashboardRoute::Agents;
        for index in 0..4 {
            agents
                .agent_activity
                .ensure_thread(&format!("agent-{index}"));
        }
        agents.agent_activity.select_thread("agent-0");
        render_shell(&agents, area);
        let tab = position_in(area, |position| {
            ShellView { shell: &agents }.dashboard_route_at(area, position)
                == Some(DashboardRoute::Agents)
        });
        agents.handle_mouse_scroll(area, tab, tui::MouseScrollDirection::Down);
        let outer_scroll = agents.dashboard_scroll.get();
        render_shell(&agents, area);
        let body = position_in(area, |position| {
            ShellView { shell: &agents }
                .dashboard_panel_position_at(area, position, "Agents")
                .is_some()
        });
        agents.handle_mouse_scroll(area, body, tui::MouseScrollDirection::Down);
        assert_eq!(
            (
                agents.agent_activity.selected_thread_id(),
                agents.dashboard_scroll.get(),
            ),
            (Some("agent-1"), outer_scroll)
        );
        for _ in 0..20 {
            agents.handle_mouse_scroll(area, body, tui::MouseScrollDirection::Down);
        }
        agents.handle_mouse_scroll(area, body, tui::MouseScrollDirection::Down);
        assert_eq!(
            (
                agents.agent_activity.selected_thread_id(),
                agents.dashboard_scroll.get(),
            ),
            (Some("agent-3"), outer_scroll)
        );
    }
}

#[test]
fn session_wheel_requests_the_next_page_at_the_loaded_boundary() {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Sessions;
    shell.session_list.replace_thread_page(
        (0..20)
            .map(|index| {
                thread_fixture(
                    test_thread_id(&format!("01900000-0000-7000-8002-{index:012x}")),
                    Some(&format!("Session {index:02}")),
                    "wheel pagination fixture",
                )
            })
            .collect(),
        Some("20".to_string()),
    );
    for _ in 1..20 {
        shell.session_list.move_selection_down();
    }
    render_shell(&shell, area);
    let body = position_in(area, |position| {
        ShellView { shell: &shell }
            .dashboard_panel_position_at(area, position, "Sessions")
            .is_some()
    });

    assert!(shell.handle_mouse_scroll(area, body, tui::MouseScrollDirection::Down));
}

#[tokio::test]
async fn long_narrow_elicitation_and_tool_options_are_clickable() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 48, /*height*/ 16,
    );
    let mut elicitation_request = mcp_url_elicitation_request();
    let ServerRequest::McpServerElicitationRequest { params, .. } = &mut elicitation_request else {
        panic!("expected MCP elicitation request");
    };
    params.server_name = "github-enterprise-with-a-long-server-name".to_string();
    let McpServerElicitationRequest::Url { message, url, .. } = &mut params.request else {
        panic!("expected URL elicitation request");
    };
    *message =
        "Open the authorization page after reviewing the extended security notice".to_string();
    *url = "https://github.example.test/login/device/with/a/long/authorization/path".to_string();
    shell.pending_elicitation = PendingElicitation::from_request(&elicitation_request);
    let rendered = render_shell(&shell, area);
    assert!(rendered.contains("↓ more"));
    let input = (ShellView { shell: &shell }).input_area(area);
    let position = Position::new(input.x.saturating_add(1), input.y.saturating_add(1));
    for _ in 0..20 {
        shell.handle_mouse_scroll(area, position, tui::MouseScrollDirection::Down);
    }
    let rendered = render_shell(&shell, area);
    assert!(
        rendered.contains("authorization") && rendered.contains("path"),
        "URL suffix should be inspectable after scrolling:\n{rendered}"
    );
    let accept = rendered_text_position(&rendered, "Accept ↵");

    shell
        .handle_mouse_click(area, accept, &config, &mut backend)
        .await
        .expect("elicitation click should succeed");

    assert!(shell.pending_elicitation.is_none());
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::Resolve(RequestId::Integer(45))]
    );

    let mut user_input_request = tool_user_input_request();
    let ServerRequest::ToolRequestUserInput { params, .. } = &mut user_input_request else {
        panic!("expected tool user input request");
    };
    params.item_id = "environment-selection-for-the-production-deployment".to_string();
    let question = params.questions.first_mut().expect("tool input question");
    question.header = "Deployment environment and release channel".to_string();
    question.question = "Which environment should receive the carefully validated release after all preflight checks complete?".to_string();
    shell.pending_user_input = PendingUserInput::from_request(&user_input_request);
    let rendered = render_shell(&shell, area);
    assert!(
        rendered.contains("Staging"),
        "narrow tool input should keep choices visible:\n{rendered}"
    );
    let staging = rendered_text_position(&rendered, "Staging");
    shell
        .handle_mouse_click(area, staging, &config, &mut backend)
        .await
        .expect("tool option click should succeed");

    assert!(shell.pending_user_input.is_none());
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::Resolve(RequestId::Integer(45)),
            RecordedBackendCall::Resolve(RequestId::Integer(43)),
        ]
    );
}

#[test]
fn long_narrow_approval_keeps_wrapped_actions_visible_and_clickable_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    let mut request = command_approval_request();
    let ServerRequest::CommandExecutionRequestApproval { params, .. } = &mut request else {
        panic!("expected command approval request");
    };
    let dangerous_suffix = "rm -rf /workspace/production";
    params.command = Some(format!(
        "{}&& {dangerous_suffix}",
        "printf 'validated safe prefix'; ".repeat(20)
    ));
    params.reason = Some("Review the full command before approving".to_string());
    shell.pending_approval =
        PendingApproval::from_request(&request).expect("approval request should be valid");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 40, /*height*/ 18,
    );
    let rendered = render_shell(&shell, area);
    let explain = rendered_text_position(&rendered, "? Explain");

    assert!(rendered.contains("↓ more"));
    assert!(!rendered.contains(dangerous_suffix));
    assert_eq!(
        ShellView { shell: &shell }.approval_action_at(area, explain),
        Some(ApprovalAction::Explain)
    );
    insta::assert_snapshot!(rendered);

    let input = (ShellView { shell: &shell }).input_area(area);
    let position = Position::new(input.x.saturating_add(1), input.y.saturating_add(1));
    for _ in 0..20 {
        shell.handle_mouse_scroll(area, position, tui::MouseScrollDirection::Down);
    }
    let rendered = render_shell(&shell, area);

    assert!(
        rendered.contains("rm -rf") && rendered.contains("production"),
        "dangerous suffix should be visible after scrolling:\n{rendered}"
    );
    assert!(rendered.contains("↑ more"));
    insta::assert_snapshot!("long_narrow_approval_scrolled_to_suffix", rendered);

    let end_offset = shell
        .pending_approval
        .as_ref()
        .expect("approval should remain pending")
        .scroll_offset();
    shell.handle_mouse_scroll(area, position, tui::MouseScrollDirection::Up);
    assert!(
        shell
            .pending_approval
            .as_ref()
            .expect("approval should remain pending")
            .scroll_offset()
            < end_offset
    );
}

#[tokio::test]
async fn safety_modal_actions_are_clickable() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    shell.submit_prompt(&backend, "Explain the request".to_string());
    complete_backend_actions(&mut shell, &backend).await;
    shell.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
        safety_buffering_notification(
            &shell,
            "turn-submit",
            /*show_buffering_ui*/ true,
            Some("faster-model"),
        ),
    ));
    let dismiss = rendered_text_position(&render_shell(&shell, area), "Dismiss and keep waiting");

    shell
        .handle_mouse_click(area, dismiss, &config, &mut backend)
        .await
        .expect("safety action click should succeed");

    assert!(shell.safety_buffering_modal_lines().is_none());
}

#[tokio::test]
async fn safety_modal_ignores_inside_chrome_and_closes_on_outside_click() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    shell.submit_prompt(&backend, "Explain the request".to_string());
    complete_backend_actions(&mut shell, &backend).await;
    shell.handle_notification(ServerNotification::ModelSafetyBufferingUpdated(
        safety_buffering_notification(
            &shell,
            "turn-submit",
            /*show_buffering_ui*/ true,
            Some("faster-model"),
        ),
    ));
    let lines = shell
        .safety_buffering_modal_lines()
        .expect("safety modal should be open");
    let panel = modal_view::modal_panel_area(area, &lines);
    let rendered = render_shell(&shell, area);
    let title = rendered_text_position(&rendered, "SAFETY REVIEW");
    let explanation = rendered_text_position(&rendered, "Our systems are thinking");
    let calls = backend.calls();

    for position in [title, explanation] {
        shell
            .handle_mouse_click(area, position, &config, &mut backend)
            .await
            .expect("inside safety modal click should succeed");
        assert!(shell.safety_buffering_modal_lines().is_some());
        assert_eq!(backend.calls(), calls);
    }

    shell
        .handle_mouse_click(
            area,
            Position::new(panel.x.saturating_sub(1), panel.y),
            &config,
            &mut backend,
        )
        .await
        .expect("outside safety modal click should succeed");

    assert!(shell.safety_buffering_modal_lines().is_none());
    assert_eq!(backend.calls(), calls);
}

#[tokio::test]
async fn blocking_overlays_capture_keys_and_paste_before_the_composer() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.set_text("draft");
    shell.pending_elicitation = PendingElicitation::from_request(&mcp_url_elicitation_request());

    shell.insert_pasted_text(" pasted");
    shell
        .handle_key(key_char('x'), &config, &mut backend)
        .await
        .expect("modal key should be captured");
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .expect("modal shortcut should be captured");

    assert_eq!(shell.composer.text(), "draft");
    assert_eq!(shell.dashboard_route, DashboardRoute::Sessions);

    shell.pending_elicitation = None;
    shell.diff_view = Some(DiffViewState::new(
        "Session edits",
        /*source_item_id*/ None,
        vec![super::diff_view::DiffFile::added(
            "src/lib.rs",
            "new line",
            super::diff_view::DiffStatus::Completed,
        )],
    ));
    shell.insert_pasted_text(" hidden by diff");
    assert_eq!(shell.composer.text(), "draft");

    shell.diff_view = None;
    shell.session_list.focused = true;
    shell.insert_pasted_text(" hidden");
    assert_eq!(shell.composer.text(), "draft");

    shell.pending_user_input = PendingUserInput::from_request(&tool_user_input_request());
    shell.insert_pasted_text(" answer");
    assert_eq!(shell.composer.text(), "draft answer");
}

#[tokio::test]
async fn escape_during_tool_input_does_not_exit_the_shell() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.pending_user_input = PendingUserInput::from_request(&tool_user_input_request());

    let should_exit = shell
        .handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("escape should be captured");

    assert!(!should_exit);
    assert!(shell.pending_user_input.is_some());
}

#[test]
fn bio_policy_error_renders_dedicated_safety_notice() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.active_turn_id = Some("turn-active".to_string());

    shell.handle_notification(ServerNotification::Error(ErrorNotification {
        error: TurnError {
            message: serde_json::json!({
                "error": {"code": "bio_policy", "message": "copy may change"}
            })
            .to_string(),
            codex_error_info: None,
            additional_details: None,
        },
        will_retry: false,
        thread_id: shell.thread_id.to_string(),
        turn_id: "turn-active".to_string(),
    }));

    assert_eq!(shell.transcript.len(), 1);
    assert_eq!(shell.transcript[0].kind, TranscriptKind::Status);
    assert_eq!(shell.active_turn_id, None);
    insta::assert_snapshot!(shell.transcript[0].text, @r"
This content can't be shown

We take extra caution with requests involving biological research and applications that could pose safety risks. Eligible researchers can apply for Trusted Access.

Trusted Access: https://www.openai.com/form/trusted-access-for-biology-research/
Learn more: https://help.openai.com/en/articles/20001326
    ");
}

#[test]
fn renders_pending_approval_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    let mut request = command_approval_request();
    let ServerRequest::CommandExecutionRequestApproval { params, .. } = &mut request else {
        panic!("expected command approval request");
    };
    let amendment = codex_app_server_protocol::ExecPolicyAmendment {
        command: vec!["cargo".to_string(), "test".to_string()],
    };
    params.command_actions = Some(vec![codex_app_server_protocol::CommandAction::Read {
        command: "cat Cargo.toml".to_string(),
        name: "Cargo.toml".to_string(),
        path: LegacyAppPathString::from_abs_path(&test_absolute_path(
            "workspace/better-codex/Cargo.toml",
        )),
    }]);
    params.additional_permissions = Some(codex_app_server_protocol::AdditionalPermissionProfile {
        network: Some(AdditionalNetworkPermissions {
            enabled: Some(true),
        }),
        file_system: Some(codex_app_server_protocol::AdditionalFileSystemPermissions {
            read: None,
            write: None,
            glob_scan_max_depth: None,
            entries: Some(vec![codex_app_server_protocol::FileSystemSandboxEntry {
                path: codex_app_server_protocol::FileSystemPath::Path {
                    path: LegacyAppPathString::from_abs_path(&test_absolute_path(
                        "workspace/shared-cache",
                    )),
                },
                access: codex_app_server_protocol::FileSystemAccessMode::Write,
            }]),
        }),
    });
    params.proposed_execpolicy_amendment = Some(amendment.clone());
    params.available_decisions = Some(vec![
        CommandExecutionApprovalDecision::Accept,
        CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment {
            execpolicy_amendment: amendment,
        },
        CommandExecutionApprovalDecision::Cancel,
    ]);
    shell.pending_approval =
        PendingApproval::from_request(&request).expect("approval request should be valid");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_network_approval_details_snapshot() {
    let mut network_request = command_approval_request();
    let ServerRequest::CommandExecutionRequestApproval { params, .. } = &mut network_request else {
        panic!("expected command approval request");
    };
    let allow = codex_app_server_protocol::NetworkPolicyAmendment {
        host: "packages.example.com".to_string(),
        action: codex_app_server_protocol::NetworkPolicyRuleAction::Allow,
    };
    params.command = None;
    params.cwd = None;
    params.command_actions = None;
    params.network_approval_context = Some(codex_app_server_protocol::NetworkApprovalContext {
        host: "packages.example.com".to_string(),
        protocol: codex_app_server_protocol::NetworkApprovalProtocol::Https,
    });
    params.proposed_network_policy_amendments = Some(vec![allow.clone()]);
    params.available_decisions = Some(vec![
        CommandExecutionApprovalDecision::Accept,
        CommandExecutionApprovalDecision::AcceptForSession,
        CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
            network_policy_amendment: allow,
        },
        CommandExecutionApprovalDecision::Cancel,
    ]);
    let mut network_shell = ShellState::snapshot_fixture();
    network_shell.pending_approval = PendingApproval::from_request(&network_request)
        .expect("network approval request should be valid");
    insta::assert_snapshot!(render_shell(
        &network_shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28
        )
    ));
}

#[test]
fn renders_filesystem_permissions_approval_details_snapshot() {
    let mut permissions_request = permissions_approval_request();
    let ServerRequest::PermissionsRequestApproval { params, .. } = &mut permissions_request else {
        panic!("expected permissions approval request");
    };
    params.permissions.file_system =
        Some(codex_app_server_protocol::AdditionalFileSystemPermissions {
            read: None,
            write: None,
            glob_scan_max_depth: None,
            entries: Some(vec![
                codex_app_server_protocol::FileSystemSandboxEntry {
                    path: codex_app_server_protocol::FileSystemPath::Special {
                        value: codex_app_server_protocol::FileSystemSpecialPath::Root,
                    },
                    access: codex_app_server_protocol::FileSystemAccessMode::Read,
                },
                codex_app_server_protocol::FileSystemSandboxEntry {
                    path: codex_app_server_protocol::FileSystemPath::GlobPattern {
                        pattern: "/private/secrets/**".to_string(),
                    },
                    access: codex_app_server_protocol::FileSystemAccessMode::Write,
                },
            ]),
        });
    let mut permissions_shell = ShellState::snapshot_fixture();
    permissions_shell.pending_approval = PendingApproval::from_request(&permissions_request)
        .expect("permissions approval request should be valid");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&permissions_shell, area));
}

#[test]
fn renders_file_change_approval_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.pending_approval = PendingApproval::from_request(&file_change_approval_request())
        .expect("approval request should be valid");

    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28
        )
    ));
}

#[test]
fn approval_action_keys_cover_full_keyboard_flow() {
    let pending = PendingApproval::from_request(&command_approval_request())
        .expect("approval request should be valid")
        .expect("request should be supported");
    assert_eq!(
        approval_action_from_key(&pending, key_char('a')),
        Some(ApprovalAction::Choose(0))
    );
    assert_eq!(
        approval_action_from_key(&pending, key_char('d')),
        Some(ApprovalAction::Choose(1))
    );
    assert_eq!(
        approval_action_from_key(&pending, key_char('e')),
        Some(ApprovalAction::Edit)
    );
    assert_eq!(
        approval_action_from_key(&pending, key_char('?')),
        Some(ApprovalAction::Explain)
    );
    assert_eq!(
        approval_action_from_key(&pending, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(ApprovalAction::Choose(0))
    );
    assert_eq!(
        approval_action_from_key(&pending, key_char('2')),
        Some(ApprovalAction::Choose(1))
    );
    assert_eq!(
        approval_action_from_key(
            &pending,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)
        ),
        None
    );
    assert_eq!(
        approval_action_from_key(&pending, key_char('j')),
        Some(ApprovalAction::ScrollDown)
    );
    assert_eq!(
        approval_action_from_key(&pending, KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)),
        Some(ApprovalAction::PageUp)
    );
}

#[test]
fn approval_denial_shortcuts_avoid_persistent_network_rules() {
    let mut request = command_approval_request();
    let ServerRequest::CommandExecutionRequestApproval { params, .. } = &mut request else {
        panic!("expected command approval request");
    };
    params.available_decisions = Some(vec![
        CommandExecutionApprovalDecision::Accept,
        CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
            network_policy_amendment: codex_app_server_protocol::NetworkPolicyAmendment {
                host: "packages.example.com".to_string(),
                action: codex_app_server_protocol::NetworkPolicyRuleAction::Deny,
            },
        },
        CommandExecutionApprovalDecision::Cancel,
    ]);
    let pending = PendingApproval::from_request(&request)
        .expect("approval request should be valid")
        .expect("request should be supported");

    assert_eq!(
        approval_action_from_key(&pending, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(ApprovalAction::Choose(2))
    );
}

#[tokio::test]
async fn approval_keys_take_priority_over_transcript_selection() {
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let mut shell = ShellState::snapshot_fixture();
    shell.pending_approval = PendingApproval::from_request(&command_approval_request())
        .expect("approval request should be valid");
    shell.transcript_selection = Some(0);

    let should_exit = shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("approval should resolve");
    complete_backend_actions(&mut shell, &backend).await;

    assert!(!should_exit);
    assert_eq!(shell.pending_approval, None);
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::Resolve(RequestId::Integer(41))]
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
            "approval explained: Run command: cargo test -p codex-tui - Reason: Needs network access - Working directory: /workspace/better-codex",
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
fn renders_empty_pending_user_input_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.pending_user_input = PendingUserInput::from_request(&tool_user_input_request());

    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
        )
    ));
}

#[test]
fn renders_auto_resolving_user_input_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.pending_user_input = PendingUserInput::from_request(
        &tool_user_input_request_with_auto_resolution(/*auto_resolution_ms*/ 60_000),
    );

    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
        )
    ));
}

#[test]
fn renders_secret_user_input_cursor_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.pending_user_input = PendingUserInput::from_request(&tool_free_form_user_input_request());
    set_composer_cursor(
        &mut shell.composer,
        &format!("{}sec▏ret|value", "hidden".repeat(20)),
    );

    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 60, /*height*/ 24,
        )
    ));
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
fn long_mcp_elicitation_keeps_destination_visible_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    let mut request = mcp_url_elicitation_request();
    let ServerRequest::McpServerElicitationRequest { params, .. } = &mut request else {
        panic!("expected MCP elicitation request");
    };
    let McpServerElicitationRequest::Url { message, url, .. } = &mut params.request else {
        panic!("expected URL elicitation request");
    };
    *message = "Review every authorization requirement before continuing. ".repeat(30);
    *url = "https://auth.example.test/device".to_string();
    shell.pending_elicitation = PendingElicitation::from_request(&request);
    let rendered = render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 60, /*height*/ 18,
        ),
    );

    assert!(rendered.contains("https://auth.example.test/device"));
    assert!(rendered.contains("↓ more"));
    assert!(rendered.contains("Accept ↵"));
    insta::assert_snapshot!(rendered);
}

#[test]
fn renders_decision_audit_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.push_decision_audit("approval", "approved", "Command: cargo test -p codex-tui");
    shell.push_decision_audit("elicitation", "declined", "MCP github: URL request");
    shell.push_decision_audit("tool input", "submitted", "Tool input: environment");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 31,
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
fn diff_summaries_use_addition_and_removal_colors() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_diff("9 files +8 -7");
    shell.dashboard_route = DashboardRoute::Status;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 60,
    );

    let buf = render_shell_buffer(&shell, area);

    assert_eq!(
        text_color_for_row(&buf, area, "+8"),
        Some(design::palette::SUCCESS)
    );
    assert_eq!(
        text_color_for_row(&buf, area, "-7"),
        Some(design::palette::ERROR)
    );
    assert_eq!(
        text_color_for_row(&buf, area, "+128"),
        Some(design::palette::SUCCESS)
    );
    assert_eq!(
        text_color_for_row(&buf, area, "-24"),
        Some(design::palette::ERROR)
    );
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
        Some(design::palette::CYAN)
    );
    assert_eq!(
        accent_color_for_row(&buf, area, "exec true"),
        Some(design::palette::SUCCESS)
    );
    assert_eq!(
        accent_color_for_row(&buf, area, "exec false"),
        Some(design::palette::ERROR)
    );
}

#[test]
fn tool_transcript_block_background_spans_conversation_width() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.push_tool_with_status("exec short", ToolBlockStatus::Running);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
    );

    let buf = render_shell_buffer(&shell, area);
    let row = row_containing(&buf, area, "exec short").expect("tool row should render");
    let content = design::pane_content_rect(ShellView { shell: &shell }.input_area(area));
    let right_edge = content.right().saturating_sub(1);

    assert_eq!(
        buf.cell((right_edge, row))
            .expect("right edge cell should exist")
            .style()
            .bg,
        Some(design::MOCHA_SURFACE0)
    );
}

#[test]
fn renders_activity_dashboard_panels_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Help;
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
fn narrow_dashboard_keeps_the_active_route_interactive() {
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

    assert!(rendered.contains("Sessions"));
    assert!(rendered.contains("CLICK TO FOCUS"));
    assert!(rendered.contains("Run command: cargo test"));
}

#[test]
fn help_dashboard_shows_every_shortcut_at_78_by_24_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Help;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24,
    );

    let rendered = render_shell(&shell, area);

    assert!(
        rendered.contains("Esc×2 exit"),
        "shortcut tail should remain visible:\n{rendered}"
    );
    insta::assert_snapshot!(rendered);
}

#[test]
fn help_dashboard_shows_every_shortcut_at_48_by_16_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Help;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 48, /*height*/ 16,
    );

    let rendered = render_shell(&shell, area);

    assert!(
        rendered.contains("Esc×2 exit"),
        "shortcut tail should remain visible:\n{rendered}"
    );
    assert!(rendered.contains("> Summarize the new shell architecture"));
    assert!(!rendered.contains("Esc composer"));
    insta::assert_snapshot!(rendered);
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
fn subagent_lifecycle_records_do_not_create_permanent_running_tool_cards() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.subagent_activity.clear();
    let lifecycle_item = ThreadItem::SubAgentActivity {
        id: "agent-started".to_string(),
        kind: SubAgentActivityKind::Started,
        agent_thread_id: "agent-thread".to_string(),
        agent_path: "/root/reviewer".to_string(),
    };

    shell.handle_notification(ServerNotification::ItemStarted(ItemStartedNotification {
        thread_id: shell.thread_id.to_string(),
        turn_id: "turn-1".to_string(),
        started_at_ms: 0,
        item: lifecycle_item.clone(),
    }));
    shell.handle_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            thread_id: shell.thread_id.to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 1,
            item: lifecycle_item,
        },
    ));

    assert_eq!(shell.transcript, VecDeque::new());
    assert_eq!(
        shell.subagent_activity,
        VecDeque::from([ToolActivity {
            id: "agent-started".to_string(),
            title: "subagent Started: /root/reviewer".to_string(),
            status: "recorded".to_string(),
        }])
    );
}

#[test]
fn child_thread_events_update_the_agent_inspector_without_touching_the_transcript() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    let root_thread_id = shell.thread_id.to_string();
    let child_thread_id = "agent-thread".to_string();

    shell.handle_notification(ServerNotification::ItemStarted(ItemStartedNotification {
        thread_id: root_thread_id,
        turn_id: "turn-1".to_string(),
        started_at_ms: 0,
        item: ThreadItem::CollabAgentToolCall {
            id: "agent-tool-1".to_string(),
            tool: CollabAgentTool::SpawnAgent,
            status: CollabAgentToolCallStatus::InProgress,
            sender_thread_id: "parent-thread".to_string(),
            receiver_thread_ids: vec![child_thread_id.clone()],
            prompt: Some("Inspect dashboard activity.".to_string()),
            model: Some("gpt-5-codex".to_string()),
            reasoning_effort: Some(ReasoningEffort::High),
            agents_states: Default::default(),
        },
    }));
    let root_transcript = shell.transcript.clone();
    shell.handle_notification(ServerNotification::AgentMessageDelta(
        codex_app_server_protocol::AgentMessageDeltaNotification {
            thread_id: child_thread_id.clone(),
            turn_id: "child-turn".to_string(),
            item_id: "message-1".to_string(),
            delta: "Review ".to_string(),
        },
    ));
    shell.handle_notification(ServerNotification::AgentMessageDelta(
        codex_app_server_protocol::AgentMessageDeltaNotification {
            thread_id: child_thread_id.clone(),
            turn_id: "child-turn".to_string(),
            item_id: "message-1".to_string(),
            delta: "complete.".to_string(),
        },
    ));
    shell.handle_notification(ServerNotification::ItemStarted(ItemStartedNotification {
        thread_id: child_thread_id.clone(),
        turn_id: "child-turn".to_string(),
        started_at_ms: 1,
        item: ThreadItem::Reasoning {
            id: "reasoning-1".to_string(),
            summary: Vec::new(),
            content: Vec::new(),
        },
    }));
    assert_eq!(
        shell
            .agent_activity
            .agent(&child_thread_id)
            .expect("spawned agent should be tracked")
            .timeline
            .back()
            .map(agent_activity::AgentTimelineEntry::label),
        Some("reasoning started".to_string())
    );
    shell.handle_notification(ServerNotification::ReasoningTextDelta(
        codex_app_server_protocol::ReasoningTextDeltaNotification {
            thread_id: child_thread_id.clone(),
            turn_id: "child-turn".to_string(),
            item_id: "reasoning-1".to_string(),
            delta: "private chain of thought".to_string(),
            content_index: 0,
        },
    ));
    let agent = shell
        .agent_activity
        .agent(&child_thread_id)
        .expect("spawned agent should remain tracked");
    assert_eq!(agent.latest_message.as_deref(), Some("Review complete."));
    assert_eq!(
        agent
            .timeline
            .back()
            .map(agent_activity::AgentTimelineEntry::label),
        Some("reasoning started".to_string())
    );
    shell.handle_notification(ServerNotification::ReasoningSummaryTextDelta(
        codex_app_server_protocol::ReasoningSummaryTextDeltaNotification {
            thread_id: child_thread_id.clone(),
            turn_id: "child-turn".to_string(),
            item_id: "reasoning-1".to_string(),
            delta: "checking constraints".to_string(),
            summary_index: 0,
        },
    ));
    shell.handle_notification(ServerNotification::CommandExecutionOutputDelta(
        CommandExecutionOutputDeltaNotification {
            thread_id: child_thread_id.clone(),
            turn_id: "child-turn".to_string(),
            item_id: "command-1".to_string(),
            delta: "tests pass".to_string(),
        },
    ));
    shell.handle_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            thread_id: child_thread_id.clone(),
            turn_id: "child-turn".to_string(),
            completed_at_ms: 2,
            item: ThreadItem::AgentMessage {
                id: "message-1".to_string(),
                text: "Review complete.".to_string(),
                phase: None,
                memory_citation: None,
            },
        },
    ));

    let agent = shell
        .agent_activity
        .agent(&child_thread_id)
        .expect("spawned agent should remain tracked");
    assert_eq!(shell.transcript, root_transcript);
    assert_eq!(agent.latest_message.as_deref(), Some("Review complete."));
    assert_eq!(
        agent
            .timeline
            .iter()
            .map(agent_activity::AgentTimelineEntry::label)
            .collect::<Vec<_>>(),
        vec![
            "spawning agent".to_string(),
            "message completed: Review complete.".to_string(),
            "reasoning: checking constraints".to_string(),
            "command output: tests pass".to_string(),
        ]
    );

    shell.handle_notification(ServerNotification::TurnCompleted(
        codex_app_server_protocol::TurnCompletedNotification {
            thread_id: child_thread_id.clone(),
            turn: test_turn("child-turn", TurnStatus::Completed),
        },
    ));
    assert_eq!(
        shell
            .agent_activity
            .agent(&child_thread_id)
            .map(|agent| agent.status),
        Some(agent_activity::AgentLifecycleStatus::Completed)
    );
    shell.handle_notification(ServerNotification::Error(ErrorNotification {
        error: TurnError {
            message: "child failed".to_string(),
            codex_error_info: None,
            additional_details: None,
        },
        will_retry: false,
        thread_id: child_thread_id.clone(),
        turn_id: "child-turn-2".to_string(),
    }));
    let agent = shell
        .agent_activity
        .agent(&child_thread_id)
        .expect("spawned agent should remain tracked");
    assert_eq!(agent.status, agent_activity::AgentLifecycleStatus::Errored);
    assert_eq!(agent.latest_message.as_deref(), Some("child failed"));
}

#[test]
fn child_turn_start_reactivates_a_stopped_agent() {
    let mut shell = ShellState::snapshot_fixture();
    let child_thread_id = "agent-thread".to_string();
    shell
        .agent_activity
        .reduce_completed(&ThreadItem::CollabAgentToolCall {
            id: "agent-state".to_string(),
            tool: CollabAgentTool::Wait,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: shell.thread_id.to_string(),
            receiver_thread_ids: vec![child_thread_id.clone()],
            prompt: None,
            model: None,
            reasoning_effort: None,
            agents_states: HashMap::from([(
                child_thread_id.clone(),
                CollabAgentState {
                    status: CollabAgentStatus::Shutdown,
                    message: None,
                },
            )]),
        });
    shell
        .active_agent_thread_ids
        .insert(child_thread_id.clone());

    shell.handle_notification(ServerNotification::TurnStarted(
        codex_app_server_protocol::TurnStartedNotification {
            thread_id: child_thread_id.clone(),
            turn: test_turn("child-turn", TurnStatus::InProgress),
        },
    ));

    assert_eq!(
        shell
            .agent_activity
            .agent(&child_thread_id)
            .map(|agent| agent.status),
        Some(agent_activity::AgentLifecycleStatus::Running)
    );
}

#[test]
fn child_notifications_discover_nested_agents() {
    let mut shell = ShellState::snapshot_fixture();
    let root_thread_id = shell.thread_id.to_string();
    let child_thread_id = "agent-child".to_string();
    let grandchild_thread_id = "agent-grandchild".to_string();

    shell.handle_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            thread_id: root_thread_id,
            turn_id: "root-turn".to_string(),
            completed_at_ms: 1,
            item: ThreadItem::SubAgentActivity {
                id: "child-started".to_string(),
                kind: SubAgentActivityKind::Started,
                agent_thread_id: child_thread_id.clone(),
                agent_path: "/root/child".to_string(),
            },
        },
    ));
    shell.handle_notification(ServerNotification::ItemStarted(ItemStartedNotification {
        thread_id: child_thread_id.clone(),
        turn_id: "child-turn".to_string(),
        started_at_ms: 2,
        item: ThreadItem::CollabAgentToolCall {
            id: "nested-spawn".to_string(),
            tool: CollabAgentTool::SpawnAgent,
            status: CollabAgentToolCallStatus::InProgress,
            sender_thread_id: child_thread_id.clone(),
            receiver_thread_ids: vec![grandchild_thread_id.clone()],
            prompt: Some("Inspect the nested flow.".to_string()),
            model: Some("gpt-5-codex".to_string()),
            reasoning_effort: Some(ReasoningEffort::High),
            agents_states: Default::default(),
        },
    }));
    shell.handle_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            thread_id: child_thread_id,
            turn_id: "child-turn".to_string(),
            completed_at_ms: 3,
            item: ThreadItem::SubAgentActivity {
                id: "grandchild-started".to_string(),
                kind: SubAgentActivityKind::Started,
                agent_thread_id: grandchild_thread_id.clone(),
                agent_path: "/root/child/grandchild".to_string(),
            },
        },
    ));

    assert!(shell.agent_activity.is_known_thread(&grandchild_thread_id));
    assert_eq!(
        shell
            .agent_activity
            .agent(&grandchild_thread_id)
            .and_then(|agent| agent.path.as_ref())
            .map(ToString::to_string)
            .as_deref(),
        Some("/root/child/grandchild")
    );
    assert_eq!(shell.agent_activity.counts().total, 2);
}

#[test]
fn non_tool_item_starts_do_not_render_as_tool_calls() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.tool_activity.clear();
    shell.subagent_activity.clear();
    let thread_id = shell.thread_id.to_string();

    for item in [
        ThreadItem::UserMessage {
            id: "user-start".to_string(),
            client_id: None,
            content: vec![UserInput::Text {
                text: "hello".to_string(),
                text_elements: Vec::new(),
            }],
        },
        ThreadItem::AgentMessage {
            id: "assistant-start".to_string(),
            text: "working".to_string(),
            phase: None,
            memory_citation: None,
        },
        ThreadItem::Reasoning {
            id: "reasoning-start".to_string(),
            summary: vec!["thinking".to_string()],
            content: Vec::new(),
        },
    ] {
        shell.handle_notification(ServerNotification::ItemStarted(ItemStartedNotification {
            thread_id: thread_id.clone(),
            turn_id: "turn-1".to_string(),
            started_at_ms: 0,
            item,
        }));
    }

    assert_eq!(shell.transcript, VecDeque::new());
    assert_eq!(shell.tool_activity, VecDeque::new());
    assert_eq!(shell.subagent_activity, VecDeque::new());
}

#[test]
fn completed_tool_item_updates_existing_transcript_status() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    let thread_id = shell.thread_id.to_string();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
    );

    shell.handle_notification(ServerNotification::ItemStarted(ItemStartedNotification {
        thread_id: thread_id.clone(),
        turn_id: "turn-1".to_string(),
        started_at_ms: 0,
        item: command_execution_item(
            "exec-1",
            CommandExecutionStatus::InProgress,
            /*exit_code*/ None,
        ),
    }));

    let running_buf = render_shell_buffer(&shell, area);
    assert_eq!(
        accent_color_for_row(&running_buf, area, "exec cargo test"),
        Some(design::palette::CYAN)
    );

    shell.handle_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            thread_id,
            turn_id: "turn-1".to_string(),
            completed_at_ms: 1,
            item: command_execution_item("exec-1", CommandExecutionStatus::Completed, Some(0)),
        },
    ));

    assert_eq!(
        shell.transcript,
        VecDeque::from([
            TranscriptLine::new(TranscriptKind::Tool, "exec cargo test exit 0 42ms")
                .tool_status(ToolBlockStatus::Success)
                .item_id("exec-1")
        ])
    );
    let completed_buf = render_shell_buffer(&shell, area);
    assert_eq!(
        accent_color_for_row(&completed_buf, area, "exec cargo test"),
        Some(design::palette::SUCCESS)
    );
}

#[test]
fn command_output_deltas_update_one_output_block() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    let thread_id = shell.thread_id.to_string();

    shell.handle_notification(ServerNotification::ItemStarted(ItemStartedNotification {
        thread_id: thread_id.clone(),
        turn_id: "turn-1".to_string(),
        started_at_ms: 0,
        item: command_execution_item("exec-1", CommandExecutionStatus::InProgress, None),
    }));
    for delta in ["pytest 40%\r", "pytest 80%\r", "pytest 100%\n"] {
        shell.handle_notification(ServerNotification::CommandExecutionOutputDelta(
            CommandExecutionOutputDeltaNotification {
                thread_id: thread_id.clone(),
                turn_id: "turn-1".to_string(),
                item_id: "exec-1".to_string(),
                delta: delta.to_string(),
            },
        ));
    }

    assert_eq!(
        shell
            .transcript
            .iter()
            .filter(|line| line.kind == TranscriptKind::Output)
            .count(),
        1
    );
    assert_eq!(
        shell.transcript,
        VecDeque::from([
            TranscriptLine::new(TranscriptKind::Tool, "exec cargo test")
                .tool_status(ToolBlockStatus::Running)
                .item_id("exec-1"),
            TranscriptLine::output(
                "pytest 40%\rpytest 80%\rpytest 100%\n",
                ToolBlockStatus::Running,
                "exec-1".to_string(),
            ),
        ])
    );
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
    );
    insta::assert_snapshot!(render_shell(&shell, area));

    shell.handle_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            thread_id,
            turn_id: "turn-1".to_string(),
            completed_at_ms: 1,
            item: ThreadItem::CommandExecution {
                id: "exec-1".to_string(),
                command: "cargo test".to_string(),
                cwd: LegacyAppPathString::from_abs_path(&test_absolute_path(
                    "workspace/better-codex",
                )),
                process_id: None,
                source: CommandExecutionSource::Agent,
                status: CommandExecutionStatus::Completed,
                command_actions: Vec::new(),
                aggregated_output: Some("pytest 100%\n".to_string()),
                exit_code: Some(0),
                duration_ms: Some(42),
            },
        },
    ));

    let mut completed_output = TranscriptLine::output(
        "pytest 100%\n",
        ToolBlockStatus::Success,
        "exec-1".to_string(),
    );
    completed_output.full_text = Some("pytest 40%\rpytest 80%\rpytest 100%\n".to_string().into());
    assert_eq!(
        shell.transcript,
        VecDeque::from([
            TranscriptLine::new(TranscriptKind::Tool, "exec cargo test exit 0 42ms")
                .tool_status(ToolBlockStatus::Success)
                .item_id("exec-1"),
            completed_output,
        ])
    );
}

#[test]
fn command_output_deltas_preserve_newline_chunks() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    let thread_id = shell.thread_id.to_string();

    for delta in ["first", "\n", "second", ""] {
        shell.handle_notification(ServerNotification::CommandExecutionOutputDelta(
            CommandExecutionOutputDeltaNotification {
                thread_id: thread_id.clone(),
                turn_id: "turn-1".to_string(),
                item_id: "exec-lines".to_string(),
                delta: delta.to_string(),
            },
        ));
    }

    assert_eq!(
        shell.transcript,
        VecDeque::from([TranscriptLine::output(
            "first\nsecond",
            ToolBlockStatus::Running,
            "exec-lines".to_string(),
        )])
    );
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
        )
    ));
}

#[test]
fn streaming_tool_output_renders_latest_lines_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    let thread_id = shell.thread_id.to_string();

    for delta in [
        "line 1\nline 2\n",
        "line 3\nline 4\n",
        "line 5\nline 6\nline 7\n",
    ] {
        shell.handle_notification(ServerNotification::CommandExecutionOutputDelta(
            CommandExecutionOutputDeltaNotification {
                thread_id: thread_id.clone(),
                turn_id: "turn-1".to_string(),
                item_id: "exec-tail".to_string(),
                delta: delta.to_string(),
            },
        ));
    }

    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
    );
    let buf = render_shell_buffer(&shell, area);
    let rendered = buffer_contents(&buf, area);
    let omitted_row = row_containing(&buf, area, "output ... 4 earlier output lines")
        .expect("output omission row should render");
    let line_5_row = row_containing(&buf, area, "line 5").expect("line 5 should render");
    let line_6_row = row_containing(&buf, area, "line 6").expect("line 6 should render");
    let line_7_row = row_containing(&buf, area, "line 7").expect("line 7 should render");

    assert_eq!(line_5_row, omitted_row + 1);
    assert_eq!(line_6_row, omitted_row + 2);
    assert_eq!(line_7_row, omitted_row + 3);
    assert!(!rendered.contains("line 1"));
    assert!(!rendered.contains("line 4"));
    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn command_output_transcript_text_is_bounded() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    let thread_id = shell.thread_id.to_string();

    let full_output = (0..200)
        .map(|index| format!("compile line {index:03}: {}\n", "x".repeat(96)))
        .collect::<String>();
    for delta in full_output.split_inclusive('\n') {
        shell.handle_notification(ServerNotification::CommandExecutionOutputDelta(
            CommandExecutionOutputDeltaNotification {
                thread_id: thread_id.clone(),
                turn_id: "turn-1".to_string(),
                item_id: "exec-1".to_string(),
                delta: delta.to_string(),
            },
        ));
    }

    let output = shell
        .transcript
        .back()
        .expect("output line should be present")
        .text
        .clone();
    let mut expected = TranscriptLine::new(TranscriptKind::Output, output.clone())
        .tool_status(ToolBlockStatus::Running)
        .item_id("exec-1");
    expected.full_text = Some(full_output.into());

    assert_eq!(shell.transcript, VecDeque::from([expected]));
    assert!(output.starts_with(TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX));
    assert!(
        output.chars().count()
            <= TRANSCRIPT_OUTPUT_HIGH_WATER_CHARS + TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX.len()
    );
    assert!(!output.contains("compile line 000"));
    assert!(output.contains("compile line 199"));
}

#[tokio::test]
async fn clicking_running_output_opens_a_live_full_output_popup() {
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    let thread_id = shell.thread_id.to_string();
    shell.handle_notification(ServerNotification::ItemStarted(ItemStartedNotification {
        thread_id: thread_id.clone(),
        turn_id: "turn-1".to_string(),
        started_at_ms: 0,
        item: command_execution_item("exec-live", CommandExecutionStatus::InProgress, None),
    }));
    let initial_output = (0..200)
        .map(|index| format!("compile line {index:03}: checking workspace\n"))
        .collect::<String>();
    shell.handle_notification(ServerNotification::CommandExecutionOutputDelta(
        CommandExecutionOutputDeltaNotification {
            thread_id: thread_id.clone(),
            turn_id: "turn-1".to_string(),
            item_id: "exec-live".to_string(),
            delta: initial_output.clone(),
        },
    ));
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
    );
    let output_index = shell
        .transcript
        .iter()
        .position(|line| line.kind == TranscriptKind::Output)
        .expect("output card should exist");
    let position = (area.y..area.bottom())
        .flat_map(|y| (area.x..area.right()).map(move |x| Position::new(x, y)))
        .find(|position| {
            (ShellView { shell: &shell }).transcript_card_at(area, *position)
                == Some(TranscriptCardHit::ToolOutput {
                    transcript_index: output_index,
                })
        })
        .expect("output card should expose a click target");

    shell
        .handle_mouse_click(area, position, &config, &mut backend)
        .await
        .expect("output click should succeed");

    let open = shell
        .tool_output
        .as_ref()
        .expect("output popup should open");
    assert!(open.output().contains("compile line 000"));
    assert!(open.output().contains("compile line 199"));
    assert_eq!(open.target.status, ToolBlockStatus::Running);

    shell.handle_notification(ServerNotification::CommandExecutionOutputDelta(
        CommandExecutionOutputDeltaNotification {
            thread_id: thread_id.clone(),
            turn_id: "turn-1".to_string(),
            item_id: "exec-live".to_string(),
            delta: "compile line 200: finished\n".to_string(),
        },
    ));
    let open = shell
        .tool_output
        .as_ref()
        .expect("live output popup should remain open");
    assert!(open.output().ends_with("compile line 200: finished\n"));

    let completed_output = format!("{initial_output}compile line 200: finished\n");
    shell.handle_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            thread_id,
            turn_id: "turn-1".to_string(),
            completed_at_ms: 1,
            item: ThreadItem::CommandExecution {
                id: "exec-live".to_string(),
                command: "cargo test".to_string(),
                cwd: LegacyAppPathString::from_abs_path(&test_absolute_path(
                    "workspace/better-codex",
                )),
                process_id: None,
                source: CommandExecutionSource::Agent,
                status: CommandExecutionStatus::Completed,
                command_actions: Vec::new(),
                aggregated_output: Some(completed_output.clone()),
                exit_code: Some(0),
                duration_ms: Some(42),
            },
        },
    ));
    let open = shell
        .tool_output
        .as_ref()
        .expect("completed output popup should remain open");
    assert_eq!(open.output(), completed_output);
    assert_eq!(open.target.status, ToolBlockStatus::Success);

    shell
        .handle_mouse_click(area, Position::new(/*x*/ 0, /*y*/ 0), &config, &mut backend)
        .await
        .expect("outside click should succeed");
    assert!(shell.tool_output.is_none());
}

#[test]
fn command_output_compaction_retains_the_low_water_line_tail() {
    let output = (0..=TRANSCRIPT_OUTPUT_HIGH_WATER_LINES)
        .map(|line| format!("compile line {line:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let retained_from = TRANSCRIPT_OUTPUT_HIGH_WATER_LINES
        .saturating_add(1)
        .saturating_sub(TRANSCRIPT_OUTPUT_LOW_WATER_LINES);
    let retained = (retained_from..=TRANSCRIPT_OUTPUT_HIGH_WATER_LINES)
        .map(|line| format!("compile line {line:03}"))
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(
        compact_output_for_transcript(output),
        format!("{TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX}{retained}")
    );
}

#[test]
fn command_output_compaction_waits_for_the_high_water_mark_after_trimming() {
    let oversized = "a".repeat(TRANSCRIPT_OUTPUT_HIGH_WATER_CHARS + 1);
    let compacted = compact_output_for_transcript(oversized);
    let retained = compacted
        .strip_prefix(TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX)
        .expect("oversized output should be marked as compacted");
    assert_eq!(retained.chars().count(), TRANSCRIPT_OUTPUT_LOW_WATER_CHARS);

    let delta = "b".repeat(TRANSCRIPT_OUTPUT_HIGH_WATER_CHARS - TRANSCRIPT_OUTPUT_LOW_WATER_CHARS);
    let at_high_water = format!("{compacted}{delta}");
    assert_eq!(
        compact_output_for_transcript(at_high_water.clone()),
        at_high_water
    );

    let compacted_again = compact_output_for_transcript(format!("{at_high_water}c"));
    let retained_again = compacted_again
        .strip_prefix(TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX)
        .expect("output above the high water mark should be compacted again");
    assert_eq!(
        retained_again.chars().count(),
        TRANSCRIPT_OUTPUT_LOW_WATER_CHARS
    );
    assert!(retained_again.ends_with('c'));
}

#[test]
fn legacy_command_exec_output_deltas_update_one_output_block() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();

    for output in ["tqdm 1/3\r", "tqdm 2/3\r", "tqdm 3/3\n"] {
        shell.handle_notification(ServerNotification::CommandExecOutputDelta(
            CommandExecOutputDeltaNotification {
                process_id: "process-1".to_string(),
                stream: CommandExecOutputStream::Stdout,
                delta_base64: base64::engine::general_purpose::STANDARD.encode(output),
                cap_reached: false,
            },
        ));
    }

    assert_eq!(
        shell.transcript,
        VecDeque::from([TranscriptLine::output(
            "tqdm 1/3\rtqdm 2/3\rtqdm 3/3\n",
            ToolBlockStatus::Running,
            "command-exec:process-1".to_string(),
        )])
    );
}

#[tokio::test]
async fn workspace_refresh_waits_until_active_turn_finishes() {
    let mut shell = ShellState::snapshot_fixture();
    shell.workspace_command_runner = Some(Arc::new(NoopWorkspaceRunner));
    let mut backend = RecordingBackend::default();
    let thread_id = shell.thread_id.to_string();
    shell.active_turn_id = Some("turn-1".to_string());
    shell.workspace_status_refresh_due = false;

    shell
        .handle_app_server_event(
            &mut backend,
            AppServerEvent::ServerNotification(ServerNotification::TurnDiffUpdated(
                TurnDiffUpdatedNotification {
                    thread_id: thread_id.clone(),
                    turn_id: "turn-1".to_string(),
                    diff: "@@\n-old\n+new\n".to_string(),
                },
            )),
        )
        .await
        .expect("diff update should be handled");

    assert_eq!(shell.workspace_status_refresh_due, true);
    assert_eq!(shell.workspace_git_status, None);

    shell
        .handle_app_server_event(
            &mut backend,
            AppServerEvent::ServerNotification(ServerNotification::TurnCompleted(
                codex_app_server_protocol::TurnCompletedNotification {
                    thread_id,
                    turn: test_turn("turn-1", TurnStatus::Completed),
                },
            )),
        )
        .await
        .expect("turn completion should schedule workspace status refresh");

    assert!(shell.workspace_status_refresh_due);
    assert!(shell.has_pending_session_hydration());
    assert_eq!(shell.workspace_git_status, None);

    finish_session_hydration(&mut shell, &backend).await;

    assert!(!shell.workspace_status_refresh_due);
    assert_eq!(
        shell.workspace_git_status,
        Some(WorkspaceGitStatus::default())
    );
}

#[test]
fn turn_diff_updates_dashboard_without_conversation_diff_box() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    let thread_id = shell.thread_id.to_string();

    shell.handle_notification(ServerNotification::TurnDiffUpdated(
        TurnDiffUpdatedNotification {
            thread_id,
            turn_id: "turn-1".to_string(),
            diff: "+added\n-removed\n+added again\n".to_string(),
        },
    ));

    assert_eq!(
        shell.latest_diff,
        Some(DiffSummary {
            files: 0,
            additions: 2,
            removals: 1,
        })
    );
    assert_eq!(shell.transcript, VecDeque::new());
}

#[test]
fn file_change_patch_updates_one_diff_box_per_item() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.tool_activity.clear();
    let thread_id = shell.thread_id.to_string();

    for diff in ["+first\n", "+second\n"] {
        shell.handle_notification(ServerNotification::FileChangePatchUpdated(
            FileChangePatchUpdatedNotification {
                thread_id: thread_id.clone(),
                turn_id: "turn-1".to_string(),
                item_id: "file-1".to_string(),
                changes: vec![FileUpdateChange {
                    path: "src/lib.rs".to_string(),
                    kind: PatchChangeKind::Update { move_path: None },
                    diff: diff.to_string(),
                }],
            },
        ));
    }

    assert_eq!(
        shell.transcript,
        VecDeque::from([TranscriptLine::new(
            TranscriptKind::Diff,
            "1 files +1 -0\n  M src/lib.rs"
        )
        .tool_status(ToolBlockStatus::Running)
        .item_id("file-1")])
    );
    assert_eq!(shell.tool_activity, VecDeque::new());
}

#[test]
fn file_change_notifications_render_only_the_edited_log_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.tool_activity.clear();
    let thread_id = shell.thread_id.to_string();
    let changes = sample_file_changes();

    shell.handle_notification(ServerNotification::ItemStarted(ItemStartedNotification {
        thread_id: thread_id.clone(),
        turn_id: "turn-1".to_string(),
        started_at_ms: 0,
        item: ThreadItem::FileChange {
            id: "file-1".to_string(),
            changes: changes.clone(),
            status: codex_app_server_protocol::PatchApplyStatus::InProgress,
        },
    }));
    shell.handle_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            thread_id,
            turn_id: "turn-1".to_string(),
            completed_at_ms: 1,
            item: ThreadItem::FileChange {
                id: "file-1".to_string(),
                changes: changes.clone(),
                status: codex_app_server_protocol::PatchApplyStatus::Completed,
            },
        },
    ));

    assert_eq!(
        shell.transcript,
        VecDeque::from([
            TranscriptLine::new(TranscriptKind::Diff, file_change_detail(&changes))
                .tool_status(ToolBlockStatus::Success)
                .item_id("file-1")
        ])
    );
    assert_eq!(shell.tool_activity, VecDeque::new());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
    );
    insta::assert_snapshot!(render_shell(&shell, area));
}

#[tokio::test]
async fn clicking_edited_card_opens_and_refreshes_the_diff_popup() {
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    let thread_id = shell.thread_id.to_string();
    let change = FileUpdateChange {
        path: "src/lib.rs".to_string(),
        kind: PatchChangeKind::Update { move_path: None },
        diff: "@@ -1 +1 @@\n-before\n+after\n".to_string(),
    };
    shell.handle_notification(ServerNotification::FileChangePatchUpdated(
        FileChangePatchUpdatedNotification {
            thread_id: thread_id.clone(),
            turn_id: "turn-1".to_string(),
            item_id: "file-1".to_string(),
            changes: vec![change],
        },
    ));
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
    );
    let diff_index = shell
        .transcript
        .iter()
        .position(|line| line.kind == TranscriptKind::Diff)
        .expect("edited card should exist");
    let position = (area.y..area.bottom())
        .flat_map(|y| (area.x..area.right()).map(move |x| Position::new(x, y)))
        .find(|position| {
            (ShellView { shell: &shell }).transcript_card_at(area, *position)
                == Some(TranscriptCardHit::Diff {
                    transcript_index: diff_index,
                })
        })
        .expect("edited card should expose a click target");

    shell
        .handle_mouse_click(area, position, &config, &mut backend)
        .await
        .expect("edited card click should succeed");

    let open = shell.diff_view.as_ref().expect("diff popup should open");
    assert_eq!(open.source_item_id(), Some("file-1"));
    assert_eq!(open.files().len(), 1);
    assert_eq!(
        open.selected_file().and_then(|file| file.old_label()),
        Some("src/lib.rs")
    );
    open.set_scroll_max(12);
    open.scroll_down(/*amount*/ 4);
    assert_eq!(open.scroll(), 4);

    shell.handle_notification(ServerNotification::FileChangePatchUpdated(
        FileChangePatchUpdatedNotification {
            thread_id,
            turn_id: "turn-1".to_string(),
            item_id: "file-1".to_string(),
            changes: vec![FileUpdateChange {
                path: "src/lib.rs".to_string(),
                kind: PatchChangeKind::Update { move_path: None },
                diff: "@@ -1 +1,2 @@\n-before\n+after\n+again\n".to_string(),
            }],
        },
    ));

    let open = shell
        .diff_view
        .as_ref()
        .expect("live diff popup should remain open");
    assert_eq!(open.scroll(), 0);
    assert!(open.selected_file().is_some_and(|file| {
        file.rows()
            .iter()
            .any(|row| row.new.as_ref().is_some_and(|cell| cell.text == "again"))
    }));

    shell
        .handle_mouse_click(area, Position::new(/*x*/ 0, /*y*/ 0), &config, &mut backend)
        .await
        .expect("outside click should succeed");
    assert!(shell.diff_view.is_none());

    shell.transcript_selection = Some(diff_index);
    assert_eq!(
        shell.handle_transcript_selection_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE,)),
        Some(false)
    );
    assert!(shell.diff_view.is_some());
}

#[tokio::test]
async fn clicking_status_edits_opens_every_session_file() {
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
    shell.composer.clear();
    shell.record_file_changes(
        "turn-1",
        "file-1",
        &sample_file_changes(),
        codex_app_server_protocol::PatchApplyStatus::Completed,
    );
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 140, /*height*/ 40,
    );
    let position = (area.y..area.bottom())
        .flat_map(|y| (area.x..area.right()).map(move |x| Position::new(x, y)))
        .find(|position| {
            (ShellView { shell: &shell })
                .dashboard_panel_position_at(area, *position, "Edits")
                .is_some()
        })
        .expect("Edits panel should expose a click target");

    shell
        .handle_mouse_click(area, position, &config, &mut backend)
        .await
        .expect("Edits click should succeed");

    let open = shell.diff_view.as_ref().expect("session diff should open");
    assert_eq!(open.source_item_id(), None);
    assert_eq!(open.files().len(), sample_file_changes().len());
}

#[test]
fn repeated_c_quoted_session_edits_render_as_one_net_file_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.record_turn_diff(
        "turn-1",
        "diff --git \"a/src/tab\\tname.rs\" \"b/src/tab\\tname.rs\"\n--- \"a/src/tab\\tname.rs\"\n+++ \"b/src/tab\\tname.rs\"\n@@ -1 +1 @@\n-old\n+middle\n",
    );
    shell.record_turn_diff(
        "turn-2",
        "diff --git \"a/src/tab\\tname.rs\" \"b/src/tab\\tname.rs\"\n--- \"a/src/tab\\tname.rs\"\n+++ \"b/src/tab\\tname.rs\"\n@@ -1 +1 @@\n-middle\n+final\n",
    );

    assert!(shell.open_session_diff_view());
    assert_eq!(
        shell
            .diff_view
            .as_ref()
            .expect("session diff should open")
            .files()
            .len(),
        1
    );
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
        ),
    ));
}

#[test]
fn mouse_wheel_routes_between_diff_files_and_content_in_both_layouts() {
    for area in [
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 140, /*height*/ 34,
        ),
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 54, /*height*/ 16,
        ),
    ] {
        let mut shell = ShellState::snapshot_fixture();
        shell.diff_view = Some(DiffViewState::new(
            "Session edits",
            /*source_item_id*/ None,
            (0..30)
                .map(|file| {
                    super::diff_view::DiffFile::added(
                        format!("src/file_{file:02}.rs"),
                        (0..50)
                            .map(|line| format!("file {file:02} line {line:02}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        super::diff_view::DiffStatus::Completed,
                    )
                })
                .collect(),
        ));
        render_shell(&shell, area);
        let selector = super::diff_view_view::diff_view_file_selector_area(area);
        let selector_header = Position::new(selector.x, selector.y);
        let selector_body = Position::new(selector.x, selector.bottom().saturating_sub(1));
        let diff_body = Position::new(
            selector.right().saturating_add(1),
            selector.bottom().saturating_sub(1),
        );

        shell.handle_mouse_scroll(area, diff_body, tui::MouseScrollDirection::Down);
        shell.handle_mouse_scroll(area, selector_header, tui::MouseScrollDirection::Up);
        let view = shell
            .diff_view
            .as_ref()
            .expect("diff view should stay open");
        assert_eq!((view.selected_file_index(), view.scroll()), (0, 3));

        shell.handle_mouse_scroll(area, selector_body, tui::MouseScrollDirection::Down);
        let view = shell
            .diff_view
            .as_ref()
            .expect("diff view should stay open");
        assert_eq!((view.selected_file_index(), view.scroll()), (1, 0));
        render_shell(&shell, area);
        shell.handle_mouse_scroll(area, diff_body, tui::MouseScrollDirection::Down);
        shell.handle_mouse_scroll(area, selector_header, tui::MouseScrollDirection::Down);
        let view = shell
            .diff_view
            .as_ref()
            .expect("diff view should stay open");
        assert_eq!((view.selected_file_index(), view.scroll()), (2, 0));

        for _ in 0..40 {
            shell.handle_mouse_scroll(area, selector_body, tui::MouseScrollDirection::Down);
        }
        let view = shell
            .diff_view
            .as_ref()
            .expect("diff view should stay open");
        assert_eq!((view.selected_file_index(), view.scroll()), (29, 0));
        let rendered = render_shell(&shell, area);
        if area.width == 54 {
            insta::assert_snapshot!("wheel_scrolled_diff_file_selector", rendered);
        }

        shell.handle_mouse_scroll(area, diff_body, tui::MouseScrollDirection::Down);
        shell.handle_mouse_scroll(area, selector_body, tui::MouseScrollDirection::Down);
        let view = shell
            .diff_view
            .as_ref()
            .expect("diff view should stay open");
        assert_eq!((view.selected_file_index(), view.scroll()), (29, 3));

        shell.handle_mouse_scroll(area, selector_body, tui::MouseScrollDirection::Up);
        let view = shell
            .diff_view
            .as_ref()
            .expect("diff view should stay open");
        assert_eq!((view.selected_file_index(), view.scroll()), (28, 0));
    }
}

#[test]
fn historical_edits_hydrate_and_session_switch_clears_them() {
    let mut shell = ShellState::snapshot_fixture();
    let mut turn = test_turn("historical-turn", TurnStatus::Completed);
    turn.items.push(ThreadItem::FileChange {
        id: "historical-file".to_string(),
        changes: vec![FileUpdateChange {
            path: "src/history.rs".to_string(),
            kind: PatchChangeKind::Add,
            diff: "first\nsecond\n".to_string(),
        }],
        status: codex_app_server_protocol::PatchApplyStatus::Completed,
    });
    let mut started = started_thread(
        "history",
        test_thread_id("01900000-0000-7000-8000-000000000421"),
        /*forked_from_id*/ None,
    );
    started.turns = vec![turn];

    shell.replace_started_session(started);

    assert_eq!(
        shell.diff_store.session_stats(),
        super::diff_view::DiffStats {
            files: 1,
            additions: 2,
            removals: 0,
        }
    );
    assert!(shell.open_session_diff_view());

    shell.replace_started_session(started_thread(
        "empty",
        test_thread_id("01900000-0000-7000-8000-000000000422"),
        /*forked_from_id*/ None,
    ));

    assert!(!shell.diff_store.has_session_edits());
    assert!(shell.diff_view.is_none());
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
fn tool_transcript_items_render_with_single_blank_row_gaps() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.composer.clear();
    shell.streaming_assistant.clear();
    shell.push_assistant("assistant done");
    shell.push_tool_with_status("exec one", ToolBlockStatus::Running);
    shell.push_tool_with_status("exec two", ToolBlockStatus::Success);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 24,
    );

    let rendered = render_shell(&shell, area);

    assert_single_blank_row_between(&rendered, "assistant done", "exec one");
    assert_single_blank_row_between(&rendered, "exec one", "exec two");
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
    shell.dashboard_route = DashboardRoute::Status;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 190, /*height*/ 36,
    );

    let rendered = render_shell(&shell, area);

    assert!(rendered.contains("gpt-5-codex-dashboard-detail"));
    assert!(rendered.contains("/workspace/better-codex/codex-rs/tui"));
    assert!(rendered.contains("feature/dashboard-width-budget"));
    assert!(rendered.contains("exec just test -p codex-tui app_shell_tests"));
}

#[test]
fn dashboard_compacts_token_counts_and_groups_other_large_numbers() {
    let mut shell = ShellState::snapshot_fixture();
    shell.token_usage = TokenUsage {
        input_tokens: 1_234_567,
        cached_input_tokens: 100_000,
        output_tokens: 234_567,
        reasoning_output_tokens: 12_345,
        total_tokens: 1_469_134,
    };
    shell.context_token_usage = shell.token_usage.clone();
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
    shell.dashboard_route = DashboardRoute::Status;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 130, /*height*/ 48,
    );

    let rendered = render_shell(&shell, area);

    assert!(rendered.contains("input 1.2m | output 235k"));
    assert!(rendered.contains("Context 27% left"));
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

    shell.scroll_transcript_down(/*rows*/ 3);
    assert_eq!(shell.transcript_scroll, 7);

    shell.transcript_scroll = 100;
    shell.transcript_scroll_max.set(10);
    shell.scroll_transcript_down(/*rows*/ 3);
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
        transcript_view::transcript_scrollbar_metrics(
            /*total_lines*/ 40, /*visible_count*/ 10, /*visible_from*/ 0,
            /*min_thumb_height*/ 2
        ),
        Some(TranscriptScrollbarMetrics {
            thumb_top: 0,
            thumb_height: 3,
        })
    );
    assert_eq!(
        transcript_view::transcript_scrollbar_metrics(
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
        transcript_view::transcript_scrollbar_metrics(
            /*total_lines*/ 1_000, /*visible_count*/ 10, /*visible_from*/ 500,
            /*min_thumb_height*/ 2
        ),
        Some(TranscriptScrollbarMetrics {
            thumb_top: 4,
            thumb_height: 2,
        })
    );
    assert_eq!(
        transcript_view::transcript_scrollbar_metrics(
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
        dashboard::context_used_percent(&TokenUsage::default(), /*model_context_window*/ None,),
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

#[tokio::test]
async fn token_usage_notification_uses_last_usage_for_context_pressure() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();

    shell
        .handle_app_server_event(
            &mut backend,
            AppServerEvent::ServerNotification(ServerNotification::ThreadTokenUsageUpdated(
                codex_app_server_protocol::ThreadTokenUsageUpdatedNotification {
                    thread_id: shell.thread_id.to_string(),
                    turn_id: "turn-context".to_string(),
                    token_usage: codex_app_server_protocol::ThreadTokenUsage {
                        total: codex_app_server_protocol::TokenUsageBreakdown {
                            total_tokens: 900_000,
                            input_tokens: 800_000,
                            cached_input_tokens: 100_000,
                            output_tokens: 100_000,
                            reasoning_output_tokens: 0,
                        },
                        last: codex_app_server_protocol::TokenUsageBreakdown {
                            total_tokens: 24_000,
                            input_tokens: 20_000,
                            cached_input_tokens: 4_000,
                            output_tokens: 4_000,
                            reasoning_output_tokens: 0,
                        },
                        model_context_window: Some(200_000),
                    },
                },
            )),
        )
        .await
        .expect("token usage notification should be handled");

    assert_eq!(shell.token_usage.total_tokens, 900_000);
    assert_eq!(
        dashboard::context_used_percent(&shell.context_token_usage, shell.model_context_window),
        Some(6)
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

#[tokio::test]
async fn tab_queues_multiple_messages_only_during_an_active_turn() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.active_turn_id = Some("turn-active".to_string());
    shell.composer.clear();

    for message in ["first queued", "second queued"] {
        shell.composer.set_text(message);
        shell
            .handle_key(
                KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .expect("Tab should queue the message");
    }

    assert_eq!(shell.composer.queued_count(), 2);
    assert_eq!(shell.composer.text(), "");
    assert_eq!(backend.calls(), Vec::new());

    shell.active_turn_id = None;
    shell.composer.set_text("draft");
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("idle Tab should indent the draft");

    assert_eq!(shell.composer.queued_count(), 2);
    assert_eq!(shell.composer.text(), "draft    ");
    assert_eq!(backend.calls(), Vec::new());
}

#[tokio::test]
async fn alt_arrows_traverse_queued_messages_without_selecting_the_transcript() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.active_turn_id = Some("turn-active".to_string());
    shell.composer.clear();
    queue_messages(&mut shell.composer, &["first", "second", "third"]);
    shell.composer.set_text("ordinary draft");
    shell.transcript_selection = Some(0);
    let alt_up = KeyEvent::new(KeyCode::Up, KeyModifiers::ALT);
    let alt_down = KeyEvent::new(KeyCode::Down, KeyModifiers::ALT);

    shell
        .handle_key(alt_up, &config, &mut backend)
        .await
        .expect("Alt+Up should edit the newest queued message");
    assert_eq!(shell.composer.text(), "third");
    assert_eq!(shell.transcript_selection, None);
    shell.composer.insert_str(" updated");
    shell
        .handle_key(alt_up, &config, &mut backend)
        .await
        .expect("Alt+Up should move to the previous queued message");
    assert_eq!(shell.composer.text(), "second");
    shell
        .handle_key(alt_down, &config, &mut backend)
        .await
        .expect("Alt+Down should move to the next queued message");
    assert_eq!(shell.composer.text(), "third updated");
    shell
        .handle_key(alt_down, &config, &mut backend)
        .await
        .expect("Alt+Down should leave queue editing after the newest message");

    assert_eq!(shell.composer.text(), "ordinary draft");
    assert_eq!(shell.composer.queued_edit_position(), None);
    assert_eq!(shell.composer.queued_count(), 3);
    assert_eq!(backend.calls(), Vec::new());
}

#[tokio::test]
async fn completed_turns_submit_queued_messages_fifo_and_preserve_the_draft() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.active_turn_id = Some("turn-current".to_string());
    shell.composer.clear();
    queue_messages(&mut shell.composer, &["first queued", "second queued"]);
    shell.composer.set_text("ordinary draft");
    assert!(shell.composer.edit_previous_queued_message());
    shell.composer.insert_str(" updated");

    shell
        .handle_app_server_event(
            &mut backend,
            turn_completed_event(shell.thread_id, "turn-current", TurnStatus::Completed),
        )
        .await
        .expect("completion should submit the first queued message");
    complete_backend_actions(&mut shell, &backend).await;

    assert_eq!(shell.composer.text(), "ordinary draft");
    assert_eq!(shell.composer.queued_count(), 1);
    assert_eq!(shell.active_turn_id.as_deref(), Some("turn-submit"));

    shell
        .handle_app_server_event(
            &mut backend,
            turn_completed_event(shell.thread_id, "turn-submit", TurnStatus::Failed),
        )
        .await
        .expect("failure should submit the next queued message");
    complete_backend_actions(&mut shell, &backend).await;

    let prompts = backend
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            RecordedBackendCall::TurnStart { prompt, .. } => Some(prompt),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        prompts,
        vec![
            "first queued".to_string(),
            "second queued updated".to_string(),
        ]
    );
    assert_eq!(shell.composer.text(), "ordinary draft");
    assert_eq!(shell.composer.queued_count(), 0);
    assert_eq!(shell.active_turn_id.as_deref(), Some("turn-submit"));
}

#[tokio::test]
async fn interrupted_turn_retains_queue_until_idle_enter_resumes_it() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.active_turn_id = Some("turn-current".to_string());
    shell.composer.clear();
    queue_messages(&mut shell.composer, &["first queued", "second queued"]);
    assert!(shell.composer.edit_previous_queued_message());
    shell.composer.set_text("edited second");

    shell
        .handle_app_server_event(
            &mut backend,
            turn_completed_event(shell.thread_id, "turn-current", TurnStatus::Interrupted),
        )
        .await
        .expect("interruption should retain queued messages");

    assert_eq!(backend.calls(), Vec::new());
    assert_eq!(shell.composer.queued_count(), 2);
    assert_eq!(shell.active_turn_id, None);

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("idle Enter should save the edit and resume the queue");
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.composer.queued_edit_position(), None);
    assert_eq!(shell.composer.queued_count(), 1);
    shell
        .handle_app_server_event(
            &mut backend,
            turn_completed_event(shell.thread_id, "turn-submit", TurnStatus::Completed),
        )
        .await
        .expect("completion should continue the queue");
    complete_backend_actions(&mut shell, &backend).await;

    let prompts = backend
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            RecordedBackendCall::TurnStart { prompt, .. } => Some(prompt),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        prompts,
        vec!["first queued".to_string(), "edited second".to_string()]
    );
    assert_eq!(shell.composer.queued_count(), 0);
}

#[tokio::test]
async fn failed_idle_queue_resume_reports_error_and_retains_message() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.clear();
    shell.composer.set_text("queued");
    assert!(shell.composer.queue_current_message());
    let transcript_len = shell.transcript.len();
    backend.fail_next_turn_start("turn start failed");

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("failed queue resume should remain in the TUI");
    complete_backend_actions(&mut shell, &backend).await;

    assert_eq!(shell.composer.queued_count(), 1);
    assert_eq!(shell.composer.text(), "");
    assert_eq!(shell.active_turn_id, None);
    assert_eq!(shell.status, "action failed");
    assert_eq!(shell.transcript.len(), transcript_len + 1);
    assert_eq!(
        shell.transcript.back().map(|line| line.kind),
        Some(TranscriptKind::Error)
    );
    assert_eq!(
        shell.transcript.back().map(|line| line.text.as_str()),
        Some("failed to submit turn: turn start failed")
    );
}

#[tokio::test]
async fn pending_turn_start_keeps_input_responsive() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = None;
    shell.composer.set_text("first request");
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let backend = RecordingBackend {
        turn_start_gate: Some(Arc::clone(&gate)),
        ..RecordingBackend::default()
    };

    tokio::time::timeout(
        std::time::Duration::from_millis(50),
        shell.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend.clone(),
        ),
    )
    .await
    .expect("turn submission should not block input")
    .expect("turn submission should be handled");
    let thread_id = shell.thread_id;
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
            &config,
            &mut backend.clone(),
        )
        .await
        .expect("session switching should remain safely blocked");
    assert_eq!(shell.thread_id, thread_id);
    shell
        .handle_key(key_char('n'), &config, &mut backend.clone())
        .await
        .expect("typing should remain responsive");

    assert!(shell.has_pending_backend_actions());
    assert_eq!(shell.composer.text(), "n");
    gate.add_permits(1);
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.active_turn_id.as_deref(), Some("turn-submit"));
    assert_eq!(shell.composer.text(), "n");
}

#[tokio::test]
async fn rejected_turn_restores_draft_and_renders_error_snapshot() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = None;
    shell.transcript.clear();
    shell.dashboard_visible = false;
    shell.composer.set_text("retry this request");
    let backend = RecordingBackend::default();
    backend.fail_next_turn_start("request rejected");

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend.clone(),
        )
        .await
        .expect("rejected turn should remain in the TUI");
    complete_backend_actions(&mut shell, &backend).await;

    assert_eq!(shell.composer.text(), "retry this request");
    assert_eq!(shell.active_turn_id, None);
    assert_eq!(
        shell.transcript.back().map(|line| line.kind),
        Some(TranscriptKind::Error)
    );
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 20,
        ),
    ));
}

#[tokio::test]
async fn failed_approval_response_keeps_modal_open() {
    let mut shell = ShellState::snapshot_fixture();
    let backend = RecordingBackend::default();
    shell
        .handle_app_server_event(
            &mut backend.clone(),
            AppServerEvent::ServerRequest(command_approval_request()),
        )
        .await
        .expect("approval request should open");
    backend.fail_next_action("approval response rejected");

    shell
        .resolve_pending_approval(&backend, /*option_index*/ 0, None)
        .expect("approval response should start");
    complete_backend_actions(&mut shell, &backend).await;

    assert!(shell.pending_approval.is_some());
    assert_eq!(shell.status, "action failed");
}

#[tokio::test]
async fn failed_settings_write_keeps_selector_and_previous_value() {
    let mut shell = ShellState::snapshot_fixture();
    let previous_policy = shell.approval_policy;
    let backend = RecordingBackend::default();
    shell.open_approval_selector();
    backend.fail_next_action("config write rejected");

    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend.clone(),
        )
        .await
        .expect("setting update should start");
    complete_backend_actions(&mut shell, &backend).await;

    assert!(shell.selector.is_some());
    assert_eq!(shell.approval_policy, previous_policy);
    assert_eq!(shell.status, "action failed");
}

#[tokio::test]
async fn failed_thread_settings_update_restores_config_and_previous_value() {
    let mut shell = ShellState::snapshot_fixture();
    let previous_policy = shell.approval_policy;
    let backend = RecordingBackend::default();
    backend
        .config_values
        .lock()
        .expect("config values should lock")
        .insert(
            "approval_policy".to_string(),
            serde_json::to_value(previous_policy).expect("approval policy should serialize"),
        );
    shell.open_approval_selector();
    backend.fail_next_thread_settings_update("thread update rejected");

    for key in [KeyCode::Down, KeyCode::Down, KeyCode::Enter] {
        shell
            .handle_selector_key(KeyEvent::new(key, KeyModifiers::NONE), &mut backend.clone())
            .await
            .expect("setting update should start");
    }
    complete_backend_actions(&mut shell, &backend).await;

    assert!(shell.selector.is_some());
    assert_eq!(shell.approval_policy, previous_policy);
    assert_eq!(shell.status, "action failed");
    assert_eq!(
        *backend
            .config_values
            .lock()
            .expect("config values should lock"),
        HashMap::from([(
            "approval_policy".to_string(),
            serde_json::to_value(previous_policy).expect("approval policy should serialize"),
        )])
    );
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ConfigWrite(vec![(
                "approval_policy".to_string(),
                serde_json::json!("never"),
            )]),
            RecordedBackendCall::ThreadSettingsUpdate {
                model: None,
                effort: None,
                service_tier: None,
                approval_policy: codex_app_server_protocol::AskForApproval::Never,
            },
            RecordedBackendCall::ConfigWrite(vec![(
                "approval_policy".to_string(),
                serde_json::to_value(previous_policy).expect("approval policy should serialize"),
            )]),
        ]
    );
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
        ),
    ));
}

#[tokio::test]
async fn failed_session_delete_keeps_confirmation_open() {
    let mut shell = ShellState::snapshot_fixture();
    let backend = RecordingBackend::default();
    let pending = PendingSessionDelete {
        thread_id: test_thread_id("01900000-0000-7000-8000-000000000099"),
        title: "keep this session".to_string(),
        descendant_count: 2,
    };
    shell.pending_session_delete = Some(pending.clone());
    backend.fail_next_action("delete rejected");

    shell
        .handle_session_delete_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend.clone(),
        )
        .await
        .expect("delete should start");
    complete_backend_actions(&mut shell, &backend).await;

    assert_eq!(shell.pending_session_delete, Some(pending));
    assert_eq!(shell.status, "action failed");
}

#[tokio::test]
async fn shift_enter_preserves_multiline_composer_when_typing() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.clear();

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
            &config,
            &mut backend,
        )
        .await
        .expect("shift enter should insert a newline");
    shell
        .handle_key(key_char('a'), &config, &mut backend)
        .await
        .expect("typing after a newline should edit the second line");

    assert_eq!(
        (
            shell.composer.text().to_string(),
            shell.composer.cursor_position()
        ),
        ("\na".to_string(), (1, 1))
    );

    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let rendered = render_shell(&shell, area);

    assert!(
        rendered.contains("MESSAGE  2:2"),
        "composer should render the cursor on the second line"
    );
    assert!(
        rendered.contains("  a"),
        "composer should keep typed text on the second line"
    );
    let view = ShellView { shell: &shell };
    let buf = render_shell_buffer(&shell, area);
    let input_area = view.input_area(area);
    let row = row_containing(&buf, input_area, "  a").expect("typed text row should render");
    let text_x =
        row_needle_x(&buf, input_area, row, "a").expect("typed text should have x position");
    let cursor = view
        .cursor_position(area)
        .expect("composer cursor should be visible");

    assert_eq!(cursor.y, row);
    assert_eq!(cursor.x, text_x + 1);
}

#[tokio::test]
async fn alt_enter_inserts_a_newline_without_submitting() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.set_text("first");

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
            &config,
            &mut backend,
        )
        .await
        .expect("alt enter should insert a newline");

    assert_eq!(
        (shell.composer.text(), shell.composer.cursor_position()),
        ("first\n", (1, 0))
    );
    assert_eq!(backend.calls(), Vec::new());
}

#[tokio::test]
async fn alt_enter_inserts_a_newline_during_tool_input() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.pending_user_input = PendingUserInput::from_request(&tool_user_input_request());
    shell.composer.set_text("details");

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
            &config,
            &mut backend,
        )
        .await
        .expect("alt enter should insert a tool-input newline");

    assert_eq!(
        (
            shell.pending_user_input.is_some(),
            shell.composer.text(),
            shell.composer.cursor_position(),
            backend.calls(),
        ),
        (true, "details\n", (1, 0), Vec::new())
    );
}

#[tokio::test]
async fn repeated_shift_enter_keeps_blank_line_cursor_visible() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.clear();

    for _ in 0..8 {
        shell
            .handle_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
                &config,
                &mut backend,
            )
            .await
            .expect("shift enter should insert a newline");
    }

    assert_eq!(shell.composer.cursor_position(), (8, 0));

    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let view = ShellView { shell: &shell };
    let buf = render_shell_buffer(&shell, area);
    let input_area = view.input_area(area);
    let title_row = row_containing(&buf, input_area, "MESSAGE  9:1")
        .expect("composer should render the ninth logical line");
    let cursor = view
        .cursor_position(area)
        .expect("composer cursor should be visible");

    assert!(title_row <= 18, "composer panel should grow upward");
    assert!(cursor.y > title_row);
    assert_eq!(cursor.x, 3);
}

#[test]
fn composer_cursor_position_tracks_text_end_without_synthetic_glyph() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.set_text("alpha\nbeta");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let view = ShellView { shell: &shell };
    let buf = render_shell_buffer(&shell, area);
    let input_area = view.input_area(area);
    let row =
        row_containing(&buf, input_area, "  beta").expect("second composer row should render");
    let text_x =
        row_needle_x(&buf, input_area, row, "beta").expect("typed text should have x position");
    let cursor = view
        .cursor_position(area)
        .expect("composer cursor should be visible");

    assert_eq!(cursor.y, row);
    assert_eq!(cursor.x, text_x + 4);
    assert!(!buffer_contents(&buf, area).contains("beta▌"));
}

#[test]
fn composer_cursor_tracks_word_wrapped_single_line_prompt() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.set_text(
        "This deliberately long one-line prompt wraps before the final words on another row",
    );
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let view = ShellView { shell: &shell };
    let buf = render_shell_buffer(&shell, area);
    let input_area = view.input_area(area);
    let first_row = row_containing(&buf, input_area, "This deliberately")
        .expect("prompt start should render on a visible row");
    let first_x = row_needle_x(&buf, input_area, first_row, "This deliberately")
        .expect("prompt start should have an x position");
    let row = row_containing(&buf, input_area, "another row")
        .expect("wrapped prompt tail should render on a visible row");
    let wrapped_x = row_needle_x(&buf, input_area, row, "before the final")
        .expect("wrapped prompt line should have an x position");
    let tail_x = row_needle_x(&buf, input_area, row, "another row")
        .expect("wrapped prompt tail should have an x position");
    let cursor = view
        .cursor_position(area)
        .expect("composer cursor should be visible");

    assert_eq!(wrapped_x, first_x);
    assert_eq!(cursor.y, row);
    assert_eq!(cursor.x, tail_x + 11);
    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn composer_cursor_wraps_at_a_boundary_space() {
    let mut shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let input = ShellView { shell: &shell }.input_area(area);
    let body = design::body_rect_after_title(design::pane_content_rect(input));
    let content_width = usize::from(body.width).saturating_sub(2);
    shell.composer.set_text("x".repeat(content_width));
    let boundary = ShellView { shell: &shell }
        .cursor_position(area)
        .expect("boundary cursor should be visible");

    shell.composer.insert_char(' ');
    let cursor = ShellView { shell: &shell }
        .cursor_position(area)
        .expect("wrapped cursor should be visible");
    let wrapped = composer_render::wrapped_composer_lines(
        shell.composer.text(),
        shell.composer.is_empty(),
        shell.composer.cursor(),
        usize::from(body.width),
    );
    let continuation = Position::new(body.x.saturating_add(2), body.y.saturating_add(1));
    let after_space = Position::new(continuation.x.saturating_add(1), continuation.y);

    assert_eq!(
        (wrapped.len(), boundary, cursor, body.contains(cursor)),
        (2, continuation, after_space, true)
    );
    let mut buf = render_shell_buffer(&shell, area);
    buf[cursor].set_symbol("▌");
    insta::assert_snapshot!(buffer_contents(&buf, area));

    shell.composer.insert_char('X');
    let buf = render_shell_buffer(&shell, area);
    assert_eq!(
        (
            row_needle_x(&buf, body, continuation.y, "X"),
            ShellView { shell: &shell }.cursor_position(area)
        ),
        (
            Some(after_space.x),
            Some(Position::new(
                after_space.x.saturating_add(1),
                after_space.y
            ))
        )
    );
}

#[test]
fn composer_preserves_multiple_boundary_spaces_before_text() {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    for space_count in [1, 3] {
        let mut shell = ShellState::snapshot_fixture();
        let input = ShellView { shell: &shell }.input_area(area);
        let body = design::body_rect_after_title(design::pane_content_rect(input));
        let content_width = usize::from(body.width).saturating_sub(2);
        shell.composer.set_text(format!(
            "{}{}Z",
            "x".repeat(content_width),
            " ".repeat(space_count)
        ));
        shell.composer.move_left();

        let wrapped = composer_render::wrapped_composer_lines(
            shell.composer.text(),
            shell.composer.is_empty(),
            shell.composer.cursor(),
            usize::from(body.width),
        );
        let cursor = ShellView { shell: &shell }
            .cursor_position(area)
            .expect("boundary-space cursor should be visible");
        let buf = render_shell_buffer(&shell, area);
        let expected = Position::new(
            body.x
                .saturating_add(2)
                .saturating_add(u16::try_from(space_count).unwrap_or(u16::MAX)),
            body.y.saturating_add(1),
        );

        assert_eq!(
            (
                space_count,
                wrapped.len(),
                cursor,
                row_needle_x(&buf, body, expected.y, "Z")
            ),
            (space_count, 2, expected, Some(expected.x))
        );
    }
}

#[test]
fn composer_wraps_multirow_spaces_and_wide_text() {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let shell = ShellState::snapshot_fixture();
    let input = ShellView { shell: &shell }.input_area(area);
    let body = design::body_rect_after_title(design::pane_content_rect(input));
    let content_width = usize::from(body.width).saturating_sub(2);
    let cases = [
        (
            format!(
                "{}{}Z",
                "x".repeat(content_width),
                " ".repeat(content_width.saturating_add(1))
            ),
            3,
            2,
        ),
        (format!("{} Z", "界".repeat(content_width / 2)), 2, 1),
    ];

    for (text, expected_rows, expected_line) in cases {
        let mut shell = ShellState::snapshot_fixture();
        shell.composer.set_text(text);
        shell.composer.move_left();
        let wrapped = composer_render::wrapped_composer_lines(
            shell.composer.text(),
            shell.composer.is_empty(),
            shell.composer.cursor(),
            usize::from(body.width),
        );
        let cursor_line = composer_render::composer_visual_cursor_line(
            shell.composer.text(),
            shell.composer.cursor(),
            usize::from(body.width),
        );
        let z_column = wrapped[expected_line].to_string().find('Z');

        assert_eq!(
            (wrapped.len(), cursor_line, z_column),
            (expected_rows, Some(expected_line), Some(3))
        );
    }
}

#[test]
fn composer_boundary_space_precedes_the_next_logical_line() {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    for (space, expected_rows, cursor_offset, expected_foo_line) in [("", 2, 0, 1), (" ", 3, 1, 2)]
    {
        let mut shell = ShellState::snapshot_fixture();
        let input = ShellView { shell: &shell }.input_area(area);
        let body = design::body_rect_after_title(design::pane_content_rect(input));
        let content_width = usize::from(body.width).saturating_sub(2);
        shell
            .composer
            .set_text(format!("{}{space}\nfoo", "x".repeat(content_width)));
        for _ in 0..4 {
            shell.composer.move_left();
        }

        let wrapped = composer_render::wrapped_composer_lines(
            shell.composer.text(),
            shell.composer.is_empty(),
            shell.composer.cursor(),
            usize::from(body.width),
        );
        let cursor = ShellView { shell: &shell }
            .cursor_position(area)
            .expect("cursor before newline should be visible");
        let content_x = body.x.saturating_add(2);
        let expected_cursor = Position::new(
            content_x.saturating_add(cursor_offset),
            body.y.saturating_add(1),
        );
        let foo_line = wrapped
            .iter()
            .position(|line| line.to_string().contains("foo"));

        assert_eq!(
            (space, wrapped.len(), cursor, foo_line),
            (
                space,
                expected_rows,
                expected_cursor,
                Some(expected_foo_line)
            )
        );
    }
}

#[test]
fn long_pasted_single_line_exposes_every_wrapped_row_at_navigation_extents() {
    const ROWS: usize = 12;

    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 24,
    );
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.composer.clear();
    let initial_input = ShellView { shell: &shell }.input_area(area);
    let initial_body = design::body_rect_after_title(design::pane_content_rect(initial_input));
    let content_width = usize::from(initial_body.width).saturating_sub(2);
    let mut pasted = (0..ROWS - 1)
        .map(|row| format!("R{row:02}{}", "x".repeat(content_width.saturating_sub(3))))
        .collect::<String>();
    pasted.push_str("R11TAIL");
    shell.insert_pasted_text(&pasted);

    let input = ShellView { shell: &shell }.input_area(area);
    let body = design::body_rect_after_title(design::pane_content_rect(input));
    let wrapped = composer_render::wrapped_composer_lines(
        shell.composer.text(),
        shell.composer.is_empty(),
        shell.composer.cursor(),
        usize::from(body.width),
    );
    assert_eq!((input.height, body.height, wrapped.len()), (12, 9, ROWS));

    let mut bottom = render_shell_buffer(&shell, area);
    let tail_row = row_containing(&bottom, body, "R11TAIL");
    let tail_x = tail_row.and_then(|row| row_needle_x(&bottom, body, row, "R11TAIL"));
    let cursor = ShellView { shell: &shell }
        .cursor_position(area)
        .expect("pasted-text cursor should be visible");
    let expected_tail_row = body.bottom().saturating_sub(1);
    let expected_tail_x = body.x.saturating_add(2);
    assert_eq!(
        (
            row_containing(&bottom, body, "R02"),
            row_containing(&bottom, body, "R03"),
            tail_row,
            tail_x,
            cursor,
        ),
        (
            None,
            Some(body.y),
            Some(expected_tail_row),
            Some(expected_tail_x),
            Position::new(expected_tail_x.saturating_add(7), expected_tail_row),
        )
    );

    while shell.composer.cursor() > 0 {
        shell.composer.move_left();
    }
    let top = render_shell_buffer(&shell, area);
    assert_eq!(
        (
            row_containing(&top, body, "R00"),
            row_containing(&top, body, "R08"),
            row_containing(&top, body, "R09"),
            ShellView { shell: &shell }.cursor_position(area),
        ),
        (
            Some(body.y),
            Some(body.bottom().saturating_sub(1)),
            None,
            Some(Position::new(body.x.saturating_add(2), body.y)),
        )
    );
    let visible_rows = (0..ROWS)
        .filter(|row| {
            let marker = format!("R{row:02}");
            row_containing(&top, body, &marker).is_some()
                || row_containing(&bottom, body, &marker).is_some()
        })
        .collect::<Vec<_>>();
    assert_eq!(visible_rows, (0..ROWS).collect::<Vec<_>>());

    while shell.composer.cursor() < pasted.len() {
        shell.composer.move_right();
    }
    assert_eq!(
        ShellView { shell: &shell }.cursor_position(area),
        Some(cursor)
    );
    bottom[cursor].set_symbol("▌");
    insta::assert_snapshot!(
        "long_pasted_composer_bottom",
        buffer_contents(&bottom, area)
    );
}

#[test]
fn dashboard_focus_keeps_context_readable_and_hides_composer_cursor() {
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let view = ShellView { shell: &shell };
    let buf = render_shell_buffer(&shell, area);
    let conversation_row =
        row_containing(&buf, area, "CONVERSATION").expect("conversation title should render");
    let conversation_x = row_needle_x(&buf, area, conversation_row, "CONVERSATION")
        .expect("conversation title should have an x position");
    let dashboard_row =
        row_containing(&buf, area, "Sessions").expect("dashboard tabs should render");
    let dashboard_x = row_needle_x(&buf, area, dashboard_row, "Sessions")
        .expect("active tab should have an x position");

    assert!(
        !buf[(conversation_x, conversation_row)]
            .style()
            .add_modifier
            .contains(Modifier::DIM)
    );
    let background_x = conversation_x + "CONVERSATION".len() as u16 + 1;
    assert_eq!(buf[(background_x, conversation_row)].symbol(), " ");
    assert_eq!(
        buf[(background_x, conversation_row)].style().bg,
        Some(design::palette::BASE)
    );
    assert!(
        !buf[(dashboard_x, dashboard_row)]
            .style()
            .add_modifier
            .contains(Modifier::DIM)
    );
    assert_eq!(view.cursor_position(area), None);
    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn composer_highlights_recognized_slash_commands_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.set_text("/goal Keep the dashboard compact");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let view = ShellView { shell: &shell };
    let buf = render_shell_buffer(&shell, area);
    let input_area = view.input_area(area);
    let row = row_containing(&buf, input_area, "/goal Keep the dashboard compact")
        .expect("slash command should render in the composer");
    let slash_x = row_needle_x(&buf, input_area, row, "/goal")
        .expect("slash command should have an x position");

    assert_eq!(buf[(slash_x, row)].style().fg, Some(design::palette::FOCUS));

    shell.composer.set_text("/clear");
    let clear_buf = render_shell_buffer(&shell, area);
    let clear_row = row_containing(&clear_buf, input_area, "/clear")
        .expect("clear command should render in the composer");
    let clear_x = row_needle_x(&clear_buf, input_area, clear_row, "/clear")
        .expect("clear command should have an x position");
    assert_eq!(
        clear_buf[(clear_x, clear_row)].style().fg,
        Some(design::palette::FOCUS)
    );

    shell.composer.set_text("/exit");
    let exit_buf = render_shell_buffer(&shell, area);
    let exit_row = row_containing(&exit_buf, input_area, "/exit")
        .expect("exit command should render in the composer");
    let exit_x = row_needle_x(&exit_buf, input_area, exit_row, "/exit")
        .expect("exit command should have an x position");
    assert_eq!(
        exit_buf[(exit_x, exit_row)].style().fg,
        Some(design::palette::FOCUS)
    );

    shell.composer.set_text("/goal Keep the dashboard compact");
    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn composer_does_not_highlight_unknown_slash_commands() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.set_text("/unknown argument");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let view = ShellView { shell: &shell };
    let buf = render_shell_buffer(&shell, area);
    let input_area = view.input_area(area);
    let row = row_containing(&buf, input_area, "/unknown argument")
        .expect("unknown slash-prefixed text should render");
    let slash_x = row_needle_x(&buf, input_area, row, "/unknown")
        .expect("slash-prefixed text should have an x position");

    assert_eq!(buf[(slash_x, row)].style().fg, Some(design::palette::TEXT));
}

#[test]
fn composer_highlights_shell_operator_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.set_text("! printf hello");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let view = ShellView { shell: &shell };
    let buf = render_shell_buffer(&shell, area);
    let input_area = view.input_area(area);
    let row = row_containing(&buf, input_area, "! printf hello")
        .expect("shell command should render in the composer");
    let operator_x = row_needle_x(&buf, input_area, row, "! printf hello")
        .expect("shell command should have an x position");

    assert_eq!(
        buf[(operator_x, row)].style().fg,
        Some(design::palette::FOCUS)
    );
    assert_eq!(
        buf[(operator_x + 1, row)].style().fg,
        Some(design::palette::TEXT)
    );
    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn command_approval_serializes_accept_and_deny() {
    let pending = PendingApproval::from_request(&command_approval_request())
        .expect("approval request should be valid")
        .expect("request should be supported");

    assert_eq!(
        pending.result(0).expect("approval should serialize"),
        json!({ "decision": "accept" })
    );
    assert_eq!(
        pending.result(1).expect("denial should serialize"),
        json!({ "decision": "cancel" })
    );
}

#[test]
fn command_approval_honors_restricted_available_decisions() {
    let mut request = command_approval_request();
    let ServerRequest::CommandExecutionRequestApproval { params, .. } = &mut request else {
        panic!("expected command approval request");
    };
    params.available_decisions = Some(vec![
        CommandExecutionApprovalDecision::AcceptForSession,
        CommandExecutionApprovalDecision::Decline,
    ]);
    let pending = PendingApproval::from_request(&request)
        .expect("approval request should be valid")
        .expect("request should be supported");

    assert_eq!(
        pending.options().collect::<Vec<_>>(),
        vec![(0, "Run for this session"), (1, "Deny")]
    );
    assert_eq!(
        pending.result(0).expect("approval should serialize"),
        json!({ "decision": "acceptForSession" })
    );
    assert_eq!(
        pending.result(1).expect("denial should serialize"),
        json!({ "decision": "decline" })
    );
}

#[test]
fn file_change_approval_serializes_all_decisions() {
    let pending = PendingApproval::from_request(&file_change_approval_request())
        .expect("approval request should be valid")
        .expect("request should be supported");

    assert_eq!(
        (0..pending.option_count())
            .map(|index| pending.result(index).expect("decision should serialize"))
            .collect::<Vec<_>>(),
        vec![
            json!({ "decision": "accept" }),
            json!({ "decision": "acceptForSession" }),
            json!({ "decision": "decline" }),
            json!({ "decision": "cancel" }),
        ]
    );
}

#[test]
fn permissions_approval_serializes_grant_and_empty_deny() {
    let pending = PendingApproval::from_request(&permissions_approval_request())
        .expect("approval request should be valid")
        .expect("request should be supported");

    assert_eq!(
        pending.result(0).expect("approval should serialize"),
        json!({
            "permissions": {
                "network": { "enabled": true }
            },
            "scope": "turn"
        })
    );
    assert_eq!(
        pending.result(1).expect("denial should serialize"),
        json!({
            "permissions": {
                "network": { "enabled": true }
            },
            "scope": "session"
        })
    );
    assert_eq!(
        pending.result(2).expect("strict grant should serialize"),
        json!({
            "permissions": {
                "network": { "enabled": true }
            },
            "scope": "turn",
            "strictAutoReview": true
        })
    );
    assert_eq!(
        pending.result(3).expect("denial should serialize"),
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
            "Run command: cargo test -p codex-tui - Reason: Needs network access - Working directory: /workspace/better-codex".to_string(),
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
fn user_input_serializes_other_answer() {
    let mut pending = PendingUserInput::from_request(&tool_user_input_request())
        .expect("request should be supported");

    assert_eq!(
        pending
            .answer_current("Use the canary environment".to_string())
            .expect("other answer should serialize"),
        UserInputAdvance::Complete {
            request_id: RequestId::Integer(43),
            result: json!({
                "answers": {
                    "environment": {
                        "answers": ["user_note: Use the canary environment"]
                    }
                }
            })
        }
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn user_input_auto_resolves_only_after_its_deadline() {
    let mut shell = ShellState::snapshot_fixture();
    let backend = RecordingBackend::default();
    shell.pending_user_input = PendingUserInput::from_request(
        &tool_user_input_request_with_auto_resolution(/*auto_resolution_ms*/ 60_000),
    );

    assert!(!shell.start_expired_user_input_resolution(&backend));
    tokio::time::advance(Duration::from_millis(/*millis*/ 59_999)).await;
    assert!(!shell.start_expired_user_input_resolution(&backend));
    tokio::time::advance(Duration::from_millis(/*millis*/ 1)).await;
    assert!(shell.start_expired_user_input_resolution(&backend));
    assert!(shell.has_pending_backend_action(ActionGroup::UserInput));
    complete_backend_actions(&mut shell, &backend).await;

    assert!(shell.pending_user_input.is_none());
    assert_eq!(
        backend
            .resolved_requests
            .lock()
            .expect("resolved requests should lock")
            .clone(),
        vec![(
            RequestId::Integer(43),
            json!({
                "answers": {}
            }),
        )]
    );
    assert_eq!(
        shell.transcript.back(),
        Some(&TranscriptLine::new(
            TranscriptKind::Audit,
            "tool input auto-resolved: Tool input: tool-input-1",
        ))
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
fn mcp_elicitation_mouse_columns_use_display_width() {
    let pending = PendingElicitation::from_request(&mcp_url_elicitation_request())
        .expect("request should be supported");
    let actions = "   Accept ↵   Decline d   Cancel c ";
    let decline = actions
        .find("Decline d")
        .expect("decline action should exist");
    let column = unicode_width::UnicodeWidthStr::width(&actions[..decline]);

    assert_eq!(
        pending.choice_at(/*line*/ 3, column),
        Some(ElicitationChoice::Decline)
    );
}

#[tokio::test]
async fn enter_accepts_url_mcp_elicitation() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.pending_elicitation = PendingElicitation::from_request(&mcp_url_elicitation_request());

    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("Enter should accept the elicitation");

    assert!(shell.pending_elicitation.is_none());
    assert_eq!(
        backend
            .resolved_requests
            .lock()
            .expect("requests should lock")
            .as_slice(),
        &[(
            RequestId::Integer(45),
            json!({
                "action": "accept",
                "content": null,
                "_meta": null
            }),
        )]
    );
}

#[tokio::test]
async fn mcp_structured_forms_collect_typed_content_and_openai_choices() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    shell
        .handle_app_server_event(
            &mut backend,
            AppServerEvent::ServerRequest(mcp_form_elicitation_request()),
        )
        .await
        .expect("structured form should open");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 60, /*height*/ 20,
    );
    insta::assert_snapshot!("structured_mcp_form", render_shell(&shell, area));

    shell.composer.set_text("owner@example.com");
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    shell
        .handle_key(enter, &config, &mut backend)
        .await
        .expect("Enter should advance the form");
    for answer in ["1,2", "", ""] {
        shell.composer.set_text(answer);
        shell
            .resolve_pending_elicitation(&mut backend, ElicitationChoice::Accept)
            .await
            .expect("form answer should resolve");
    }
    shell.pending_elicitation = PendingElicitation::from_request(&mcp_rich_elicitation_request());
    shell.composer.set_text("1");
    shell
        .resolve_pending_elicitation(&mut backend, ElicitationChoice::Accept)
        .await
        .expect("OpenAI form choice should resolve");

    let resolved = backend
        .resolved_requests
        .lock()
        .expect("requests should lock");
    assert_eq!(
        resolved.as_slice(),
        &[
            (
                RequestId::Integer(47),
                json!({
                    "action": "accept",
                    "content": {
                        "email": "owner@example.com",
                        "regions": ["us", "eu"],
                        "retries": 3,
                        "subscribe": true
                    },
                    "_meta": null
                }),
            ),
            (
                RequestId::Integer(46),
                json!({
                    "action": "accept",
                    "content": { "template": "monthly-review" },
                    "_meta": null
                }),
            ),
        ]
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
10 files +10 -0
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

async fn complete_backend_actions(shell: &mut ShellState, backend: &RecordingBackend) {
    tokio::time::timeout(Duration::from_secs(/*secs*/ 1), async {
        while shell.has_pending_backend_actions() {
            tokio::task::yield_now().await;
            shell.poll_backend_actions(backend).await;
        }
    })
    .await
    .expect("background action should complete");
}

fn position_in(area: Rect, predicate: impl Fn(Position) -> bool) -> Position {
    (area.x..area.right())
        .flat_map(|x| (area.y..area.bottom()).map(move |y| Position::new(x, y)))
        .find(|position| predicate(*position))
        .expect("matching shell position should exist")
}

fn rendered_text_position(rendered: &str, needle: &str) -> Position {
    rendered
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            let start = line.find(needle)?;
            let x = unicode_width::UnicodeWidthStr::width(&line[..start])
                + unicode_width::UnicodeWidthStr::width(needle) / 2;
            Some(Position::new(
                u16::try_from(x).unwrap_or(u16::MAX),
                u16::try_from(y).unwrap_or(u16::MAX),
            ))
        })
        .expect("rendered text should contain target")
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
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

fn text_color_for_row(buf: &Buffer, area: Rect, needle: &str) -> Option<Color> {
    let row = row_containing(buf, area, needle)?;
    let x = row_needle_x(buf, area, row, needle)?;
    buf.cell((x, row))?.style().fg
}

fn accent_x_for_row(buf: &Buffer, area: Rect, y: u16) -> Option<u16> {
    for x in area.x..area.right() {
        let cell = buf.cell((x, y))?;
        if cell.symbol() == "▌" {
            return Some(x);
        }
    }
    None
}

fn rightmost_bg_x_for_row(buf: &Buffer, area: Rect, y: u16, background: Color) -> Option<u16> {
    (area.x..area.right()).rev().find(|x| {
        buf.cell((*x, y))
            .is_some_and(|cell| cell.style().bg == Some(background))
    })
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

fn row_containing(buf: &Buffer, area: Rect, needle: &str) -> Option<u16> {
    for y in area.y..area.bottom() {
        let mut row = String::new();
        for x in area.x..area.right() {
            if let Some(cell) = buf.cell((x, y)) {
                row.push_str(cell.symbol());
            }
        }
        if row.contains(needle) {
            return Some(y);
        }
    }
    None
}

fn row_needle_x(buf: &Buffer, area: Rect, y: u16, needle: &str) -> Option<u16> {
    let mut row = String::new();
    for x in area.x..area.right() {
        if let Some(cell) = buf.cell((x, y)) {
            row.push_str(cell.symbol());
        }
    }
    row.find(needle).and_then(|offset| {
        area.x
            .checked_add(u16::try_from(row[..offset].chars().count()).ok()?)
    })
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

fn assert_single_blank_row_between(rendered: &str, first: &str, second: &str) {
    let rows = rendered.lines().collect::<Vec<_>>();
    let first_index = rows
        .iter()
        .position(|row| row.contains(first))
        .unwrap_or_else(|| panic!("missing rendered row containing {first:?}"));
    let second_index = rows
        .iter()
        .position(|row| row.contains(second))
        .unwrap_or_else(|| panic!("missing rendered row containing {second:?}"));

    assert_eq!(second_index, first_index + 2);
}

fn key_char(ch: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty())
}

fn safety_buffering_notification(
    shell: &ShellState,
    turn_id: &str,
    show_buffering_ui: bool,
    faster_model: Option<&str>,
) -> ModelSafetyBufferingUpdatedNotification {
    ModelSafetyBufferingUpdatedNotification {
        thread_id: shell.thread_id.to_string(),
        turn_id: turn_id.to_string(),
        model: shell.model.clone(),
        use_cases: Vec::new(),
        reasons: Vec::new(),
        show_buffering_ui,
        faster_model: faster_model.map(str::to_string),
    }
}

fn model_preset_fixture(
    slug: &str,
    show_in_picker: bool,
    default_reasoning_effort: ReasoningEffort,
    supported_reasoning_efforts: &[ReasoningEffort],
    service_tiers: &[&str],
) -> ModelPreset {
    ModelPreset {
        id: slug.to_string(),
        model: slug.to_string(),
        display_name: slug.to_string(),
        description: format!("{slug} description"),
        default_reasoning_effort,
        supported_reasoning_efforts: supported_reasoning_efforts
            .iter()
            .cloned()
            .map(|effort| ReasoningEffortPreset {
                description: format!("{effort} reasoning"),
                effort,
            })
            .collect(),
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

fn collaboration_mode_fixture(
    model: &str,
    reasoning_effort: Option<ReasoningEffort>,
) -> Box<CollaborationMode> {
    Box::new(CollaborationMode {
        mode: ModeKind::Default,
        settings: CollaborationModeSettings {
            model: model.to_string(),
            reasoning_effort,
            developer_instructions: None,
        },
    })
}

fn command_approval_request() -> ServerRequest {
    ServerRequest::CommandExecutionRequestApproval {
        request_id: RequestId::Integer(41),
        params: CommandExecutionRequestApprovalParams {
            thread_id: SNAPSHOT_THREAD_ID.to_string(),
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
            available_decisions: Some(vec![
                CommandExecutionApprovalDecision::Accept,
                CommandExecutionApprovalDecision::Cancel,
            ]),
        },
    }
}

fn permissions_approval_request() -> ServerRequest {
    ServerRequest::PermissionsRequestApproval {
        request_id: RequestId::Integer(42),
        params: PermissionsRequestApprovalParams {
            thread_id: SNAPSHOT_THREAD_ID.to_string(),
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

fn file_change_approval_request() -> ServerRequest {
    ServerRequest::FileChangeRequestApproval {
        request_id: RequestId::Integer(44),
        params: codex_app_server_protocol::FileChangeRequestApprovalParams {
            thread_id: SNAPSHOT_THREAD_ID.to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "patch-1".to_string(),
            started_at_ms: 0,
            reason: Some("Update the dashboard layout".to_string()),
            grant_root: Some(test_absolute_path("workspace/better-codex").to_path_buf()),
        },
    }
}

fn tool_user_input_request() -> ServerRequest {
    ServerRequest::ToolRequestUserInput {
        request_id: RequestId::Integer(43),
        params: ToolRequestUserInputParams {
            thread_id: SNAPSHOT_THREAD_ID.to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "tool-input-1".to_string(),
            questions: vec![ToolRequestUserInputQuestion {
                id: "environment".to_string(),
                header: "Environment".to_string(),
                question: "Which environment should the tool use?".to_string(),
                is_other: true,
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

fn tool_user_input_request_with_auto_resolution(auto_resolution_ms: u64) -> ServerRequest {
    let mut request = tool_user_input_request();
    let ServerRequest::ToolRequestUserInput { params, .. } = &mut request else {
        unreachable!("tool user input fixture should return a tool input request");
    };
    params.auto_resolution_ms = Some(auto_resolution_ms);
    request
}

fn tool_free_form_user_input_request() -> ServerRequest {
    ServerRequest::ToolRequestUserInput {
        request_id: RequestId::Integer(44),
        params: ToolRequestUserInputParams {
            thread_id: SNAPSHOT_THREAD_ID.to_string(),
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
            thread_id: SNAPSHOT_THREAD_ID.to_string(),
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
            thread_id: SNAPSHOT_THREAD_ID.to_string(),
            turn_id: Some("turn-1".to_string()),
            server_name: "payments".to_string(),
            request: McpServerElicitationRequest::OpenAiForm {
                meta: None,
                message: "Collect billing contact details.".to_string(),
                requested_schema: json!({
                    "type": "object",
                    "properties": {
                        "template": {
                            "type": "openai/imagePicker",
                            "items": [{ "id": "monthly-review" }]
                        }
                    }
                }),
            },
        },
    }
}

fn mcp_form_elicitation_request() -> ServerRequest {
    ServerRequest::McpServerElicitationRequest {
        request_id: RequestId::Integer(47),
        params: McpServerElicitationRequestParams {
            thread_id: SNAPSHOT_THREAD_ID.to_string(),
            turn_id: Some("turn-1".to_string()),
            server_name: "deployments".to_string(),
            request: McpServerElicitationRequest::Form {
                meta: None,
                message: "Configure the deployment notification.".to_string(),
                requested_schema: serde_json::from_value(json!({
                    "type": "object",
                    "properties": {
                        "email": { "type": "string", "title": "Owner email" },
                        "regions": {
                            "type": "array",
                            "items": { "type": "string", "enum": ["us", "eu", "apac"] }
                        },
                        "retries": { "type": "integer", "default": 3 },
                        "subscribe": { "type": "boolean", "default": true }
                    },
                    "required": ["email", "regions"]
                }))
                .expect("valid MCP form schema"),
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

fn command_execution_item(
    id: &str,
    status: CommandExecutionStatus,
    exit_code: Option<i32>,
) -> ThreadItem {
    ThreadItem::CommandExecution {
        id: id.to_string(),
        command: "cargo test".to_string(),
        cwd: LegacyAppPathString::from_abs_path(&test_absolute_path("workspace/better-codex")),
        process_id: None,
        source: CommandExecutionSource::Agent,
        status,
        command_actions: Vec::new(),
        aggregated_output: None,
        exit_code,
        duration_ms: Some(42),
    }
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
                plugin_summary_fixture(
                    "plugin-calendar",
                    "Calendar",
                    /*installed*/ true,
                    /*enabled*/ true,
                ),
                plugin_summary_fixture(
                    "plugin-drive",
                    "Drive",
                    /*installed*/ false,
                    /*enabled*/ false,
                ),
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
        version: None,
        local_version: None,
        name: name.to_string(),
        share_context: None,
        source: PluginSource::Local {
            path: test_absolute_path(&format!("codex-home/plugins/{id}")),
        },
        installed,
        enabled,
        install_policy: PluginInstallPolicy::Available,
        install_policy_source: None,
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
async fn ctrl_n_starts_a_clear_session_with_current_settings() {
    let config = test_config().await;
    let initial_id = test_thread_id(SNAPSHOT_THREAD_ID);
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.model = "gpt-current".to_string();
    shell.reasoning_effort = Some(ReasoningEffort::High);
    shell.service_tier = Some("priority".to_string());
    shell.approval_policy = codex_app_server_protocol::AskForApproval::Never;
    shell.cwd = "/workspace/current".to_string();
    shell.runtime_workspace_roots = vec![test_absolute_path("workspace/current")];
    let mut backend = RecordingBackend::default();

    let should_exit = shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .expect("new session should start");

    assert!(!should_exit);
    assert_eq!(
        backend.calls().first(),
        Some(&RecordedBackendCall::Start(Some(ThreadStartSource::Clear)))
    );
    let start_configs = backend.start_configs();
    let started_config = start_configs
        .first()
        .expect("start config should be recorded");
    assert_eq!(
        (
            started_config.model.as_deref(),
            started_config.model_reasoning_effort.as_ref(),
            started_config.service_tier.as_deref(),
            started_config.permissions.approval_policy.value(),
            started_config.cwd.as_path(),
            started_config.workspace_roots.as_slice(),
            started_config.workspace_roots_explicit,
        ),
        (
            Some("gpt-current"),
            Some(&ReasoningEffort::High),
            Some("priority"),
            codex_app_server_protocol::AskForApproval::Never.to_core(),
            std::path::Path::new("/workspace/current"),
            [test_absolute_path("workspace/current")].as_slice(),
            true,
        )
    );
    assert_ne!(shell.thread_id, initial_id);
    assert!(!shell.session_list.focused);
    assert!(
        ShellView { shell: &shell }
            .cursor_position(Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
            ))
            .is_some()
    );
}

#[tokio::test]
async fn new_session_is_blocked_during_an_active_turn() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.active_turn_id = Some("turn-active".to_string());
    let initial_id = shell.thread_id;
    let mut backend = RecordingBackend::default();

    let should_exit = shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .expect("blocked new session should be handled");

    assert!(!should_exit);
    assert_eq!(shell.thread_id, initial_id);
    assert_eq!(backend.calls(), Vec::new());
    assert!(shell.transcript.iter().any(|line| {
        line.kind == TranscriptKind::Status
            && line.text == "finish or interrupt the active turn before switching sessions"
    }));
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

    refresh_session_list(&mut shell, &backend).await;
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
            cursor: None,
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
    finish_session_hydration(&mut shell, &backend).await;
    shell
        .handle_session_list_key(key_char('a'), &config, &mut backend)
        .await
        .expect("archive should resolve");
    shell
        .handle_session_list_key(key_char('v'), &config, &mut backend)
        .await
        .expect("archived view should load");
    finish_session_hydration(&mut shell, &backend).await;
    shell
        .handle_session_list_key(key_char('u'), &config, &mut backend)
        .await
        .expect("unarchive should resolve");
    shell
        .handle_session_list_key(key_char('v'), &config, &mut backend)
        .await
        .expect("active view should reload");
    finish_session_hydration(&mut shell, &backend).await;
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
    complete_backend_actions(&mut shell, &backend).await;
    shell
        .handle_session_list_key(key_char('d'), &config, &mut backend)
        .await
        .expect("delete confirmation should open");
    complete_backend_actions(&mut shell, &backend).await;
    shell
        .handle_session_delete_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("delete should resolve");
    complete_backend_actions(&mut shell, &backend).await;

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
async fn committed_session_search_loads_beyond_initial_page_and_clears_remotely() {
    let config = test_config().await;
    let target_id = test_thread_id("01900000-0000-7000-8000-000000000799");
    let mut threads = (0..20)
        .map(|index| {
            let thread_id = test_thread_id(&format!("01900000-0000-7000-8000-{index:012x}"));
            thread_fixture(
                thread_id,
                Some(&format!("session {index}")),
                "ordinary preview",
            )
        })
        .collect::<Vec<_>>();
    threads[0].name = Some("Needle case mismatch".to_string());
    threads[1]
        .git_info
        .as_mut()
        .expect("thread should have git info")
        .branch = Some("needle-branch-only".to_string());
    threads[2].cwd = test_absolute_path("workspace/needle-cwd-only");
    threads.push(thread_fixture(
        target_id,
        Some("needle target"),
        "match beyond the initial page",
    ));
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    let mut backend = RecordingBackend::with_threads(threads);

    refresh_session_list(&mut shell, &backend).await;
    assert!(
        shell.session_list.lines(/*width*/ 80)[0]
            .to_string()
            .contains("20+ sessions")
    );
    shell
        .handle_session_list_key(key_char('/'), &config, &mut backend)
        .await
        .expect("search mode should start");
    shell
        .handle_session_list_key(
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("backspace on an empty query should be a no-op");
    for ch in "needle".chars() {
        shell
            .handle_session_list_key(key_char(ch), &config, &mut backend)
            .await
            .expect("typing should filter locally");
    }
    assert_eq!(shell.session_list.selected_thread_id(), None);
    assert!(
        shell.session_list.lines(/*width*/ 80)[2]
            .to_string()
            .contains("filter* needle▏  · Enter search all")
    );
    insta::assert_snapshot!(
        "session_filter_contract",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
            )
        )
    );
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::ThreadList {
            archived: Some(false),
            search_term: None,
            cursor: None,
        }]
    );

    shell
        .handle_session_list_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("committed search should refresh from the backend");
    finish_session_hydration(&mut shell, &backend).await;
    assert_eq!(shell.session_list.selected_thread_id(), Some(target_id));
    assert!(
        shell.session_list.lines(/*width*/ 80)[2]
            .to_string()
            .contains("search needle  · server results")
    );

    shell
        .handle_session_list_key(key_char('/'), &config, &mut backend)
        .await
        .expect("committed search should reopen");
    shell
        .handle_session_list_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("clearing search should restore the unfiltered page");
    finish_session_hydration(&mut shell, &backend).await;
    assert_ne!(shell.session_list.selected_thread_id(), Some(target_id));
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: None,
            },
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: Some("needle".to_string()),
                cursor: None,
            },
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: None,
            },
        ]
    );
}

#[tokio::test]
async fn session_navigation_appends_the_next_page_snapshot() {
    let config = test_config().await;
    let threads = (0..25)
        .map(|index| {
            thread_fixture(
                test_thread_id(&format!("01900000-0000-7000-8001-{index:012x}")),
                Some(&format!("Session {index:02}")),
                "pagination fixture",
            )
        })
        .collect::<Vec<_>>();
    let expected_id = ThreadId::from_string(&threads[20].id).expect("thread id should be valid");
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    let mut backend = RecordingBackend::with_threads(threads);

    refresh_session_list(&mut shell, &backend).await;
    for _ in 0..20 {
        shell
            .handle_session_list_key(key_char('j'), &config, &mut backend)
            .await
            .expect("session navigation should succeed");
    }
    finish_session_hydration(&mut shell, &backend).await;

    assert_eq!(shell.session_list.selected_thread_id(), Some(expected_id));
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: None,
            },
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: Some("20".to_string()),
            },
        ]
    );
    insta::assert_snapshot!(
        "session_list_second_page",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
            )
        )
    );
}

async fn refresh_session_list<S>(shell: &mut ShellState, app_server: &S)
where
    S: backend::AppShellBackend,
{
    shell.start_session_list_refresh(app_server);
    finish_session_hydration(shell, app_server).await;
}

async fn finish_session_hydration<S>(shell: &mut ShellState, app_server: &S)
where
    S: backend::AppShellBackend,
{
    tokio::time::timeout(Duration::from_secs(/*secs*/ 1), async {
        loop {
            let _changed = shell.poll_session_hydration(app_server).await;
            if !shell.has_pending_session_hydration() {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("session hydration should finish");
}

#[tokio::test]
async fn native_session_list_resume_and_fork_switch_shell_thread() {
    let config = test_config().await;
    let resume_id = test_thread_id("01900000-0000-7000-8000-000000000401");
    let fork_id = test_thread_id("01900000-0000-7000-8000-000000000402");
    let initial_id = test_thread_id(SNAPSHOT_THREAD_ID);
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.session_list.focused = true;
    let runner = Arc::new(RecordingWorkspaceRunner::new(
        crate::workspace_command::WorkspaceCommandOutput {
            exit_code: 0,
            stdout: "## hydrated\n M src/lib.rs\n".to_string(),
            stderr: String::new(),
        },
    ));
    shell.workspace_command_runner = Some(runner.clone());
    let mut backend = RecordingBackend::with_threads(vec![
        thread_fixture(resume_id, Some("resume target"), "resume preview"),
        thread_fixture(fork_id, Some("fork target"), "fork preview"),
    ]);
    let resume_goal = test_thread_goal(&resume_id, ThreadGoalStatus::Active, "Resume goal");
    *backend.active_goal.lock().expect("goal should lock") = Some(resume_goal.clone());

    refresh_session_list(&mut shell, &backend).await;
    shell
        .handle_session_list_key(key_char('r'), &config, &mut backend)
        .await
        .expect("resume should resolve");
    complete_backend_actions(&mut shell, &backend).await;
    finish_session_hydration(&mut shell, &backend).await;
    assert_eq!(shell.thread_id, resume_id);
    assert_eq!(shell.active_goal, Some(resume_goal));
    assert_eq!(
        (shell.dashboard_route, shell.session_list.focused),
        (DashboardRoute::Sessions, true)
    );

    refresh_session_list(&mut shell, &backend).await;
    shell.session_list.move_selection_down();
    let forked_id = test_thread_id("01900000-0000-7000-8000-000000000202");
    let fork_goal = test_thread_goal(&forked_id, ThreadGoalStatus::Paused, "Fork goal");
    *backend.active_goal.lock().expect("goal should lock") = Some(fork_goal.clone());
    shell
        .handle_session_list_key(key_char('f'), &config, &mut backend)
        .await
        .expect("fork should resolve");
    finish_session_hydration(&mut shell, &backend).await;
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: None,
            },
            RecordedBackendCall::Resume(resume_id),
            RecordedBackendCall::Unsubscribe(initial_id),
            RecordedBackendCall::GoalGet {
                thread_id: resume_id,
            },
            RecordedBackendCall::RateLimits,
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: None,
            },
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: None,
            },
            RecordedBackendCall::Fork(fork_id),
            RecordedBackendCall::Unsubscribe(resume_id),
            RecordedBackendCall::GoalGet {
                thread_id: forked_id,
            },
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: None,
            },
        ]
    );
    assert_eq!(
        shell.thread_name,
        Some("forked".to_string()),
        "fork should replace the active shell session"
    );
    assert_eq!(shell.active_goal, Some(fork_goal));
    assert_eq!(
        (shell.dashboard_route, shell.session_list.focused),
        (DashboardRoute::Sessions, true)
    );
    assert_eq!(
        shell.workspace_git_status,
        Some(WorkspaceGitStatus {
            branch: Some("hydrated".to_string()),
            changes: workspace::WorkspaceChangeSummary {
                modified: 1,
                ..workspace::WorkspaceChangeSummary::default()
            },
        })
    );
    assert_eq!(
        runner
            .commands()
            .into_iter()
            .map(|command| (command.argv, command.cwd))
            .collect::<Vec<_>>(),
        vec![
            (
                vec![
                    "git".to_string(),
                    "status".to_string(),
                    "--porcelain=v1".to_string(),
                    "--branch".to_string(),
                ],
                Some(PathBuf::from("/workspace/better-codex")),
            ),
            (
                vec![
                    "git".to_string(),
                    "status".to_string(),
                    "--porcelain=v1".to_string(),
                    "--branch".to_string(),
                ],
                Some(PathBuf::from("/workspace/better-codex")),
            ),
        ]
    );
}

#[tokio::test]
async fn session_switch_hydration_is_nonblocking_and_preserves_newer_state() {
    let config = test_config().await;
    let initial_id = test_thread_id(SNAPSHOT_THREAD_ID);
    let target_id = test_thread_id("01900000-0000-7000-8000-000000000405");
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.session_list.focused = true;
    let (runner, gate) =
        RecordingWorkspaceRunner::blocked(crate::workspace_command::WorkspaceCommandOutput {
            exit_code: 0,
            stdout: "## stale\n M stale.rs\n".to_string(),
            stderr: String::new(),
        });
    shell.workspace_command_runner = Some(Arc::new(runner));
    let mut backend = RecordingBackend::with_threads(vec![thread_fixture(
        target_id,
        Some("switch target"),
        "switch preview",
    )]);
    *backend.active_goal.lock().expect("goal should lock") = Some(test_thread_goal(
        &target_id,
        ThreadGoalStatus::Active,
        "Stale goal",
    ));

    refresh_session_list(&mut shell, &backend).await;
    tokio::time::timeout(
        Duration::from_secs(/*secs*/ 1),
        shell.handle_session_list_key(key_char('r'), &config, &mut backend),
    )
    .await
    .expect("session switch should not wait for workspace hydration")
    .expect("session switch should resolve");

    assert!(shell.has_pending_backend_actions());
    complete_backend_actions(&mut shell, &backend).await;
    assert!(shell.has_pending_session_hydration());
    assert!(
        backend
            .calls()
            .contains(&RecordedBackendCall::Unsubscribe(initial_id))
    );
    shell
        .set_goal_objective(&mut backend, "Newer goal".to_string())
        .await;
    shell.active_turn_id = Some("new-turn".to_string());
    shell.mark_workspace_status_refresh_due();

    gate.add_permits(/*n*/ 1);
    finish_session_hydration(&mut shell, &backend).await;

    assert_eq!(
        shell
            .active_goal
            .as_ref()
            .map(|goal| goal.objective.as_str()),
        Some("Newer goal")
    );
    assert_eq!(shell.workspace_git_status, None);
    assert!(shell.workspace_status_refresh_due);

    shell.active_turn_id = None;
    let fresh_runner =
        RecordingWorkspaceRunner::new(crate::workspace_command::WorkspaceCommandOutput {
            exit_code: 0,
            stdout: "## fresh\n?? fresh.rs\n".to_string(),
            stderr: String::new(),
        });
    shell.refresh_workspace_status(&fresh_runner).await;
    assert_eq!(
        shell.workspace_git_status,
        Some(WorkspaceGitStatus {
            branch: Some("fresh".to_string()),
            changes: workspace::WorkspaceChangeSummary {
                untracked: 1,
                ..workspace::WorkspaceChangeSummary::default()
            },
        })
    );
    assert!(!shell.workspace_status_refresh_due);
}

#[tokio::test]
async fn initial_hydration_applies_fast_lookups_while_workspace_is_still_loading() {
    let target_id = test_thread_id("01900000-0000-7000-8000-000000000408");
    let mut shell = ShellState::snapshot_fixture();
    let (runner, gate) =
        RecordingWorkspaceRunner::blocked(crate::workspace_command::WorkspaceCommandOutput {
            exit_code: 0,
            stdout: "## startup\n".to_string(),
            stderr: String::new(),
        });
    shell.workspace_command_runner = Some(Arc::new(runner));
    let backend = RecordingBackend::with_threads(vec![thread_fixture(
        target_id,
        Some("startup target"),
        "loaded without blocking input",
    )]);
    let goal = test_thread_goal(&shell.thread_id, ThreadGoalStatus::Active, "Startup goal");
    *backend.active_goal.lock().expect("goal should lock") = Some(goal.clone());

    shell.start_initial_dashboard_hydration(&backend);

    tokio::time::timeout(Duration::from_secs(/*secs*/ 1), async {
        loop {
            let _changed = shell.poll_session_hydration(&backend).await;
            if shell.session_list.selected_thread_id() == Some(target_id)
                && !shell.rate_limits.is_empty()
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fast startup lookups should finish independently");

    assert_eq!(shell.active_goal, None);
    assert!(
        !backend
            .calls()
            .iter()
            .any(|call| matches!(call, RecordedBackendCall::GoalGet { .. }))
    );

    shell.start_initial_goal_hydration(&backend);
    tokio::time::timeout(Duration::from_secs(/*secs*/ 1), async {
        loop {
            let _changed = shell.poll_session_hydration(&backend).await;
            if shell.active_goal.as_ref() == Some(&goal) {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("goal lookup should finish independently");

    assert!(shell.has_pending_session_hydration());
    assert_eq!(shell.active_goal, Some(goal.clone()));
    assert_eq!(
        shell
            .rate_limits
            .first()
            .and_then(|limit| limit.limit_id.as_deref()),
        Some("codex")
    );

    gate.add_permits(/*n*/ 1);
    finish_session_hydration(&mut shell, &backend).await;

    assert_eq!(shell.active_goal, Some(goal));
    assert_eq!(
        shell.workspace_git_status,
        Some(WorkspaceGitStatus {
            branch: Some("startup".to_string()),
            changes: workspace::WorkspaceChangeSummary::default(),
        })
    );
}

#[test]
fn newer_session_list_refresh_rejects_an_older_completion() {
    let mut shell = ShellState::snapshot_fixture();
    let stale_id = test_thread_id("01900000-0000-7000-8000-000000000409");
    let current_id = test_thread_id("01900000-0000-7000-8000-000000000410");
    let stale_revision = shell.begin_session_list_refresh();
    let current_revision = shell.begin_session_list_refresh();

    assert!(shell.finish_session_list_refresh(
        current_revision,
        session_hydration::SessionListLoad::Replace,
        Ok(ThreadListResponse {
            data: vec![thread_fixture(current_id, Some("current"), "newer result")],
            next_cursor: None,
            backwards_cursor: None,
        })
    ));
    assert!(!shell.finish_session_list_refresh(
        stale_revision,
        session_hydration::SessionListLoad::Replace,
        Ok(ThreadListResponse {
            data: vec![thread_fixture(stale_id, Some("stale"), "older result")],
            next_cursor: None,
            backwards_cursor: None,
        })
    ));

    assert_eq!(shell.session_list.selected_thread_id(), Some(current_id));
}

#[tokio::test]
async fn changed_session_list_query_supersedes_a_blocked_refresh() {
    let stale_id = test_thread_id("01900000-0000-7000-8000-000000000413");
    let current_id = test_thread_id("01900000-0000-7000-8000-000000000414");
    let gate = Arc::new(tokio::sync::Semaphore::new(/*permits*/ 0));
    let backend = RecordingBackend {
        threads: Arc::new(Mutex::new(vec![thread_fixture(
            stale_id,
            Some("stale"),
            "superseded result",
        )])),
        thread_list_gate: Some(gate.clone()),
        ..RecordingBackend::default()
    };
    let mut shell = ShellState::snapshot_fixture();

    shell.start_session_list_refresh(&backend);
    tokio::time::timeout(Duration::from_secs(/*secs*/ 1), async {
        while backend.calls().len() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the first list request should start");

    *backend.threads.lock().expect("threads should lock") = vec![thread_fixture(
        current_id,
        Some("current"),
        "replacement result",
    )];
    shell.session_list.toggle_archived();
    shell.start_session_list_refresh(&backend);
    tokio::time::timeout(Duration::from_secs(/*secs*/ 1), async {
        while backend.calls().len() != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the replacement list request should start");

    gate.add_permits(/*n*/ 1);
    finish_session_hydration(&mut shell, &backend).await;

    assert_eq!(shell.session_list.selected_thread_id(), Some(current_id));
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: None,
            },
            RecordedBackendCall::ThreadList {
                archived: Some(true),
                search_term: None,
                cursor: None,
            },
        ]
    );
}

#[tokio::test]
async fn completed_list_refresh_cannot_restore_a_deleted_session() {
    let config = test_config().await;
    let session_id = test_thread_id("01900000-0000-7000-8000-000000000415");
    let thread = thread_fixture(session_id, Some("delete me"), "stale server result");
    let backend_threads = vec![thread.clone()];
    let mut backend = RecordingBackend::with_threads(backend_threads);
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    shell.session_list.replace_threads(vec![thread]);

    shell.start_session_list_refresh(&backend);
    tokio::time::timeout(Duration::from_secs(/*secs*/ 1), async {
        while backend.calls().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the stale list request should complete");
    shell
        .handle_session_list_key(key_char('d'), &config, &mut backend)
        .await
        .expect("delete confirmation should open");
    complete_backend_actions(&mut shell, &backend).await;
    assert!(shell.pending_session_delete.is_some());
    shell
        .handle_session_delete_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("confirmed delete should succeed");
    complete_backend_actions(&mut shell, &backend).await;
    finish_session_hydration(&mut shell, &backend).await;

    assert_eq!(shell.session_list.selected_thread_id(), None);
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: None,
            },
            RecordedBackendCall::ThreadList {
                archived: Some(false),
                search_term: None,
                cursor: None,
            },
            RecordedBackendCall::ThreadList {
                archived: Some(true),
                search_term: None,
                cursor: None,
            },
            RecordedBackendCall::Delete(session_id),
        ]
    );
}

#[tokio::test]
async fn rate_limit_notification_during_startup_baseline_triggers_a_refetch() {
    let mut shell = ShellState::snapshot_fixture();
    let backend = RecordingBackend::default();
    shell.start_initial_dashboard_hydration(&backend);
    shell.handle_notification(ServerNotification::AccountRateLimitsUpdated(
        AccountRateLimitsUpdatedNotification {
            rate_limits: RateLimitSnapshot {
                limit_id: Some("codex".to_string()),
                limit_name: Some("Codex".to_string()),
                primary: Some(codex_app_server_protocol::RateLimitWindow {
                    used_percent: 73,
                    window_duration_mins: Some(300),
                    resets_at: None,
                }),
                secondary: None,
                credits: None,
                individual_limit: None,
                plan_type: None,
                rate_limit_reached_type: None,
            },
        },
    ));

    tokio::time::timeout(Duration::from_secs(/*secs*/ 1), async {
        loop {
            let _changed = shell.poll_session_hydration(&backend).await;
            let refreshes = backend
                .calls()
                .into_iter()
                .filter(|call| matches!(call, RecordedBackendCall::RateLimits))
                .count();
            if refreshes == 2 && !shell.has_pending_session_hydration() {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("stale startup baseline should be refetched");

    assert_eq!(
        shell
            .rate_limits
            .first()
            .and_then(|limit| limit.primary.as_ref())
            .map(|window| window.used_percent),
        Some(73)
    );
}

#[tokio::test]
async fn rate_limit_notification_after_baseline_fetches_canonical_state() {
    let mut shell = ShellState::snapshot_fixture();
    let backend = RecordingBackend::default();
    shell.start_initial_dashboard_hydration(&backend);
    finish_session_hydration(&mut shell, &backend).await;
    backend.set_rate_limits_used_percent(/*used_percent*/ 41);

    shell.handle_notification(ServerNotification::AccountRateLimitsUpdated(
        AccountRateLimitsUpdatedNotification {
            rate_limits: RateLimitSnapshot {
                limit_id: Some("codex".to_string()),
                limit_name: Some("Codex".to_string()),
                primary: Some(codex_app_server_protocol::RateLimitWindow {
                    used_percent: 92,
                    window_duration_mins: Some(300),
                    resets_at: None,
                }),
                secondary: None,
                credits: None,
                individual_limit: None,
                plan_type: None,
                rate_limit_reached_type: None,
            },
        },
    ));

    assert_eq!(
        shell.rate_limits[0]
            .primary
            .as_ref()
            .map(|window| window.used_percent),
        Some(92)
    );
    assert!(shell.has_pending_session_hydration());
    finish_session_hydration(&mut shell, &backend).await;
    assert_eq!(
        shell.rate_limits[0]
            .primary
            .as_ref()
            .map(|window| window.used_percent),
        Some(41)
    );
    assert_eq!(
        backend
            .calls()
            .into_iter()
            .filter(|call| matches!(call, RecordedBackendCall::RateLimits))
            .count(),
        2
    );
}

#[tokio::test]
async fn session_switch_restarts_an_in_flight_rate_limit_baseline() {
    let gate = Arc::new(tokio::sync::Semaphore::new(/*permits*/ 0));
    let backend = RecordingBackend {
        rate_limits_gate: Some(Arc::clone(&gate)),
        ..RecordingBackend::default()
    };
    let mut shell = ShellState::snapshot_fixture();
    shell.start_initial_dashboard_hydration(&backend);
    tokio::task::yield_now().await;

    shell.replace_started_session(started_thread(
        "replacement",
        test_thread_id("01900000-0000-7000-8000-000000000411"),
        /*forked_from_id*/ None,
    ));
    shell.start_replaced_session_hydration(&backend);

    assert_eq!(
        backend
            .calls()
            .into_iter()
            .filter(|call| matches!(call, RecordedBackendCall::RateLimits))
            .count(),
        2
    );
    assert!(shell.has_pending_session_hydration());

    gate.add_permits(/*n*/ 1);
    finish_session_hydration(&mut shell, &backend).await;
    assert_eq!(
        shell
            .rate_limits
            .first()
            .and_then(|limit| limit.primary.as_ref())
            .map(|window| window.used_percent),
        Some(73)
    );

    shell.replace_started_session(started_thread(
        "second replacement",
        test_thread_id("01900000-0000-7000-8000-000000000412"),
        /*forked_from_id*/ None,
    ));
    shell.start_replaced_session_hydration(&backend);
    finish_session_hydration(&mut shell, &backend).await;
    assert_eq!(
        backend
            .calls()
            .into_iter()
            .filter(|call| matches!(call, RecordedBackendCall::RateLimits))
            .count(),
        2,
        "a loaded account baseline should survive later session switches"
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn session_hydration_times_out_stalled_workspace_lookup() {
    let target_id = test_thread_id("01900000-0000-7000-8000-000000000406");
    let mut shell = ShellState::snapshot_fixture();
    let (runner, _gate) =
        RecordingWorkspaceRunner::blocked(crate::workspace_command::WorkspaceCommandOutput {
            exit_code: 0,
            stdout: "## never\n".to_string(),
            stderr: String::new(),
        });
    shell.workspace_command_runner = Some(Arc::new(runner));
    let backend = RecordingBackend::default();
    shell.replace_started_session(started_thread(
        "replacement",
        target_id,
        /*forked_from_id*/ None,
    ));
    shell.start_replaced_session_hydration(&backend);

    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(/*secs*/ 6)).await;
    tokio::task::yield_now().await;
    assert!(shell.poll_session_hydration(&backend).await);
    assert!(!shell.has_pending_session_hydration());
}

#[tokio::test]
async fn session_switch_waits_for_the_active_turn_to_finish() {
    let config = test_config().await;
    let target_id = test_thread_id("01900000-0000-7000-8000-000000000403");
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    shell.active_turn_id = Some("active-turn".to_string());
    let mut backend = RecordingBackend::with_threads(vec![thread_fixture(
        target_id,
        Some("switch target"),
        "switch preview",
    )]);
    refresh_session_list(&mut shell, &backend).await;

    shell
        .handle_session_list_key(key_char('r'), &config, &mut backend)
        .await
        .expect("blocked session switch should remain interactive");

    assert_eq!(shell.thread_id, test_thread_id(SNAPSHOT_THREAD_ID));
    assert!(shell.transcript.iter().any(|line| {
        line.kind == TranscriptKind::Status
            && line.text == "finish or interrupt the active turn before switching sessions"
    }));
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::ThreadList {
            archived: Some(false),
            search_term: None,
            cursor: None,
        }]
    );
}

#[tokio::test]
async fn session_switch_waits_for_pending_agent_input() {
    let config = test_config().await;
    let target_id = test_thread_id("01900000-0000-7000-8000-000000000404");
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    shell.pending_user_input = PendingUserInput::from_request(&tool_user_input_request());
    let mut backend = RecordingBackend::with_threads(vec![thread_fixture(
        target_id,
        Some("switch target"),
        "switch preview",
    )]);
    refresh_session_list(&mut shell, &backend).await;

    shell
        .handle_session_list_key(key_char('r'), &config, &mut backend)
        .await
        .expect("blocked session switch should remain interactive");

    assert_eq!(shell.thread_id, test_thread_id(SNAPSHOT_THREAD_ID));
    assert!(shell.transcript.iter().any(|line| {
        line.kind == TranscriptKind::Status
            && line.text == "resolve the pending request before switching sessions"
    }));
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::ThreadList {
            archived: Some(false),
            search_term: None,
            cursor: None,
        }]
    );
}

#[tokio::test]
async fn session_switch_preserves_nonempty_composer_draft() {
    let config = test_config().await;
    let target_id = test_thread_id("01900000-0000-7000-8000-000000000407");
    let mut shell = ShellState::snapshot_fixture();
    let draft = shell.composer.text().to_string();
    shell.session_list.focused = true;
    let mut backend = RecordingBackend::with_threads(vec![thread_fixture(
        target_id,
        Some("switch target"),
        "switch preview",
    )]);
    refresh_session_list(&mut shell, &backend).await;

    shell
        .handle_session_list_key(key_char('r'), &config, &mut backend)
        .await
        .expect("draft-blocked session switch should remain interactive");

    assert_eq!(shell.thread_id, test_thread_id(SNAPSHOT_THREAD_ID));
    assert_eq!(shell.composer.text(), draft);
    assert!(shell.transcript.iter().any(|line| {
        line.kind == TranscriptKind::Status
            && line.text == "send or clear the message draft before switching sessions"
    }));
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::ThreadList {
            archived: Some(false),
            search_term: None,
            cursor: None,
        }]
    );
}

#[test]
fn replacing_session_hydrates_agent_history_without_child_chat_in_transcript() {
    let root_id = test_thread_id("01900000-0000-7000-8000-000000000411");
    let child_id = test_thread_id("01900000-0000-7000-8000-000000000412");
    let mut started = started_thread("resumed", root_id, /*forked_from_id*/ None);
    let mut child = thread_fixture(child_id, /*name*/ None, "child preview");
    child.session_id = root_id.to_string();
    child.parent_thread_id = Some(root_id.to_string());
    child.source = SessionSource::SubAgent(codex_protocol::protocol::SubAgentSource::ThreadSpawn {
        parent_thread_id: root_id,
        depth: 1,
        agent_path: Some(
            codex_protocol::AgentPath::try_from("/root/alpha").expect("agent path should be valid"),
        ),
        agent_nickname: None,
        agent_role: None,
    });
    child.thread_source = Some(codex_app_server_protocol::ThreadSource::Subagent);
    let mut turn = test_turn("child-turn", TurnStatus::Completed);
    turn.items.push(ThreadItem::AgentMessage {
        id: "child-message".to_string(),
        text: "private child result".to_string(),
        phase: None,
        memory_citation: None,
    });
    child.turns.push(turn);
    started.agent_threads.push(child);
    let mut root_turn = test_turn("root-turn", TurnStatus::Completed);
    root_turn.items.extend([
        ThreadItem::CollabAgentToolCall {
            id: "historical-wait".to_string(),
            tool: CollabAgentTool::Wait,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: root_id.to_string(),
            receiver_thread_ids: vec![child_id.to_string()],
            prompt: None,
            model: None,
            reasoning_effort: None,
            agents_states: HashMap::from([(
                child_id.to_string(),
                CollabAgentState {
                    status: CollabAgentStatus::Running,
                    message: Some("stale active snapshot".to_string()),
                },
            )]),
        },
        ThreadItem::SubAgentActivity {
            id: "historical-child-start".to_string(),
            kind: SubAgentActivityKind::Started,
            agent_thread_id: child_id.to_string(),
            agent_path: "/root/alpha".to_string(),
        },
    ]);
    started.turns.push(root_turn);
    let mut shell = ShellState::snapshot_fixture();

    shell.replace_started_session(started);

    let agent = shell
        .agent_activity
        .agent(&child_id.to_string())
        .expect("child agent should be restored");
    assert_eq!(agent.display_name(), "alpha");
    assert_eq!(
        agent.latest_message.as_deref(),
        Some("private child result")
    );
    assert_eq!(agent.status, agent_activity::AgentLifecycleStatus::Shutdown);
    assert_eq!(
        shell.agent_activity.counts(),
        agent_activity::AgentActivityCounts {
            total: 1,
            completed: 1,
            ..Default::default()
        }
    );
    assert!(
        shell
            .transcript
            .iter()
            .all(|line| !line.text.contains("private child result"))
    );
    assert!(
        shell
            .transcript
            .iter()
            .all(|line| !matches!(line.tool_status, Some(ToolBlockStatus::Running)))
    );
}

#[tokio::test]
async fn replacing_session_clears_session_bound_surfaces() {
    let next_id = test_thread_id("01900000-0000-7000-8000-000000000413");
    let mut shell = ShellState::snapshot_fixture();
    shell.open_command_palette();
    shell.exit_confirmation_pending = true;
    shell.active_turn_id = Some("old-turn".to_string());
    shell.active_goal = Some(test_thread_goal(
        &shell.thread_id,
        ThreadGoalStatus::Active,
        "Old goal",
    ));
    shell.agent_activity.ensure_thread("old-agent");
    shell
        .active_agent_thread_ids
        .insert("old-agent".to_string());
    shell.subagent_activity.push_back(ToolActivity {
        id: "old-subagent".to_string(),
        title: "old child work".to_string(),
        status: "running".to_string(),
    });
    shell.workspace_git_status = Some(WorkspaceGitStatus {
        branch: Some("old-branch".to_string()),
        changes: workspace::WorkspaceChangeSummary {
            modified: 1,
            ..workspace::WorkspaceChangeSummary::default()
        },
    });
    shell.workspace_status_refresh_due = true;

    let mcp_response = ListMcpServerStatusResponse {
        data: vec![mcp_status_fixture(
            "github",
            McpAuthStatus::OAuth,
            ["search"],
        )],
        next_cursor: None,
    };
    shell.mcp_inventory = McpInventorySummary::from_response(&mcp_response);
    shell.mcp_catalog = Some(mcp_response);
    shell.open_mcp_management();
    let plugin_response = plugin_list_response_fixture();
    shell.plugin_inventory = PluginInventorySummary::from_response(&plugin_response);
    shell.plugin_catalog = Some(plugin_response);
    shell.open_plugin_management();
    let mut backend = RecordingBackend::with_external_agent_items(external_agent_items());
    shell
        .start_external_agent_import_review(&mut backend)
        .await
        .expect("external agent review should open");

    shell.replace_started_session(started_thread(
        "replacement",
        next_id,
        /*forked_from_id*/ None,
    ));

    assert_eq!(
        (
            shell.composer.text(),
            shell.command_palette,
            shell.exit_confirmation_pending,
            shell.pending_external_agent_import,
            shell.pending_mcp_management,
            shell.pending_plugin_management,
        ),
        ("", None, false, None, None, None)
    );
    assert_eq!(
        (
            shell.active_turn_id,
            shell.active_goal,
            shell.tool_activity,
            shell.agent_activity,
            shell.active_agent_thread_ids,
            shell.subagent_activity,
        ),
        (
            None,
            None,
            VecDeque::new(),
            AgentActivityState::default(),
            HashSet::new(),
            VecDeque::new(),
        )
    );
    assert_eq!(
        (
            shell.latest_diff,
            shell.workspace_git_status,
            shell.workspace_status_refresh_due,
            shell.token_usage,
            shell.context_token_usage,
            shell.model_context_window,
        ),
        (
            None,
            None,
            false,
            TokenUsage::default(),
            TokenUsage::default(),
            None,
        )
    );
    assert_eq!(
        (
            shell.mcp_inventory,
            shell.mcp_catalog,
            shell.plugin_inventory,
            shell.plugin_catalog,
        ),
        (
            McpInventorySummary::default(),
            None,
            PluginInventorySummary::default(),
            None,
        )
    );
    assert_eq!(shell.plan_explanation, None);
    assert_eq!(shell.plan_steps, Vec::new());
    assert_eq!(shell.status, "ready");
}

#[tokio::test]
async fn native_settings_integrations_refresh_mcp_and_plugins() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
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
        rendered.contains("MCP servers: 2 servers / 3 tools")
            && rendered.contains("Plugins: 1 installed / 2 available"),
        "settings should render native integration summaries:\n{rendered}"
    );
}

#[tokio::test]
async fn mcp_management_catalog_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
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
    shell.dashboard_route = DashboardRoute::Status;
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
    shell.dashboard_route = DashboardRoute::Status;
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
    shell.dashboard_route = DashboardRoute::Status;
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
    shell.dashboard_route = DashboardRoute::Status;
    shell.settings.focused = true;
    shell.available_models = vec![model_preset_fixture(
        "gpt-5-codex",
        /*show_in_picker*/ true,
        ReasoningEffort::Low,
        &[
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::XHigh,
            ReasoningEffort::Max,
            ReasoningEffort::Ultra,
        ],
        &[],
    )];
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
        .expect("reasoning selector should open");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("low reasoning should be selected");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("low reasoning should persist");
    complete_backend_actions(&mut shell, &backend).await;
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
        .expect("approval selector should open");
    for code in [KeyCode::Down, KeyCode::Down, KeyCode::Enter] {
        shell
            .handle_selector_key(KeyEvent::new(code, KeyModifiers::NONE), &mut backend)
            .await
            .expect("never approval policy should persist");
    }
    complete_backend_actions(&mut shell, &backend).await;
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
    complete_backend_actions(&mut shell, &backend).await;

    let calls = backend.calls();
    assert!(calls.contains(&RecordedBackendCall::ConfigWrite(vec![
        ("model".to_string(), serde_json::json!("gpt-5-codex"),),
        (
            "model_reasoning_effort".to_string(),
            serde_json::json!("low"),
        ),
    ])));
    assert!(calls.contains(&RecordedBackendCall::ThreadSettingsUpdate {
        model: None,
        effort: Some(ReasoningEffort::Low),
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
    assert_eq!(shell.reasoning_effort, Some(ReasoningEffort::Low));
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
async fn ultra_reasoning_warns_about_configured_agent_concurrency() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.settings.focused = true;
    shell.settings.focus_action(SettingsAction::ReasoningEffort);
    shell.max_concurrent_threads_per_session = 8;
    shell.available_models = vec![model_preset_fixture(
        "gpt-5-codex",
        /*show_in_picker*/ true,
        ReasoningEffort::Ultra,
        &[ReasoningEffort::Ultra],
        &[],
    )];
    let mut backend = RecordingBackend::default();

    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("reasoning selector should open");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("ultra reasoning should be focused");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("ultra reasoning should be selected");
    complete_backend_actions(&mut shell, &backend).await;

    assert_eq!(shell.reasoning_effort, Some(ReasoningEffort::Ultra));
    let warning = shell.transcript.back().expect("warning should be rendered");
    assert_eq!(warning.kind, TranscriptKind::Status);
    insta::assert_snapshot!(warning.text, @"Ultra reasoning may proactively use multiple agents. This session is configured for 8 concurrent threads with up to 7 subagents which can increase usage quickly. Consider setting features.multi_agent_v2.max_concurrent_threads_per_session below 8.");
}

#[tokio::test]
async fn native_settings_select_models_and_service_tiers() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
    shell.settings.focused = true;
    shell.available_models = vec![
        model_preset_fixture(
            "gpt-5-codex",
            /*show_in_picker*/ true,
            ReasoningEffort::Medium,
            &[ReasoningEffort::Low, ReasoningEffort::Medium],
            &[],
        ),
        model_preset_fixture(
            "hidden-model",
            /*show_in_picker*/ false,
            ReasoningEffort::Medium,
            &[ReasoningEffort::Low, ReasoningEffort::Medium],
            &[],
        ),
        model_preset_fixture(
            "gpt-5.5",
            /*show_in_picker*/ true,
            ReasoningEffort::Medium,
            &[ReasoningEffort::Low, ReasoningEffort::Medium],
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
        .expect("model selector should open");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("visible model should be focused");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("visible model should be selected");
    complete_backend_actions(&mut shell, &backend).await;
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
        .expect("service tier selector should open");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("fast tier should be focused");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("fast tier should be selected");
    complete_backend_actions(&mut shell, &backend).await;
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("service tier selector should reopen");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("batch tier should be focused");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("batch tier should be selected");
    complete_backend_actions(&mut shell, &backend).await;
    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("service tier selector should reopen");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("default tier should be focused");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("default tier should be selected");
    complete_backend_actions(&mut shell, &backend).await;

    assert_eq!(shell.model, "gpt-5.5");
    assert_eq!(shell.reasoning_effort, Some(ReasoningEffort::Medium));
    assert_eq!(
        shell.service_tier.as_deref(),
        Some(SERVICE_TIER_DEFAULT_REQUEST_VALUE)
    );
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ConfigWrite(vec![
                ("model".to_string(), json!("gpt-5.5")),
                ("model_reasoning_effort".to_string(), json!("medium")),
            ]),
            RecordedBackendCall::ThreadSettingsUpdate {
                model: Some("gpt-5.5".to_string()),
                effort: Some(ReasoningEffort::Medium),
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
async fn native_settings_reasoning_selector_resets_active_thread_to_model_default() {
    let mut shell = ShellState::snapshot_fixture();
    shell.settings.focused = true;
    shell.settings.focus_action(SettingsAction::ReasoningEffort);
    shell.model = "gpt-5.6-sol".to_string();
    shell.reasoning_effort = Some(ReasoningEffort::Ultra);
    shell.collaboration_mode = Some(collaboration_mode_fixture(
        "gpt-5.6-sol",
        Some(ReasoningEffort::Ultra),
    ));
    shell.available_models = vec![model_preset_fixture(
        "gpt-5.6-sol",
        /*show_in_picker*/ true,
        ReasoningEffort::Low,
        &[
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::XHigh,
            ReasoningEffort::Max,
            ReasoningEffort::Ultra,
        ],
        &["priority"],
    )];
    let mut backend = RecordingBackend::default();

    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("reasoning selector should open");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("default reasoning should be focused");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("default reasoning should be selected");
    complete_backend_actions(&mut shell, &backend).await;

    assert_eq!(shell.reasoning_effort, None);
    assert_eq!(
        shell.collaboration_mode,
        Some(collaboration_mode_fixture(
            "gpt-5.6-sol",
            /*reasoning_effort*/ None,
        ))
    );
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ConfigWrite(vec![
                ("model".to_string(), json!("gpt-5.6-sol")),
                (
                    "model_reasoning_effort".to_string(),
                    serde_json::Value::Null
                ),
            ]),
            RecordedBackendCall::ThreadSettingsUpdate {
                model: None,
                effort: Some(ReasoningEffort::Low),
                service_tier: None,
                approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            },
        ]
    );
}

#[tokio::test]
async fn native_settings_model_switch_resets_unsupported_runtime_options() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Status;
    shell.settings.focused = true;
    shell.model = "gpt-5.6-sol".to_string();
    shell.reasoning_effort = Some(ReasoningEffort::Ultra);
    shell.service_tier = Some("priority".to_string());
    shell.collaboration_mode = Some(collaboration_mode_fixture(
        "gpt-5.6-sol",
        Some(ReasoningEffort::Ultra),
    ));
    shell.available_models = vec![
        model_preset_fixture(
            "gpt-5.6-sol",
            /*show_in_picker*/ true,
            ReasoningEffort::Low,
            &[
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::XHigh,
                ReasoningEffort::Max,
                ReasoningEffort::Ultra,
            ],
            &["priority"],
        ),
        model_preset_fixture(
            "gpt-5.6-luna",
            /*show_in_picker*/ true,
            ReasoningEffort::Medium,
            &[
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::XHigh,
                ReasoningEffort::Max,
            ],
            &[],
        ),
    ];
    let mut backend = RecordingBackend::default();

    shell
        .handle_settings_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("model selector should open");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("luna model should be focused");
    shell
        .handle_selector_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut backend,
        )
        .await
        .expect("luna model should be selected");
    complete_backend_actions(&mut shell, &backend).await;
    shell.submit_prompt(&backend, "Use current settings".to_string());
    complete_backend_actions(&mut shell, &backend).await;

    assert_eq!(shell.model, "gpt-5.6-luna");
    assert_eq!(shell.reasoning_effort, Some(ReasoningEffort::Medium));
    assert_eq!(shell.service_tier, None);
    assert_eq!(
        shell.collaboration_mode,
        Some(collaboration_mode_fixture(
            "gpt-5.6-luna",
            Some(ReasoningEffort::Medium),
        ))
    );
    assert_eq!(
        backend.calls(),
        vec![
            RecordedBackendCall::ConfigWrite(vec![
                ("model".to_string(), json!("gpt-5.6-luna")),
                ("model_reasoning_effort".to_string(), json!("medium")),
                ("service_tier".to_string(), serde_json::Value::Null),
            ]),
            RecordedBackendCall::ThreadSettingsUpdate {
                model: Some("gpt-5.6-luna".to_string()),
                effort: Some(ReasoningEffort::Medium),
                service_tier: Some(None),
                approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            },
            RecordedBackendCall::TurnStart {
                thread_id: shell.thread_id,
                prompt: "Use current settings".to_string(),
                cwd: PathBuf::from("/workspace/better-codex"),
                model: "gpt-5.6-luna".to_string(),
                effort: Some(ReasoningEffort::Medium),
                collaboration_mode: Some(CollaborationMode {
                    mode: ModeKind::Default,
                    settings: CollaborationModeSettings {
                        model: "gpt-5.6-luna".to_string(),
                        reasoning_effort: Some(ReasoningEffort::Medium),
                        developer_instructions: None,
                    },
                }),
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

    shell.submit_prompt(&backend, "hello app shell".to_string());
    complete_backend_actions(&mut shell, &backend).await;
    shell
        .handle_app_server_event(
            &mut backend,
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
            AppServerEvent::ServerRequest(command_approval_request()),
        )
        .await
        .expect("approval request should be handled");
    shell
        .resolve_pending_approval(&backend, /*option_index*/ 0, None)
        .expect("approval should resolve");
    complete_backend_actions(&mut shell, &backend).await;

    shell.active_turn_id = Some("turn-interrupt".to_string());
    shell
        .interrupt_active_turn(&mut backend)
        .await
        .expect("interrupt should resolve");
    shell
        .handle_app_server_event(
            &mut backend,
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
    resolved_requests: Arc<Mutex<Vec<(RequestId, serde_json::Value)>>>,
    start_configs: Arc<Mutex<Vec<Config>>>,
    threads: Arc<Mutex<Vec<Thread>>>,
    mcp_statuses: Arc<Mutex<Vec<McpServerStatus>>>,
    plugin_response: Arc<Mutex<Option<PluginListResponse>>>,
    plugin_install_response: Arc<Mutex<PluginInstallResponse>>,
    external_agent_items: Arc<Mutex<Vec<ExternalAgentConfigMigrationItem>>>,
    external_agent_import_in_progress: Arc<Mutex<bool>>,
    active_goal: Arc<Mutex<Option<ThreadGoal>>>,
    thread_list_gate: Option<Arc<tokio::sync::Semaphore>>,
    rate_limits_gate: Option<Arc<tokio::sync::Semaphore>>,
    rate_limits_used_percent: Arc<Mutex<i32>>,
    turn_start_error: Arc<Mutex<Option<String>>>,
    turn_start_gate: Option<Arc<tokio::sync::Semaphore>>,
    action_error: Arc<Mutex<Option<String>>>,
    thread_settings_error: Arc<Mutex<Option<String>>>,
    config_values: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    remote_workspace: bool,
    embedded_app_server: bool,
}

impl Default for RecordingBackend {
    fn default() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            resolved_requests: Arc::new(Mutex::new(Vec::new())),
            start_configs: Arc::new(Mutex::new(Vec::new())),
            threads: Arc::new(Mutex::new(Vec::new())),
            mcp_statuses: Arc::new(Mutex::new(Vec::new())),
            plugin_response: Arc::new(Mutex::new(None)),
            plugin_install_response: Arc::new(Mutex::new(PluginInstallResponse {
                auth_policy: PluginAuthPolicy::OnUse,
                apps_needing_auth: Vec::new(),
            })),
            external_agent_items: Arc::new(Mutex::new(Vec::new())),
            external_agent_import_in_progress: Arc::new(Mutex::new(false)),
            active_goal: Arc::new(Mutex::new(None)),
            thread_list_gate: None,
            rate_limits_gate: None,
            rate_limits_used_percent: Arc::new(Mutex::new(73)),
            turn_start_error: Arc::new(Mutex::new(None)),
            turn_start_gate: None,
            action_error: Arc::new(Mutex::new(None)),
            thread_settings_error: Arc::new(Mutex::new(None)),
            config_values: Arc::new(Mutex::new(HashMap::new())),
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

    fn start_configs(&self) -> Vec<Config> {
        self.start_configs
            .lock()
            .expect("start configs should lock")
            .clone()
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

    fn set_rate_limits_used_percent(&self, used_percent: i32) {
        *self
            .rate_limits_used_percent
            .lock()
            .expect("rate-limit percentage should lock") = used_percent;
    }

    fn fail_next_turn_start(&self, message: &str) {
        *self
            .turn_start_error
            .lock()
            .expect("turn-start error should lock") = Some(message.to_string());
    }

    fn fail_next_action(&self, message: &str) {
        *self.action_error.lock().expect("action error should lock") = Some(message.to_string());
    }

    fn fail_next_thread_settings_update(&self, message: &str) {
        *self
            .thread_settings_error
            .lock()
            .expect("thread settings error should lock") = Some(message.to_string());
    }

    fn take_action_error(&self) -> Option<String> {
        self.action_error
            .lock()
            .expect("action error should lock")
            .take()
    }

    fn take_thread_settings_error(&self) -> Option<String> {
        self.thread_settings_error
            .lock()
            .expect("thread settings error should lock")
            .take()
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
        cursor: Option<String>,
    },
    ThreadReadFull(codex_protocol::ThreadId),
    RateLimits,
    Archive(codex_protocol::ThreadId),
    Unarchive(codex_protocol::ThreadId),
    Delete(codex_protocol::ThreadId),
    SetName {
        thread_id: codex_protocol::ThreadId,
        name: String,
    },
    GoalGet {
        thread_id: codex_protocol::ThreadId,
    },
    GoalSet {
        thread_id: codex_protocol::ThreadId,
        objective: Option<String>,
        status: Option<ThreadGoalStatus>,
        token_budget: Option<Option<i64>>,
    },
    GoalClear {
        thread_id: codex_protocol::ThreadId,
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
        effort: Option<ReasoningEffort>,
        collaboration_mode: Option<CollaborationMode>,
    },
    Interrupt {
        thread_id: codex_protocol::ThreadId,
        turn_id: String,
    },
    Rollback {
        thread_id: codex_protocol::ThreadId,
        num_turns: u32,
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
        config: &Config,
        session_start_source: Option<ThreadStartSource>,
    ) -> color_eyre::Result<crate::app_server_session::AppServerStartedThread> {
        self.push(RecordedBackendCall::Start(session_start_source));
        self.start_configs
            .lock()
            .expect("start configs should lock")
            .push(config.clone());
        Ok(started_thread(
            "started",
            test_thread_id("01900000-0000-7000-8000-000000000201"),
            /*forked_from_id*/ None,
        ))
    }

    async fn resume_thread(
        &mut self,
        _config: Config,
        thread_id: codex_protocol::ThreadId,
    ) -> color_eyre::Result<crate::app_server_session::AppServerStartedThread> {
        self.push(RecordedBackendCall::Resume(thread_id));
        Ok(started_thread(
            "resumed", thread_id, /*forked_from_id*/ None,
        ))
    }

    fn resume_thread_in_background(
        &self,
        config: Config,
        thread_id: codex_protocol::ThreadId,
    ) -> impl std::future::Future<
        Output = color_eyre::Result<crate::app_server_session::AppServerStartedThread>,
    > + Send
    + 'static {
        let mut backend = self.clone();
        async move { backend.resume_thread(config, thread_id).await }
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
        let cursor = params.cursor.clone();
        self.push(RecordedBackendCall::ThreadList {
            archived: params.archived,
            search_term: params.search_term.clone(),
            cursor: cursor.clone(),
        });
        let limit = params.limit.map_or(usize::MAX, |limit| {
            usize::try_from(limit).unwrap_or(usize::MAX)
        });
        let offset = cursor
            .as_deref()
            .and_then(|cursor| cursor.parse::<usize>().ok())
            .unwrap_or_default();
        let search_term = params.search_term.unwrap_or_default();
        let mut data = {
            let threads = self.threads.lock().expect("threads should lock");
            threads
                .iter()
                .filter(|thread| {
                    let matches_search = search_term.is_empty()
                        || thread
                            .name
                            .as_deref()
                            .is_some_and(|name| name.contains(&search_term))
                        || thread.preview.contains(&search_term);
                    let matches_ancestor =
                        params.ancestor_thread_id.as_ref().is_none_or(|ancestor| {
                            if params.archived == Some(true) {
                                return false;
                            }
                            let mut parent = thread.parent_thread_id.as_deref();
                            for _ in 0..threads.len() {
                                let Some(parent_id) = parent else {
                                    return false;
                                };
                                if parent_id == ancestor {
                                    return true;
                                }
                                parent = threads
                                    .iter()
                                    .find(|candidate| candidate.id == parent_id)
                                    .and_then(|candidate| candidate.parent_thread_id.as_deref());
                            }
                            false
                        });
                    matches_search && matches_ancestor
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        let total = data.len();
        data = data.into_iter().skip(offset).take(limit).collect();
        let next_offset = offset.saturating_add(data.len());
        let next_cursor = (next_offset < total).then(|| next_offset.to_string());
        let response = ThreadListResponse {
            data,
            next_cursor,
            backwards_cursor: None,
        };
        if let Some(gate) = self.thread_list_gate.clone() {
            gate.acquire_owned()
                .await
                .expect("session-list gate should remain open")
                .forget();
        }
        Ok(response)
    }

    fn thread_list_in_background(
        &self,
        params: ThreadListParams,
    ) -> impl std::future::Future<Output = color_eyre::Result<ThreadListResponse>> + Send + 'static
    {
        let mut backend = self.clone();
        async move { backend.thread_list(params).await }
    }

    fn thread_read_full_in_background(
        &self,
        thread_id: codex_protocol::ThreadId,
    ) -> impl std::future::Future<Output = color_eyre::Result<Thread>> + Send + 'static {
        self.push(RecordedBackendCall::ThreadReadFull(thread_id));
        let thread = self
            .threads
            .lock()
            .expect("threads should lock")
            .iter()
            .find(|thread| thread.id == thread_id.to_string())
            .cloned();
        async move { thread.ok_or_else(|| color_eyre::eyre::eyre!("thread {thread_id} was not found")) }
    }

    fn account_rate_limits_in_background(
        &self,
    ) -> impl std::future::Future<Output = color_eyre::Result<GetAccountRateLimitsResponse>>
    + Send
    + 'static {
        self.push(RecordedBackendCall::RateLimits);
        let gate = self.rate_limits_gate.clone();
        let used_percent = *self
            .rate_limits_used_percent
            .lock()
            .expect("rate-limit percentage should lock");
        async move {
            if let Some(gate) = gate {
                gate.acquire_owned()
                    .await
                    .expect("rate-limit gate should remain open")
                    .forget();
            }
            Ok(GetAccountRateLimitsResponse {
                rate_limits: RateLimitSnapshot {
                    limit_id: Some("codex".to_string()),
                    limit_name: Some("Codex".to_string()),
                    primary: Some(codex_app_server_protocol::RateLimitWindow {
                        used_percent,
                        window_duration_mins: Some(300),
                        resets_at: None,
                    }),
                    secondary: None,
                    credits: None,
                    individual_limit: None,
                    plan_type: None,
                    rate_limit_reached_type: None,
                },
                rate_limits_by_limit_id: None,
                rate_limit_reset_credits: None,
            })
        }
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
        if let Some(error) = self.take_action_error() {
            return Err(color_eyre::eyre::eyre!(error));
        }
        Ok(())
    }

    fn thread_delete_in_background(
        &self,
        thread_id: codex_protocol::ThreadId,
    ) -> impl std::future::Future<Output = color_eyre::Result<()>> + Send + 'static {
        let mut backend = self.clone();
        async move { backend.thread_delete(thread_id).await }
    }

    fn thread_descendant_count_in_background(
        &self,
        thread_id: codex_protocol::ThreadId,
    ) -> impl std::future::Future<Output = color_eyre::Result<usize>> + Send + 'static {
        let mut backend = self.clone();
        async move {
            let mut count = 0;
            for archived in [false, true] {
                let response = backend
                    .thread_list(ThreadListParams {
                        cursor: None,
                        limit: None,
                        sort_key: None,
                        sort_direction: None,
                        model_providers: None,
                        source_kinds: None,
                        archived: Some(archived),
                        cwd: None,
                        use_state_db_only: true,
                        search_term: None,
                        parent_thread_id: None,
                        ancestor_thread_id: Some(thread_id.to_string()),
                    })
                    .await?;
                count += response.data.len();
            }
            Ok(count)
        }
    }

    async fn thread_set_name(
        &mut self,
        thread_id: codex_protocol::ThreadId,
        name: String,
    ) -> color_eyre::Result<()> {
        self.push(RecordedBackendCall::SetName { thread_id, name });
        if let Some(error) = self.take_action_error() {
            return Err(color_eyre::eyre::eyre!(error));
        }
        Ok(())
    }

    fn thread_set_name_in_background(
        &self,
        thread_id: codex_protocol::ThreadId,
        name: String,
    ) -> impl std::future::Future<Output = color_eyre::Result<()>> + Send + 'static {
        let mut backend = self.clone();
        async move { backend.thread_set_name(thread_id, name).await }
    }

    async fn thread_goal_get(
        &mut self,
        thread_id: codex_protocol::ThreadId,
    ) -> color_eyre::Result<ThreadGoalGetResponse> {
        self.push(RecordedBackendCall::GoalGet { thread_id });
        Ok(ThreadGoalGetResponse {
            goal: self.active_goal.lock().expect("goal should lock").clone(),
        })
    }

    fn thread_goal_get_in_background(
        &self,
        thread_id: codex_protocol::ThreadId,
    ) -> impl std::future::Future<Output = color_eyre::Result<ThreadGoalGetResponse>> + Send + 'static
    {
        self.push(RecordedBackendCall::GoalGet { thread_id });
        let goal = self.active_goal.lock().expect("goal should lock").clone();
        async move { Ok(ThreadGoalGetResponse { goal }) }
    }

    async fn thread_goal_set(
        &mut self,
        thread_id: codex_protocol::ThreadId,
        objective: Option<String>,
        status: Option<ThreadGoalStatus>,
        token_budget: Option<Option<i64>>,
    ) -> color_eyre::Result<ThreadGoalSetResponse> {
        self.push(RecordedBackendCall::GoalSet {
            thread_id,
            objective: objective.clone(),
            status,
            token_budget,
        });

        let mut goal = self.active_goal.lock().expect("goal should lock");
        let existing = goal.clone();
        let Some(objective) =
            objective.or_else(|| existing.as_ref().map(|goal| goal.objective.clone()))
        else {
            return Err(color_eyre::eyre::eyre!("no goal is currently set"));
        };
        let status = status
            .or_else(|| existing.as_ref().map(|goal| goal.status))
            .unwrap_or(ThreadGoalStatus::Active);
        let mut updated = test_thread_goal(&thread_id, status, &objective);
        updated.token_budget =
            token_budget.unwrap_or_else(|| existing.as_ref().and_then(|goal| goal.token_budget));
        if let Some(existing) = existing {
            updated.tokens_used = existing.tokens_used;
            updated.time_used_seconds = existing.time_used_seconds;
            updated.created_at = existing.created_at;
        }
        *goal = Some(updated.clone());
        Ok(ThreadGoalSetResponse { goal: updated })
    }

    async fn thread_goal_clear(
        &mut self,
        thread_id: codex_protocol::ThreadId,
    ) -> color_eyre::Result<ThreadGoalClearResponse> {
        self.push(RecordedBackendCall::GoalClear { thread_id });
        let cleared = self
            .active_goal
            .lock()
            .expect("goal should lock")
            .take()
            .is_some();
        Ok(ThreadGoalClearResponse { cleared })
    }

    async fn write_config(
        &mut self,
        edits: Vec<ConfigEdit>,
    ) -> color_eyre::Result<ConfigWriteResponse> {
        self.push(RecordedBackendCall::ConfigWrite(
            edits
                .iter()
                .map(|edit| (edit.key_path.clone(), edit.value.clone()))
                .collect(),
        ));
        if let Some(error) = self.take_action_error() {
            return Err(color_eyre::eyre::eyre!(error));
        }
        let mut values = self
            .config_values
            .lock()
            .expect("config values should lock");
        for edit in edits {
            if edit.value.is_null() {
                values.remove(&edit.key_path);
            } else {
                values.insert(edit.key_path, edit.value);
            }
        }
        Ok(ConfigWriteResponse {
            status: WriteStatus::Ok,
            version: "1".to_string(),
            file_path: test_absolute_path("codex-home/config.toml"),
            overridden_metadata: None,
        })
    }

    fn persist_settings_update_in_background(
        &self,
        edits: Vec<ConfigEdit>,
        thread_update: Option<ThreadSettingsUpdateParams>,
    ) -> impl std::future::Future<Output = color_eyre::Result<()>> + Send + 'static {
        let mut backend = self.clone();
        async move {
            let rollback_edits = {
                let values = backend
                    .config_values
                    .lock()
                    .expect("config values should lock");
                edits
                    .iter()
                    .map(|edit| ConfigEdit {
                        key_path: edit.key_path.clone(),
                        value: values
                            .get(&edit.key_path)
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
                        merge_strategy: MergeStrategy::Replace,
                    })
                    .collect()
            };
            backend.write_config(edits).await?;
            let Some(params) = thread_update else {
                return Ok(());
            };
            backend.push(RecordedBackendCall::ThreadSettingsUpdate {
                model: params.model,
                effort: params.effort,
                service_tier: params.service_tier,
                approval_policy: params
                    .approval_policy
                    .unwrap_or(codex_app_server_protocol::AskForApproval::OnRequest),
            });
            let Some(error) = backend.take_thread_settings_error() else {
                return Ok(());
            };
            match backend.write_config(rollback_edits).await {
                Ok(_) => Err(color_eyre::eyre::eyre!(error)).wrap_err(
                    "thread settings update failed; global config changes were rolled back",
                ),
                Err(rollback_error) => Err(color_eyre::eyre::eyre!(
                    "thread settings update failed: {error}; global config rollback also failed: {rollback_error:#}"
                )),
            }
        }
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
            effort: params.effort,
            collaboration_mode: params.collaboration_mode,
        });
        if let Some(error) = self
            .turn_start_error
            .lock()
            .expect("turn-start error should lock")
            .take()
        {
            return Err(color_eyre::eyre::eyre!(error));
        }
        if let Some(gate) = self.turn_start_gate.clone() {
            gate.acquire_owned()
                .await
                .expect("turn-start gate should remain open")
                .forget();
        }
        Ok(TurnStartResponse {
            turn: test_turn("turn-submit", TurnStatus::InProgress),
        })
    }

    fn turn_start_in_background(
        &self,
        params: backend::AppShellTurnStart,
    ) -> impl std::future::Future<Output = color_eyre::Result<TurnStartResponse>> + Send + 'static
    {
        let mut backend = self.clone();
        async move { backend.turn_start(params).await }
    }

    async fn turn_interrupt(
        &mut self,
        thread_id: codex_protocol::ThreadId,
        turn_id: String,
    ) -> std::result::Result<(), TypedRequestError> {
        self.push(RecordedBackendCall::Interrupt { thread_id, turn_id });
        Ok(())
    }

    async fn thread_rollback(
        &mut self,
        thread_id: codex_protocol::ThreadId,
        num_turns: u32,
    ) -> color_eyre::Result<ThreadRollbackResponse> {
        self.push(RecordedBackendCall::Rollback {
            thread_id,
            num_turns,
        });
        Ok(ThreadRollbackResponse {
            thread: thread_fixture(thread_id, Some("rolled back"), "rolled back preview"),
        })
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
        result: serde_json::Value,
    ) -> std::io::Result<()> {
        self.resolved_requests
            .lock()
            .expect("resolved requests should lock")
            .push((request_id.clone(), result));
        self.push(RecordedBackendCall::Resolve(request_id));
        if let Some(error) = self.take_action_error() {
            return Err(std::io::Error::other(error));
        }
        Ok(())
    }

    fn resolve_server_request_in_background(
        &self,
        request_id: RequestId,
        result: serde_json::Value,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send + 'static {
        let backend = self.clone();
        async move { backend.resolve_server_request(request_id, result).await }
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

    async fn unsubscribe_threads(&self, thread_ids: Vec<codex_protocol::ThreadId>) {
        for thread_id in thread_ids {
            self.push(RecordedBackendCall::Unsubscribe(thread_id));
        }
    }

    fn unsubscribe_threads_in_background(
        &self,
        thread_ids: Vec<codex_protocol::ThreadId>,
    ) -> tokio::task::JoinHandle<()> {
        for thread_id in thread_ids {
            self.push(RecordedBackendCall::Unsubscribe(thread_id));
        }
        tokio::spawn(async {})
    }

    async fn shutdown(self) -> std::io::Result<()> {
        self.push(RecordedBackendCall::Shutdown);
        Ok(())
    }
}

struct NoopWorkspaceRunner;

struct RecordingWorkspaceRunner {
    commands: Mutex<Vec<crate::workspace_command::WorkspaceCommand>>,
    run_process_ids: Mutex<Vec<String>>,
    terminate_process_ids: Mutex<Vec<String>>,
    output: crate::workspace_command::WorkspaceCommandOutput,
    gate: Option<Arc<tokio::sync::Semaphore>>,
}

impl RecordingWorkspaceRunner {
    fn new(output: crate::workspace_command::WorkspaceCommandOutput) -> Self {
        Self {
            commands: Mutex::new(Vec::new()),
            run_process_ids: Mutex::new(Vec::new()),
            terminate_process_ids: Mutex::new(Vec::new()),
            output,
            gate: None,
        }
    }

    fn blocked(
        output: crate::workspace_command::WorkspaceCommandOutput,
    ) -> (Self, Arc<tokio::sync::Semaphore>) {
        let gate = Arc::new(tokio::sync::Semaphore::new(/*permits*/ 0));
        (
            Self {
                commands: Mutex::new(Vec::new()),
                run_process_ids: Mutex::new(Vec::new()),
                terminate_process_ids: Mutex::new(Vec::new()),
                output,
                gate: Some(Arc::clone(&gate)),
            },
            gate,
        )
    }

    fn commands(&self) -> Vec<crate::workspace_command::WorkspaceCommand> {
        self.commands
            .lock()
            .expect("workspace commands should lock")
            .clone()
    }

    fn run_process_ids(&self) -> Vec<String> {
        self.run_process_ids
            .lock()
            .expect("workspace run process ids should lock")
            .clone()
    }

    fn terminate_process_ids(&self) -> Vec<String> {
        self.terminate_process_ids
            .lock()
            .expect("workspace terminate process ids should lock")
            .clone()
    }
}

impl crate::workspace_command::WorkspaceCommandExecutor for RecordingWorkspaceRunner {
    fn run(
        &self,
        command: crate::workspace_command::WorkspaceCommand,
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
        self.commands
            .lock()
            .expect("workspace commands should lock")
            .push(command);
        let output = self.output.clone();
        let gate = self.gate.clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire_owned().await;
            }
            Ok(output)
        })
    }

    fn run_cancellable(
        &self,
        command: crate::workspace_command::WorkspaceCommand,
        process_id: String,
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
        self.run_process_ids
            .lock()
            .expect("workspace run process ids should lock")
            .push(process_id);
        self.run(command)
    }

    fn terminate(
        &self,
        process_id: String,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        crate::workspace_command::WorkspaceCommandTermination,
                        crate::workspace_command::WorkspaceCommandError,
                    >,
                > + Send
                + '_,
        >,
    > {
        self.terminate_process_ids
            .lock()
            .expect("workspace terminate process ids should lock")
            .push(process_id);
        let gate = self.gate.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            if let Some(gate) = gate {
                gate.add_permits(/*n*/ 1);
            }
            Ok(crate::workspace_command::WorkspaceCommandTermination::Requested)
        })
    }
}

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
        agent_threads: Vec::new(),
        agent_history_task: None,
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
        history_mode: Default::default(),
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

fn turn_completed_event(
    thread_id: codex_protocol::ThreadId,
    turn_id: &str,
    status: TurnStatus,
) -> AppServerEvent {
    AppServerEvent::ServerNotification(ServerNotification::TurnCompleted(
        codex_app_server_protocol::TurnCompletedNotification {
            thread_id: thread_id.to_string(),
            turn: test_turn(turn_id, status),
        },
    ))
}

fn queue_messages(composer: &mut ComposerState, messages: &[&str]) {
    for message in messages {
        composer.set_text(*message);
        assert!(composer.queue_current_message());
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
