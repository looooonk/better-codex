use super::*;
use pretty_assertions::assert_eq;

#[test]
fn restored_child_settings_follow_native_metadata_and_live_updates() {
    let mut shell = ShellState::snapshot_fixture();
    let root_settings = (shell.model.clone(), shell.reasoning_effort.clone());
    let child_id = ThreadId::from_string("01900000-0000-7000-8000-000000000002").unwrap();
    let mut child = subagent_thread_fixture(
        child_id,
        shell.thread_id,
        shell.thread_id,
        "/root/review",
        "Review",
    );
    child.model = Some("gpt-6.1-sol".into());
    child.reasoning_effort = Some(ReasoningEffort::High);
    shell.handle_notification(ServerNotification::ThreadStarted(
        ThreadStartedNotification {
            thread: child.clone(),
        },
    ));
    let agent = shell.agent_activity.agent(&child_id.to_string()).unwrap();
    assert_eq!(
        (&agent.model, &agent.reasoning_effort),
        (&child.model, &child.reasoning_effort)
    );

    let mut settings = codex_app_server_protocol::ThreadSettings {
        disabled_plugin_ids: Vec::new(),
        cwd: test_absolute_path("workspace/review"),
        approval_policy: codex_app_server_protocol::AskForApproval::Never,
        approvals_reviewer: codex_app_server_protocol::ApprovalsReviewer::User,
        sandbox_policy: codex_app_server_protocol::SandboxPolicy::DangerFullAccess,
        active_permission_profile: None,
        model: "gpt-6-astra".into(),
        model_provider: "openai".into(),
        service_tier: None,
        effort: None,
        summary: None,
        collaboration_mode: *collaboration_mode_fixture("gpt-6-astra", None),
        multi_agent_mode: Default::default(),
        personality: None,
    };
    shell.handle_notification(ServerNotification::ThreadSettingsUpdated(
        ThreadSettingsUpdatedNotification {
            thread_id: child_id.to_string(),
            thread_settings: settings.clone(),
        },
    ));
    shell
        .agent_activity
        .reduce_completed(&ThreadItem::CollabAgentToolCall {
            id: "old-spawn".into(),
            tool: CollabAgentTool::SpawnAgent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: shell.thread_id.to_string(),
            receiver_thread_ids: vec![child_id.to_string()],
            prompt: None,
            model: Some("old-model".into()),
            reasoning_effort: Some(ReasoningEffort::Low),
            agents_states: HashMap::new(),
        });
    shell.agent_activity.hydrate_threads(vec![child]);
    let agent = shell.agent_activity.agent(&child_id.to_string()).unwrap();
    assert_eq!(
        (&agent.model, &agent.reasoning_effort),
        (&Some("gpt-6-astra".into()), &None)
    );
    assert_eq!(
        (shell.model.clone(), shell.reasoning_effort.clone()),
        root_settings
    );

    let before = shell.agent_activity.clone();
    shell.handle_notification(ServerNotification::ThreadSettingsUpdated(
        ThreadSettingsUpdatedNotification {
            thread_id: "unrelated-thread".into(),
            thread_settings: settings.clone(),
        },
    ));
    assert_eq!(shell.agent_activity, before);

    settings.model = "界".repeat(512);
    settings.effort = Some(ReasoningEffort::Custom("考".repeat(512)));
    shell.handle_notification(ServerNotification::ThreadSettingsUpdated(
        ThreadSettingsUpdatedNotification {
            thread_id: child_id.to_string(),
            thread_settings: settings,
        },
    ));
    let agent = shell.agent_activity.agent(&child_id.to_string()).unwrap();
    assert_eq!(
        (&agent.model, &agent.reasoning_effort),
        (
            &Some(format!("{}...", "界".repeat(125))),
            &Some(ReasoningEffort::Custom(format!("{}...", "考".repeat(125)))),
        )
    );
}
