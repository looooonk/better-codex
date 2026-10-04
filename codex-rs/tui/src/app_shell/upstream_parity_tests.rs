use super::*;
use pretty_assertions::assert_eq;

fn verification_request() -> ServerRequest {
    let mut request = mcp_url_elicitation_request();
    let ServerRequest::McpServerElicitationRequest { params, .. } = &mut request else {
        unreachable!()
    };
    params.request = McpServerElicitationRequest::UserVerification {
        meta: None,
        title: "Approve production access".to_string(),
        description: "Verify with your device to continue.".to_string(),
        challenge: "dGVzdA".to_string(),
    };
    request
}

#[test]
fn sol_catalog_supports_current_model_and_reasoning_controls() {
    let mut shell = ShellState::snapshot_fixture();
    shell.model = "gpt-6.1-sol".to_string();
    shell.reasoning_effort = Some(ReasoningEffort::Ultra);
    shell.available_models = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .map(Into::into)
        .collect();
    shell.open_model_selector();
    insta::assert_snapshot!(
        "sol_model_selector",
        render_shell(&shell, Rect::new(0, 0, 100, 30))
    );
    shell.open_reasoning_selector();
    insta::assert_snapshot!(
        "sol_reasoning_selector",
        render_shell(&shell, Rect::new(0, 0, 100, 30))
    );
}

#[test]
fn verification_requires_proof_and_displays_native_approval() {
    let mut shell = ShellState::snapshot_fixture();
    shell.pending_elicitation = PendingElicitation::from_request(&verification_request());
    assert_eq!(
        shell
            .pending_elicitation
            .as_ref()
            .unwrap()
            .result(ElicitationChoice::Accept),
        Err("device verification must finish before accepting".to_string())
    );
    insta::assert_snapshot!(
        "device_verification",
        render_shell(&shell, Rect::new(0, 0, 100, 28))
    );
}

#[tokio::test]
async fn cancelling_verification_discards_a_late_background_result() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.pending_elicitation = PendingElicitation::from_request(&verification_request());
    shell
        .resolve_pending_elicitation(&mut backend, ElicitationChoice::Accept)
        .await
        .unwrap();
    assert!(shell.has_pending_backend_action(ActionGroup::UserVerification));
    shell
        .resolve_pending_elicitation(&mut backend, ElicitationChoice::Cancel)
        .await
        .unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    assert!(shell.pending_elicitation.is_none());
    assert_eq!(
        *backend.resolved_requests.lock().unwrap(),
        vec![(
            RequestId::Integer(45),
            json!({ "action": "cancel", "content": null, "_meta": null })
        )]
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn nonblocking_input_uses_native_deadline_and_user_interaction_snoozes_it() {
    let mut request = tool_user_input_request();
    let ServerRequest::ToolRequestUserInput { params, .. } = &mut request else {
        unreachable!()
    };
    params.auto_resolution_ms = Some(1);
    let blocking = PendingUserInput::from_request(&request).unwrap();
    assert_eq!(blocking.auto_resolution_ms(), None);
    let ServerRequest::ToolRequestUserInput { params, .. } = &mut request else {
        unreachable!()
    };
    params.is_blocking = false;
    params.auto_resolution_ms = None;
    let mut pending = PendingUserInput::from_request(&request).unwrap();
    assert_eq!(pending.auto_resolution_ms(), Some(120_000));
    pending.snooze_auto_resolution();
    assert_eq!(pending.auto_resolution_ms(), None);
}

#[test]
fn native_function_outputs_are_visible_in_the_transcript() {
    let mut shell = ShellState::snapshot_fixture();
    shell.ingest_completed_item(
        ThreadItem::FunctionCallOutput {
            id: "native-function-output".to_string(),
            namespace: Some("functions".to_string()),
            name: "lookup".to_string(),
            output: codex_protocol::models::FunctionCallOutputBody::Text(
                "Found the requested record.".to_string(),
            ),
        },
        CompletedItemOrigin::Live,
    );
    insta::assert_snapshot!(
        "native_function_output",
        render_shell(&shell, Rect::new(0, 0, 100, 28))
    );
}

#[test]
fn hook_completion_updates_activity_with_its_output() {
    let mut shell = ShellState::snapshot_fixture();
    let mut run = codex_app_server_protocol::HookRunSummary {
        id: "post-tool-check".to_string(),
        event_name: codex_app_server_protocol::HookEventName::PostToolUse,
        handler_type: codex_app_server_protocol::HookHandlerType::Command,
        execution_mode: codex_app_server_protocol::HookExecutionMode::Sync,
        scope: codex_app_server_protocol::HookScope::Turn,
        source_path: AbsolutePathBuf::from_absolute_path_checked(
            "/workspace/better-codex/config.toml",
        )
        .unwrap(),
        source: codex_app_server_protocol::HookSource::Project,
        display_order: 0,
        status: codex_app_server_protocol::HookRunStatus::Running,
        status_message: None,
        started_at: 1,
        completed_at: None,
        duration_ms: None,
        entries: Vec::new(),
    };
    shell.record_hook_activity(run.clone());
    run.status = codex_app_server_protocol::HookRunStatus::Completed;
    run.status_message = Some("Workspace checks passed.".to_string());
    shell.record_hook_activity(run);
    insta::assert_snapshot!(
        "hook_activity",
        render_shell(&shell, Rect::new(0, 0, 100, 28))
    );
}

#[tokio::test]
async fn side_conversation_preserves_the_running_parent_and_draft() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    let parent_id = shell.thread_id;
    shell.clear_streaming_assistant();
    shell.active_turn_id = Some("parent-turn".to_string());
    shell.composer.set_text("keep my draft");
    let side_id = test_thread_id("11111111-1111-4111-8111-111111111111");
    shell.complete_side_fork(
        &backend,
        parent_id,
        String::new(),
        Ok(started_thread(
            "Side conversation",
            side_id,
            Some(parent_id),
        )),
    );
    assert_eq!(shell.thread_id, side_id);
    assert!(
        shell
            .transcript
            .iter()
            .all(|line| line.kind != TranscriptKind::User)
    );
    shell
        .handle_app_server_event(
            &mut backend,
            AppServerEvent::ServerNotification(Box::new(ServerNotification::AgentMessageDelta(
                codex_app_server_protocol::AgentMessageDeltaNotification {
                    thread_id: parent_id.to_string(),
                    turn_id: "parent-turn".to_string(),
                    item_id: "parent-answer".to_string(),
                    delta: "Still working in the main conversation.".to_string(),
                },
            ))),
        )
        .await
        .unwrap();
    insta::assert_snapshot!(
        "side_conversation",
        render_shell(&shell, Rect::new(0, 0, 100, 28))
    );
    shell.return_from_side(&mut backend).await.unwrap();
    assert_eq!(
        (
            shell.thread_id,
            shell.active_turn_id.as_deref(),
            shell.composer.text()
        ),
        (parent_id, Some("parent-turn"), "keep my draft")
    );
    assert_eq!(
        shell.streaming_assistant,
        "Still working in the main conversation."
    );
}

