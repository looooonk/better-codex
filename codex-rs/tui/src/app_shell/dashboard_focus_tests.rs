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
    for route in [DashboardRoute::Sessions, DashboardRoute::Settings] {
        let mut shell = ShellState::snapshot_fixture();
        shell.dashboard_visible = false;
        shell.set_dashboard_route(route);
        shell.session_list.focused = route == DashboardRoute::Sessions;
        shell.settings.focused = route == DashboardRoute::Settings;
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

#[tokio::test]
async fn settings_rows_apply_in_place_and_remain_reachable_on_a_short_terminal() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.dashboard_route = DashboardRoute::Settings;

    let wide = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 32,
    );
    let buf = render_shell_buffer(&shell, wide);
    let row = row_containing(&buf, wide, "Gruvbox Dark").unwrap();
    let x = row_needle_x(&buf, wide, row, "Gruvbox Dark").unwrap();
    shell
        .handle_mouse_click(wide, Position::new(x, row), &config, &mut backend)
        .await
        .unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.app_theme, TuiAppTheme::GruvboxDark);
    assert!(shell.selector.is_none());
    insta::assert_snapshot!("settings_dashboard", render_shell(&shell, wide));

    let short = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 48, /*height*/ 16,
    );
    for key in [KeyCode::Tab, KeyCode::Down] {
        shell
            .handle_key(
                KeyEvent::new(key, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .unwrap();
    }
    let rendered = render_shell(&shell, short);
    assert_eq!(shell.settings.selected_action(), SettingsAction::Plugins);
    assert!(rendered.contains("› Plugins"), "{rendered}");
    insta::assert_snapshot!("settings_dashboard_short", rendered);
}

#[tokio::test]
async fn clicking_the_inline_theme_editor_restores_focus_and_preserves_the_draft() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.dashboard_route = DashboardRoute::Settings;
    shell.settings.start_theme_edit("unknown-theme".to_owned());
    shell.settings.set_error("Unknown syntax theme");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 32,
    );

    for label in [
        "Edit unknown-theme",
        "Unknown syntax theme",
        "Syntax theme:",
    ] {
        shell.settings.focused = false;
        let buf = render_shell_buffer(&shell, area);
        let y = row_containing(&buf, area, label).unwrap();
        let x = row_needle_x(&buf, area, y, label).unwrap();
        let pointer = Position::new(x, y);
        shell
            .handle_mouse_selection_down(area, pointer, &config, &mut backend)
            .await
            .unwrap();
        shell
            .handle_mouse_selection_release(area, pointer, &config, &mut backend)
            .await
            .unwrap();
        assert_eq!(
            (shell.settings.focused, shell.settings.edit_value()),
            (true, Some("unknown-theme".to_owned()))
        );
    }
    assert!(backend.calls().is_empty());
}
