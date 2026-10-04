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
async fn native_untrusted_policy_is_session_only_and_keeps_saved_defaults() -> Result<()> {
    use crate::app_shell::ShellState;
    use crate::legacy_core::config::ConfigBuilder;
    use codex_app_server_client::AppServerEvent;
    use codex_app_server_protocol::AskForApproval;
    use codex_app_server_protocol::ServerNotification;
    use codex_config::LoaderOverrides;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;

    let home = tempfile::tempdir()?;
    let path = home.path().join("config.toml");
    let saved = "approval_policy = \"on-request\"\n";
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
    shell.apply_approval_policy(AskForApproval::UnlessTrusted, &mut app_server)?;
    tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 10), async {
        while shell.has_pending_backend_actions() {
            shell.poll_backend_actions(&app_server).await;
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let applied = tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 10), async {
        loop {
            let event = app_server
                .next_event()
                .await
                .expect("native app-server event stream");
            if let AppServerEvent::ServerNotification(notification) = event
                && let ServerNotification::ThreadSettingsUpdated(updated) = *notification
                && updated.thread_id == shell.thread_id.to_string()
            {
                break updated.thread_settings.approval_policy;
            }
        }
    })
    .await?;
    assert_eq!(
        (
            applied,
            shell.approval_policy,
            tokio::fs::read_to_string(&path).await?
        ),
        (
            AskForApproval::UnlessTrusted,
            AskForApproval::UnlessTrusted,
            saved.to_string()
        )
    );
    shell.dashboard_visible = false;
    shell.open_approval_selector();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 28,
    );
    let mut buffer = Buffer::empty(area);
    crate::app_shell::render::ShellView { shell: &shell }.render(area, &mut buffer);
    let rendered = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("session_only_untrusted_policy", rendered);
    app_server.shutdown().await?;
    Ok(())
}