#[test]
fn resumed_view_only_agents_cannot_receive_turns() {
    let mut shell = ShellState::snapshot_fixture();
    let backend = RecordingBackend::default();
    let mut started = started_thread(
        "Approval reviewer",
        test_thread_id("11111111-1111-4111-8111-111111111111"),
        None,
    );
    started.session.can_accept_direct_input = false;
    shell.replace_started_session(started);
    shell.submit_prompt(&backend, "Run this anyway".to_string());
    assert!(!shell.has_pending_backend_action(ActionGroup::TurnStart));
    assert!(backend.calls.lock().unwrap().is_empty());
    insta::assert_snapshot!(
        "view_only_agent",
        render_shell(&shell, Rect::new(0, 0, 100, 28))
    );
}

#[test]
fn guardian_review_reports_terminal_decision_without_creating_duplicate_activity() {
    let mut shell = ShellState::snapshot_fixture();
    let mut review = codex_app_server_protocol::GuardianApprovalReview {
        status: codex_app_server_protocol::GuardianApprovalReviewStatus::InProgress,
        risk_level: None,
        user_authorization: None,
        rationale: None,
    };
    shell.record_guardian_review("review-1".to_string(), review.clone());
    review.status = codex_app_server_protocol::GuardianApprovalReviewStatus::Denied;
    review.rationale =
        Some("The requested upload includes credentials outside the authorized scope.".to_string());
    shell.record_guardian_review("review-1".to_string(), review);
    assert_eq!(
        shell
            .transcript
            .iter()
            .filter(|line| line.kind == TranscriptKind::Tool
                && line.item_id.as_deref() == Some("guardian:review-1"))
            .count(),
        1
    );
    insta::assert_snapshot!(
        "guardian_review_denied",
        render_shell(&shell, Rect::new(0, 0, 100, 28))
    );
}

