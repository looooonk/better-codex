use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn cancelling_model_or_permissions_keeps_hidden_dashboard_input_in_the_composer() {
    let config = test_config().await;
    for command in ["/model", "/permissions"] {
        let mut shell = ShellState::snapshot_fixture();
        shell.dashboard_visible = false;
        shell.active_turn_id = None;
        shell.composer.clear();
        let mut backend = RecordingBackend::default();
        for ch in command.chars() {
            shell
                .handle_key(key_char(ch), &config, &mut backend)
                .await
                .unwrap();
        }
        shell
            .handle_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .unwrap();
        assert!(shell.selector.is_some());
        shell
            .handle_key(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .unwrap();
        for ch in "/statusline ".chars() {
            shell
                .handle_key(key_char(ch), &config, &mut backend)
                .await
                .unwrap();
        }
        shell.insert_pasted_text("thread-name");
        assert_eq!(shell.composer.submission_text(), "/statusline thread-name");
        assert!(shell.composer_owns_focus());
        assert!(shell.selector.is_none());
        assert!(backend.calls().is_empty());
        if command == "/model" {
            insta::assert_snapshot!(
                "composer_after_model_cancel",
                render_shell(
                    &shell,
                    Rect::new(
                        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
                    )
                )
            );
        }
    }
}

#[tokio::test]
async fn hidden_dashboard_routes_cannot_steal_keys_repeats_or_pastes() {
    let config = test_config().await;
    for route in [DashboardRoute::Sessions, DashboardRoute::Status] {
        let mut shell = ShellState::snapshot_fixture();
        shell.dashboard_visible = false;
        shell.set_dashboard_route(route);
        shell.session_list.focused = route == DashboardRoute::Sessions;
        shell.settings.focused = route == DashboardRoute::Status;
        shell.active_turn_id = None;
        shell.composer.clear();
        let mut backend = RecordingBackend::default();
        for ch in "/statuslin".chars() {
            shell
                .handle_key(key_char(ch), &config, &mut backend)
                .await
                .unwrap();
        }
        shell
            .handle_key(
                KeyEvent::new_with_kind(
                    KeyCode::Char('e'),
                    KeyModifiers::NONE,
                    KeyEventKind::Repeat,
                ),
                &config,
                &mut backend,
            )
            .await
            .unwrap();
        shell.insert_pasted_text(" thread-name");
        assert_eq!(shell.composer.submission_text(), "/statusline thread-name");
        assert!(shell.composer_owns_focus());
        assert!(!shell.session_list.search_active());
        assert!(shell.selector.is_none());
        assert!(backend.calls().is_empty());
    }
}
