use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn rollback_edits_restore_existing_values_and_clear_new_values() {
    let config = Map::from_iter([("model".to_string(), json!("gpt-existing"))]);
    let edits = vec![
        replace_config_value("model", json!("gpt-new")),
        replace_config_value("service_tier", json!("fast")),
    ];

    assert_eq!(
        rollback_edits(&config, &edits).expect("rollback edits should build"),
        vec![
            replace_config_value("model", json!("gpt-existing")),
            replace_config_value("service_tier", JsonValue::Null),
        ]
    );
}

#[tokio::test]
async fn native_permission_picker_applies_complete_modes_and_preserves_saved_defaults() -> Result<()>
{
    use crate::app_shell::ShellState;
    use crate::app_shell::permission_modes::PermissionMode;
    use crate::legacy_core::config::ConfigBuilder;
    use codex_app_server_protocol::ApprovalsReviewer;
    use codex_app_server_protocol::AskForApproval;
    use codex_app_server_protocol::SandboxPolicy;
    use codex_config::LoaderOverrides;

    let home = tempfile::tempdir()?;
    let path = home.path().join("config.toml");
    let saved = "approval_policy = \"on-request\"\nsandbox_mode = \"danger-full-access\"\n";
    tokio::fs::write(&path, saved).await?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    let mut app_server = crate::start_embedded_app_server_for_picker(&config).await?;
    let started = app_server.start_thread(&config).await?;
    let mut shell = ShellState::snapshot_fixture();
    shell.replace_started_session(started);
    shell.configure_permission_picker(&config);
    assert!(shell.custom_permissions_active());
    shell.active_turn_id = None;
    for (mode, profile, approval, reviewer) in [
        (
            PermissionMode::FullAccess,
            ":danger-full-access",
            AskForApproval::Never,
            ApprovalsReviewer::User,
        ),
        (
            PermissionMode::Ask,
            ":workspace",
            AskForApproval::OnRequest,
            ApprovalsReviewer::User,
        ),
        (
            PermissionMode::AutoReview,
            ":workspace",
            AskForApproval::OnRequest,
            ApprovalsReviewer::AutoReview,
        ),
        (
            PermissionMode::ReadOnly,
            ":read-only",
            AskForApproval::OnRequest,
            ApprovalsReviewer::User,
        ),
        (
            PermissionMode::Ask,
            ":workspace",
            AskForApproval::OnRequest,
            ApprovalsReviewer::User,
        ),
    ] {
        shell.apply_permission_mode(mode, &app_server);
        let settings = updated_settings(&mut shell, &mut app_server).await?;
        assert!(shell.settings.custom_permissions.is_some());
        assert!(!shell.custom_permissions_active());
        assert_eq!(shell.permission_mode(), Some(mode));
        assert_eq!(settings.active_permission_profile.unwrap().id, profile);
        assert_eq!(settings.approval_policy, approval);
        assert_eq!(settings.approvals_reviewer, reviewer);
        assert!(matches!(
            (mode, settings.sandbox_policy),
            (PermissionMode::FullAccess, SandboxPolicy::DangerFullAccess)
                | (PermissionMode::ReadOnly, SandboxPolicy::ReadOnly { .. })
                | (
                    PermissionMode::Ask | PermissionMode::AutoReview,
                    SandboxPolicy::WorkspaceWrite {
                        network_access: false,
                        ..
                    }
                )
        ));
    }
    shell.select_custom_permissions(&app_server);
    let restored = updated_settings(&mut shell, &mut app_server).await?;
    assert_eq!(restored.sandbox_policy, SandboxPolicy::DangerFullAccess);
    assert_eq!(restored.approval_policy, AskForApproval::OnRequest);
    assert_eq!(restored.approvals_reviewer, ApprovalsReviewer::User);
    assert!(shell.custom_permissions_active());
    assert_eq!(tokio::fs::read_to_string(&path).await?, saved);
    app_server.shutdown().await?;
    Ok(())
}

async fn updated_settings(
    shell: &mut crate::app_shell::ShellState,
    app_server: &mut crate::app_server_session::AppServerSession,
) -> Result<codex_app_server_protocol::ThreadSettings> {
    use codex_app_server_client::AppServerEvent;
    use codex_app_server_protocol::ServerNotification;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while shell.has_pending_backend_actions() {
            shell.poll_backend_actions(app_server).await;
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let settings = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Some(AppServerEvent::ServerNotification(notification)) =
                app_server.next_event().await
                && let ServerNotification::ThreadSettingsUpdated(updated) = *notification
                && updated.thread_id == shell.thread_id.to_string()
            {
                let settings = updated.thread_settings.clone();
                shell.handle_notification(ServerNotification::ThreadSettingsUpdated(updated));
                break settings;
            }
        }
    })
    .await?;
    Ok(settings)
}