#[test]
fn daybreak_state_survives_session_replacement_and_blocks_unknown_model_access() {
    let mut shell = ShellState::snapshot_fixture();
    let backend = RecordingBackend::default();
    let mut started = started_thread(
        "Daybreak work",
        test_thread_id("11111111-1111-4111-8111-111111111111"),
        None,
    );
    started.session.daybreak_enabled = true;
    shell.replace_started_session(started);
    shell.available_models.clear();
    shell.submit_prompt(&backend, "Inspect the findings".to_string());
    assert!(shell.daybreak_enabled);
    assert!(!shell.has_pending_backend_action(ActionGroup::TurnStart));
    insta::assert_snapshot!(
        "daybreak_unconfirmed_model",
        render_shell(&shell, Rect::new(0, 0, 100, 28))
    );
}

#[tokio::test]
async fn returning_from_side_waits_for_pending_turn_submission() {
    for action in [
        ActionGroup::TurnStart,
        ActionGroup::TurnSteer,
        ActionGroup::Workspace,
    ] {
        let mut shell = ShellState::snapshot_fixture();
        let mut backend = RecordingBackend::default();
        let parent_id = shell.thread_id;
        shell.complete_side_fork(
            &backend,
            parent_id,
            String::new(),
            Ok(started_thread(
                "Side conversation",
                test_thread_id("11111111-1111-4111-8111-111111111111"),
                Some(parent_id),
            )),
        );
        shell.start_backend_action(
            action,
            "sending",
            std::future::pending::<BackendActionResult>(),
        );
        let side_id = shell.thread_id;
        shell.return_from_side(&mut backend).await.unwrap();
        assert_eq!(shell.thread_id, side_id);
        assert!(shell.side_parent.is_some());
        assert!(shell.has_pending_backend_action(action));
        if action == ActionGroup::Workspace {
            shell.dashboard_visible = false;
            insta::assert_snapshot!(
                "side_return_waits_for_workspace_action",
                render_shell(&shell, Rect::new(0, 0, 110, 24))
            );
        }
    }
}

#[tokio::test]
async fn pending_session_transitions_preserve_new_text_and_image_submissions() {
    for action in [ActionGroup::SessionSwitch, ActionGroup::ConversationBranch] {
        let mut shell = ShellState::snapshot_fixture();
        let mut backend = RecordingBackend::default();
        shell.composer.set_text("Keep this draft");
        shell
            .composer
            .attach_images(vec![crate::app_shell::attachments::ImageAttachment {
                label: "draft.png".to_string(),
                url: "data:image/png;base64,draft".into(),
            }]);
        let expected = shell.composer.submission_items(shell.composer.text());
        shell.start_backend_action(
            action,
            "changing session",
            std::future::pending::<BackendActionResult>(),
        );
        shell.submit_prompt(&backend, "Keep this draft".to_string());
        shell.queue_current_message(&backend);
        shell.active_turn_id = Some("previous-turn".to_string());
        shell
            .steer_active_turn(&mut backend, "Keep this draft".to_string())
            .await
            .unwrap();
        assert_eq!(
            shell.composer.submission_items(shell.composer.text()),
            expected
        );
        assert!(!shell.composer.has_queued_messages());
        assert!(!shell.has_pending_backend_action(ActionGroup::TurnStart));
        assert!(!shell.has_pending_backend_action(ActionGroup::TurnSteer));
        assert!(backend.calls.lock().unwrap().is_empty());
        if action == ActionGroup::SessionSwitch {
            shell.dashboard_visible = false;
            insta::assert_snapshot!(
                "submission_during_session_transition",
                render_shell(&shell, Rect::new(0, 0, 100, 28))
            );
        }
    }
}

#[tokio::test]
async fn workspace_actions_wait_for_session_transitions_and_settings() {
    for action in [
        ActionGroup::SessionSwitch,
        ActionGroup::ConversationBranch,
        ActionGroup::Settings,
    ] {
        let mut shell = ShellState::snapshot_fixture();
        let mut backend = RecordingBackend::default();
        shell.start_backend_action(
            action,
            "pending",
            std::future::pending::<BackendActionResult>(),
        );
        shell
            .run_workspace_command(
                crate::app_shell::workspace_commands::WorkspaceCommand::Usage,
                "reset confirm",
                &test_config().await,
                &mut backend,
            )
            .await
            .unwrap();
        assert!(!shell.has_pending_backend_action(ActionGroup::Workspace));
        assert!(shell.has_pending_backend_action(action));
        if action == ActionGroup::SessionSwitch {
            shell.dashboard_visible = false;
            insta::assert_snapshot!(
                "workspace_command_during_transition",
                render_shell(&shell, Rect::new(0, 0, 110, 24))
            );
        }
    }
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.start_backend_action(
        ActionGroup::Workspace,
        "pending",
        std::future::pending::<BackendActionResult>(),
    );
    assert!(shell.block_session_switch_if_busy());
}
