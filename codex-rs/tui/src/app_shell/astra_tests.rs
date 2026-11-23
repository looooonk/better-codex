use super::*;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ThreadItemPayload;
use pretty_assertions::assert_eq;

#[test]
fn composer_buttons_reflect_the_active_policy_speed_and_model_effort() {
    use composer_toolbar::ComposerControl;
    use composer_toolbar::ComposerToolbar;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.model = "gpt-6.2-astra".into();
    shell.available_models = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .map(Into::into)
        .collect();
    for (policy, speed, effort, expected) in [
        (
            AskForApproval::OnRequest,
            None,
            None,
            ["ⓘ Ask for approval", "low", "◷ Default"],
        ),
        (
            AskForApproval::UnlessTrusted,
            Some("fast"),
            None,
            ["⛨ Unless trusted", "low", "↯ Fast"],
        ),
        (
            AskForApproval::Never,
            Some("priority"),
            Some(ReasoningEffort::High),
            ["⚠ Full access", "high", "↯ Fast"],
        ),
        (
            AskForApproval::OnRequest,
            Some("flex"),
            None,
            ["ⓘ Ask for approval", "low", "≈ flex"],
        ),
        (
            AskForApproval::OnRequest,
            Some("ultrafast"),
            None,
            ["ⓘ Ask for approval", "low", "⇈ ultrafast"],
        ),
        (
            AskForApproval::OnRequest,
            Some("custom"),
            None,
            ["ⓘ Ask for approval", "low", "◆ custom"],
        ),
    ] {
        shell.approval_policy = policy;
        shell.service_tier = speed.map(str::to_owned);
        shell.reasoning_effort = effort;
        for width in [40, 50, 80] {
            let area = Rect::new(0, 0, width, 20);
            let toolbar =
                ComposerToolbar::new(&shell, ShellView { shell: &shell }.input_area(area));
            let buffer = render_shell_buffer(&shell, area);
            let actual = [
                ComposerControl::Permissions,
                ComposerControl::ReasoningEffort,
                ComposerControl::ServiceTier,
            ]
            .map(|control| {
                let button = toolbar.button_area(control);
                assert_eq!(toolbar.control_at(button.as_position()), Some(control));
                buffer_contents(&buffer, button)
                    .trim()
                    .trim_end_matches('▾')
                    .trim_end()
                    .to_owned()
            });
            assert_eq!(actual, expected.map(str::to_owned));
            let expected_color = if policy == AskForApproval::Never {
                design::palette::error()
            } else {
                design::palette::purple()
            };
            assert_eq!(
                buffer[toolbar
                    .button_area(ComposerControl::Permissions)
                    .as_position()]
                .fg,
                expected_color
            );
        }
    }
}

#[test]
fn astra_catalog_drives_model_reasoning_and_fast_selectors() {
    let mut shell = ShellState::snapshot_fixture();
    shell.model = "gpt-6.2-astra".to_string();
    shell.reasoning_effort = Some(ReasoningEffort::Ultra);
    shell.available_models = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .map(Into::into)
        .collect();
    shell.open_model_selector();
    insta::assert_snapshot!(
        "astra_model_selector",
        render_shell(&shell, Rect::new(0, 0, 100, 30))
    );
    shell.open_reasoning_selector();
    insta::assert_snapshot!(
        "astra_reasoning_selector",
        render_shell(&shell, Rect::new(0, 0, 100, 30))
    );
    shell.open_service_tier_selector();
    insta::assert_snapshot!(
        "astra_service_tier_selector",
        render_shell(&shell, Rect::new(0, 0, 100, 30))
    );
}

