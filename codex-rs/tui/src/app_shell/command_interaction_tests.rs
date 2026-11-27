use super::*;
use crate::app_shell::workspace_requests::WorkspaceResponse;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn settings_aliases_and_uncached_integrations_open_their_native_views() {
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    for name in ["/settings", "/options", "/mcp", "/plugins"] {
        let mut shell = ShellState::snapshot_fixture();
        shell.dashboard_visible = false;
        shell.mcp_catalog = None;
        shell.plugin_catalog = None;
        shell.composer.set_text(name);
        shell
            .handle_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .unwrap();
        assert_eq!(shell.dashboard_route, DashboardRoute::Settings);
        assert!(shell.settings.focused);
        assert_eq!(shell.pending_mcp_management.is_some(), name == "/mcp");
        assert_eq!(
            shell.pending_plugin_management.is_some(),
            name == "/plugins"
        );
    }
}

#[tokio::test]
async fn a_command_report_scrolls_and_returns_to_the_existing_draft() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.composer.set_text("Keep this draft");
    let transcript = shell.transcript.clone();
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let body = (1..=30)
        .map(|n| format!("Result {n:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    shell.complete_workspace_request(
        shell.thread_id,
        Ok(WorkspaceResponse::Report("Results", body)),
    );
    let area = Rect::new(0, 0, 48, 14);
    let before = render_shell(&shell, area);
    assert!(before.contains("Result 01"));
    shell
        .handle_key(
            KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    insta::assert_snapshot!("command_report_narrow", render_shell(&shell, area));
    shell.insert_pasted_text("ignored while reading");
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    assert!(shell.preview.is_none());
    assert_eq!(shell.composer.text(), "Keep this draft");
    assert_eq!(shell.transcript, transcript);
}