#[tokio::test]
async fn both_permission_pickers_follow_the_configured_workspace_setup() -> Result<()> {
    use crate::app_shell::ShellState;
    use crate::app_shell::selector::SelectorOutcome;
    use crate::app_shell::selector::SelectorState;
    use crate::app_shell::selector::SelectorValue;
    use crate::app_shell::settings::SettingsAction;
    use crate::legacy_core::config::ConfigBuilder;
    use codex_config::LoaderOverrides;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let home = tempfile::tempdir()?;
    let path = home.path().join("config.toml");
    let mut shell = ShellState::snapshot_fixture();
    for (sandbox, policy, reviewer, extra, custom) in [
        (
            "workspace-write",
            "on-request",
            "user",
            "[sandbox_workspace_write]\nnetwork_access = true",
            true,
        ),
        ("workspace-write", "on-request", "user", "", false),
        ("workspace-write", "on-request", "auto_review", "", false),
        ("danger-full-access", "never", "user", "", false),
        ("read-only", "on-request", "user", "", false),
    ] {
        let saved = format!(
            "sandbox_mode = {sandbox:?}\napproval_policy = {policy:?}\napprovals_reviewer = {reviewer:?}\n{extra}\n"
        );
        tokio::fs::write(&path, saved).await?;
        let config = ConfigBuilder::default()
            .codex_home(home.path().into())
            .fallback_cwd(Some(home.path().into()))
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .build()
            .await?;
        shell.configure_permission_picker(&config);
        let mut picker = SelectorState::permissions(&shell);
        assert_eq!(
            matches!(
                picker.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE)),
                SelectorOutcome::Selected(SelectorValue::CustomPermissions)
            ),
            custom,
            "{sandbox} {extra}"
        );
        let dashboard_custom = shell
            .settings
            .lines(&shell, 60)
            .iter()
            .position(|line| line.to_string().contains("Custom"));
        assert_eq!(dashboard_custom.is_some(), custom, "{sandbox} {extra}");
        if let Some(line) = dashboard_custom {
            assert_eq!(
                shell.settings.action_at_line(line),
                Some(SettingsAction::CustomPermissions)
            );
            let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
            shell.replace_started_session(server.start_thread(&config).await?);
            shell.select_permission_mode(
                crate::app_shell::permission_modes::PermissionMode::ReadOnly,
                &server,
            );
            updated_settings(&mut shell, &mut server).await?;
            shell.select_custom_permissions(&server);
            let settings = updated_settings(&mut shell, &mut server).await?;
            assert!(matches!(
                settings.sandbox_policy,
                codex_app_server_protocol::SandboxPolicy::WorkspaceWrite {
                    network_access: true,
                    ..
                }
            ));
            assert!(shell.custom_permissions_active());
            server.shutdown().await?;
            shell
                .settings
                .focus_action(SettingsAction::CustomPermissions);
        } else {
            assert_ne!(
                shell.settings.selected_action(),
                SettingsAction::CustomPermissions
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn restoring_named_custom_permissions_reselects_the_server_profile() -> Result<()> {
    use crate::app_shell::ShellState;
    use crate::app_shell::permission_modes::PermissionMode;
    use crate::legacy_core::config::ConfigBuilder;
    use codex_config::LoaderOverrides;

    let home = tempfile::tempdir()?;
    let saved = r#"default_permissions = "audit"
approval_policy = "on-request"
[permissions.audit.filesystem]
":root" = "read"
[permissions.audit.filesystem.":workspace_roots"]
"reports" = "write"
"#;
    tokio::fs::write(home.path().join("config.toml"), saved).await?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().into())
        .fallback_cwd(Some(home.path().into()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    let mut shell = ShellState::snapshot_fixture();
    shell.replace_started_session(server.start_thread(&config).await?);
    shell.configure_permission_picker(&config);
    assert!(shell.custom_permissions_active());
    shell.select_permission_mode(PermissionMode::ReadOnly, &server);
    updated_settings(&mut shell, &mut server).await?;
    assert!(!shell.custom_permissions_active());
    shell.select_custom_permissions(&server);
    let settings = updated_settings(&mut shell, &mut server).await?;
    assert_eq!(settings.active_permission_profile.unwrap().id, "audit");
    assert!(shell.custom_permissions_active());
    server.shutdown().await?;
    Ok(())
}