#[test]
fn astra_async_question_is_visible_while_the_turn_runs() {
    let mut shell = ShellState::snapshot_fixture();
    shell.ingest_completed_item(
        ThreadItem::untimed(ThreadItemPayload::AgentMessage {
            delivery: None,
            questions: Some(vec![codex_app_server_protocol::AsyncUserInputQuestion {
                title: "Which approach?".to_string(),
                options: Some(vec!["Small change".to_string(), "Full rewrite".to_string()]),
            }]),

            id: "question-1".to_string(),
            text: String::new(),
            phase: Some(codex_protocol::models::MessagePhase::Commentary),
            memory_citation: None,
        }),
        CompletedItemOrigin::Live,
    );
    insta::assert_snapshot!(
        "astra_async_question",
        render_shell(&shell, Rect::new(0, 0, 100, 30))
    );
}

#[tokio::test]
async fn effort_slider_previews_drags_and_applies_on_release() {
    let mut shell = ShellState::snapshot_fixture();
    shell.model = "gpt-6.2-astra".to_string();
    shell.reasoning_effort = Some(ReasoningEffort::Medium);
    shell.available_models = vec![model_preset_fixture(
        &shell.model,
        true,
        ReasoningEffort::Medium,
        &[
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::Ultra,
        ],
        &[],
    )];
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let area = Rect::new(0, 0, 100, 28);
    let view = ShellView { shell: &shell };
    let button = position_in(view.input_area(area), |p| {
        view.composer_control_at(area, p)
            == Some(composer_toolbar::ComposerControl::ReasoningEffort)
    });
    shell
        .handle_mouse_selection_down(area, button, &config, &mut backend)
        .await
        .unwrap();
    shell
        .handle_mouse_selection_release(area, button, &config, &mut backend)
        .await
        .unwrap();
    let panel = ShellView { shell: &shell }.selector_area(area);
    let rail = position_in(panel, |p| {
        shell.selector.as_ref().unwrap().option_at(panel, p) == Some(1)
    });
    let right = Position::new(panel.right() + 1, rail.y);
    shell
        .handle_mouse_selection_down(area, rail, &config, &mut backend)
        .await
        .unwrap();
    shell.handle_mouse_selection_drag(area, right);
    let preview = render_shell_buffer(&shell, area);
    assert!(buffer_contents(&preview, panel).contains("Ultra"));
    let middle = rail.y + 1;
    let start = rail.x + 9;
    let color = preview[(start, middle)].bg;
    assert_ne!(preview[(start + 9, middle)].bg, color);
    // The 36-column rail moves right by nine columns in a quarter cycle.
    let selector = shell.selector.as_mut().unwrap();
    assert!(selector.shows_ultra());
    for _ in 0..10 {
        selector.advance_rainbow();
    }
    let next_frame = render_shell_buffer(&shell, area);
    assert_eq!(next_frame[(start + 9, middle)].bg, color);
    assert_eq!(shell.reasoning_effort, Some(ReasoningEffort::Medium));
    assert!(backend.calls().is_empty());

    // Esc cancels the drag, including a delayed terminal mouse-up.
    press_plain_key(&mut shell, &config, &mut backend, KeyCode::Esc).await;
    shell
        .handle_mouse_selection_release(area, right, &config, &mut backend)
        .await
        .unwrap();
    assert!(backend.calls().is_empty());

    shell.open_reasoning_selector();
    shell
        .handle_mouse_selection_down(area, rail, &config, &mut backend)
        .await
        .unwrap();
    shell.handle_mouse_selection_drag(area, right);
    shell
        .handle_mouse_selection_release(area, right, &config, &mut backend)
        .await
        .unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.reasoning_effort, Some(ReasoningEffort::Ultra));
    assert!(shell.selector.as_ref().unwrap().shows_ultra());

    press_plain_key(&mut shell, &config, &mut backend, KeyCode::Left).await;
    press_plain_key(&mut shell, &config, &mut backend, KeyCode::Enter).await;
    assert!(!shell.selector.as_ref().unwrap().shows_ultra());
    // Choose another stop before the preceding write completes, then dismiss.
    press_plain_key(&mut shell, &config, &mut backend, KeyCode::Left).await;
    press_plain_key(&mut shell, &config, &mut backend, KeyCode::Enter).await;
    shell
        .handle_mouse_selection_down(area, Position::new(2, 2), &config, &mut backend)
        .await
        .unwrap();
    assert!(shell.selector.is_none());
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.reasoning_effort, Some(ReasoningEffort::Low));
    assert!(shell.selector.is_none());
}

