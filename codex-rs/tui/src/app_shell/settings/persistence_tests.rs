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
    use codex_app_server_client::AppServerEvent;
    use codex_app_server_protocol::ApprovalsReviewer;
    use codex_app_server_protocol::AskForApproval;
    use codex_app_server_protocol::SandboxPolicy;
    use codex_app_server_protocol::ServerNotification;
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
    shell.thread_id = started.session.thread_id;
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
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while shell.has_pending_backend_actions() {
                shell.poll_backend_actions(&app_server).await;
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
    assert_eq!(tokio::fs::read_to_string(&path).await?, saved);
    app_server.shutdown().await?;
    Ok(())
}