#[tokio::test]
async fn composer_permissions_button_opens_the_policy_picker() {
    let mut shell = ShellState::snapshot_fixture();
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let area = Rect::new(0, 0, 100, 28);
    let view = ShellView { shell: &shell };
    let input = view.input_area(area);
    let button = position_in(input, |p| {
        view.composer_control_at(area, p) == Some(composer_toolbar::ComposerControl::Permissions)
    });
    let effort = position_in(input, |p| {
        view.composer_control_at(area, p)
            == Some(composer_toolbar::ComposerControl::ReasoningEffort)
    });
    let buffer = render_shell_buffer(&shell, area);
    assert_eq!(buffer[button].style(), buffer[effort].style());
    assert_eq!(button.y, input.bottom() - 2);
    shell
        .handle_mouse_selection_down(area, button, &config, &mut backend)
        .await
        .unwrap();
    let panel = ShellView { shell: &shell }.selector_area(area);
    assert_eq!(panel.bottom(), button.y);
    assert_eq!(panel.x, button.x);
    let popup = render_shell_buffer(&shell, area);
    assert_eq!(popup[(area.x, area.y)], buffer[(area.x, area.y)]);
    insta::assert_snapshot!("composer_permissions_menu", buffer_contents(&popup, panel));
    assert!(backend.calls().is_empty());

    let full_access = text_color_for_row(&popup, panel, "Full access");
    assert_eq!(full_access, Some(design::palette::error()));
    press_plain_key(&mut shell, &config, &mut backend, KeyCode::Char('3')).await;
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.approval_policy, AskForApproval::Never);
    let buffer = render_shell_buffer(&shell, area);
    assert_eq!(text_color_for_row(&buffer, input, "⚠ Full access"), full_access);
}

#[tokio::test]
async fn composer_menus_switch_anchors_and_select_visible_rows_after_resize() {
    use composer_toolbar::ComposerControl;
    let mut shell = ShellState::snapshot_fixture();
    shell.model = "gpt-6.2-astra".to_string();
    shell.available_models = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .map(Into::into)
        .collect();
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let wide = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 28,
    );
    for (control, snapshot) in [
        (ComposerControl::ServiceTier, "composer_speed_menu"),
        (ComposerControl::Model, "composer_model_menu"),
    ] {
        let view = ShellView { shell: &shell };
        let input = view.input_area(wide);
        let button = position_in(input, |p| {
            view.composer_control_at(wide, p) == Some(control)
        });
        shell
            .handle_mouse_click(wide, button, &config, &mut backend)
            .await
            .unwrap();
        let panel = ShellView { shell: &shell }.selector_area(wide);
        assert_eq!(panel.bottom(), button.y);
        assert!(panel.x <= button.x && panel.right() > button.x);
        insta::assert_snapshot!(
            snapshot,
            buffer_contents(&render_shell_buffer(&shell, wide), panel)
        );
    }

    let narrow = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 40, /*height*/ 11,
    );
    press_plain_key(&mut shell, &config, &mut backend, KeyCode::End).await;
    let panel = ShellView { shell: &shell }.selector_area(narrow);
    let selector = shell.selector.as_ref().unwrap();
    let count = shell
        .available_models
        .iter()
        .filter(|m| m.show_in_picker)
        .count();
    let last = position_in(panel, |p| selector.option_at(panel, p) == Some(count - 1));
    let buf = render_shell_buffer(&shell, narrow);
    let label = buffer_contents(&buf, Rect::new(panel.x + 1, last.y, panel.width - 2, 1));
    let expected = shell
        .available_models
        .iter()
        .find(|m| m.display_name == label.trim())
        .expect("last visible menu row names a catalog model")
        .model
        .clone();
    shell
        .handle_mouse_click(narrow, last, &config, &mut backend)
        .await
        .unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.model, expected);
    assert!(shell.selector.is_none());
}
