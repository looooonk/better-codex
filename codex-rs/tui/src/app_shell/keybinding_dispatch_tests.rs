use super::*;
use crate::app_shell::keybindings::ShellKeymap;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn remapped_word_delete_reaches_the_composer_and_disables_legacy_aliases() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.active_turn_id = None;
    shell.keybindings = ShellKeymap::from_config(
        &toml::from_str("[editor]\ndelete_backward_word = 'f18'").unwrap(),
    )
    .unwrap();
    shell.composer.set_text("one café");
    let mut backend = RecordingBackend::default();
    for key in [
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('\u{007f}'), KeyModifiers::CONTROL),
    ] {
        shell.handle_key(key, &config, &mut backend).await.unwrap();
    }
    assert_eq!(shell.composer.submission_text(), "one café");
    shell
        .handle_key(
            KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    assert_eq!(shell.composer.submission_text(), "one ");
    assert!(backend.calls().is_empty());
}

#[tokio::test]
async fn remapped_editor_controls_and_submit_reach_existing_dispatch_once() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.active_turn_id = None;
    shell.keybindings = ShellKeymap::from_config(
        &toml::from_str("[composer]\nsubmit = 'f18'\n[editor]\nmove_left = 'f19'").unwrap(),
    )
    .unwrap();
    shell.composer.set_text("ab");
    let mut backend = RecordingBackend::default();
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    shell
        .handle_key(
            KeyEvent::new(KeyCode::F(19), KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    shell
        .handle_key(key_char('X'), &config, &mut backend)
        .await
        .unwrap();
    assert_eq!(shell.composer.submission_text(), "aXb");
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    assert!(!shell.has_pending_backend_actions());
    shell
        .handle_key(
            KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(
        backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| matches!(call, RecordedBackendCall::TurnStart { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn completed_native_chord_submits_through_the_existing_backend_once() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.active_turn_id = None;
    shell.keybindings =
        ShellKeymap::from_config(&toml::from_str("[composer]\nsubmit = 'ctrl-x ctrl-s'").unwrap())
            .unwrap();
    shell.composer.set_text("Submit this chord");
    let mut backend = RecordingBackend::default();
    for ch in ['x', 's'] {
        shell
            .handle_key(
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL),
                &config,
                &mut backend,
            )
            .await
            .unwrap();
    }
    complete_backend_actions(&mut shell, &backend).await;
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(
        backend
            .calls()
            .iter()
            .filter(|call| matches!(call, RecordedBackendCall::TurnStart { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn remapped_approval_respects_available_decisions_and_unbound_default() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.keybindings =
        ShellKeymap::from_config(&toml::from_str("[approval]\napprove = 'f18'").unwrap()).unwrap();
    shell.pending_approval = PendingApproval::from_request(&command_approval_request()).unwrap();
    let mut backend = RecordingBackend::default();
    shell
        .handle_key(key_char('a'), &config, &mut backend)
        .await
        .unwrap();
    assert!(shell.pending_approval.is_some());
    shell
        .handle_key(
            KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(
        *backend.resolved_requests.lock().unwrap(),
        vec![(RequestId::Integer(41), json!({"decision":"accept"}))]
    );
}

#[tokio::test]
async fn remapped_selector_accept_and_keymap_picker_share_the_current_hints() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.keybindings = ShellKeymap::from_config(
        &toml::from_str("[list]\naccept = 'f18'\ncancel = 'f19'").unwrap(),
    )
    .unwrap();
    shell.run_keymap_command("", &config).await.unwrap();
    let mut backend = RecordingBackend::default();
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    assert!(shell.selector.is_some());
    insta::assert_snapshot!(
        "remapped_keymap_picker",
        render_shell(&shell, Rect::new(0, 0, 110, 28))
    );
    shell
        .handle_key(
            KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    assert!(shell.selector.is_none());
    assert!(shell.composer.submission_text().starts_with("/keymap "));
}

#[tokio::test]
async fn fast_shortcut_uses_model_capabilities_and_restores_default_tier() {
    let mut config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.keybindings =
        ShellKeymap::from_config(&toml::from_str("[global]\ntoggle_fast_mode = 'f18'").unwrap())
            .unwrap();
    config
        .features
        .enable(codex_features::Feature::FastMode)
        .unwrap();
    shell.active_turn_id = None;
    let key = KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE);
    let mut backend = RecordingBackend::default();
    shell.available_models = vec![model_preset_fixture(
        &shell.model,
        true,
        ReasoningEffort::Medium,
        &[ReasoningEffort::Medium],
        &[],
    )];
    shell.handle_key(key, &config, &mut backend).await.unwrap();
    assert!(backend.calls().is_empty());
    assert!(
        shell
            .transcript
            .back()
            .unwrap()
            .text
            .contains("unavailable")
    );
    shell.available_models = vec![model_preset_fixture(
        &shell.model,
        true,
        ReasoningEffort::Medium,
        &[ReasoningEffort::Medium],
        &["priority"],
    )];
    shell.handle_key(key, &config, &mut backend).await.unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.service_tier, Some("priority".to_string()));
    shell.handle_key(key, &config, &mut backend).await.unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.service_tier, Some("default".to_string()));
}

#[tokio::test]
async fn find_shortcut_searches_retained_text_without_replacing_the_draft() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.transcript.clear();
    shell.push_user("Find the café response");
    shell.push_assistant("The café serves tea");
    shell.push_assistant("An unrelated answer");
    shell.composer.set_text("Keep this draft");
    shell.keybindings =
        ShellKeymap::from_config(&toml::from_str("[global]\nfind_transcript = 'f18'").unwrap())
            .unwrap();
    let mut backend = RecordingBackend::default();
    shell
        .handle_key(
            KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    for ch in "café".chars() {
        shell
            .handle_key(key_char(ch), &config, &mut backend)
            .await
            .unwrap();
    }
    insta::assert_snapshot!(
        "retained_transcript_find",
        render_shell(&shell, Rect::new(0, 0, 110, 28))
    );
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    assert_eq!(shell.transcript_selection, Some(1));
    assert_eq!(shell.composer.submission_text(), "Keep this draft");
    assert!(backend.calls().is_empty());
}

#[test]
fn find_caps_query_and_results_and_rejects_a_stale_match() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    for index in 0..100 {
        shell.push_user(format!("item {index}"));
    }
    let stale = shell.transcript.front().unwrap().render_revision;
    shell.open_transcript_find(&"é".repeat(300));
    assert_eq!(shell.transcript_find.as_ref().unwrap().text().len(), 256);
    shell.composer.set_text("Unchanged draft");
    shell.open_transcript_find("");
    shell.insert_pasted_text(&"é".repeat(300));
    assert_eq!(shell.transcript_find.as_ref().unwrap().text().len(), 256);
    assert_eq!(shell.composer.submission_text(), "Unchanged draft");
    shell.open_transcript_find("");
    let rendered = render_shell(&shell, Rect::new(0, 0, 110, 28));
    assert!(rendered.contains("1/50"));
    shell.transcript.pop_front();
    shell.select_transcript_match(stale);
    assert_eq!(shell.transcript_selection, None);
    assert!(
        shell
            .transcript
            .back()
            .unwrap()
            .text
            .contains("changed or left")
    );
}

#[tokio::test]
async fn configured_find_navigation_does_not_edit_the_query() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.push_user("old match");
    shell.push_assistant("new match");
    shell.keybindings = ShellKeymap::from_config(
        &toml::from_str("[list]\njump_bottom = 'f18'\nmove_down = 'j'").unwrap(),
    )
    .unwrap();
    shell.open_transcript_find("");
    let mut backend = RecordingBackend::default();
    shell
        .handle_key(key_char('j'), &config, &mut backend)
        .await
        .unwrap();
    assert_eq!(shell.transcript_find.as_ref().unwrap().text(), "j");
    shell.open_transcript_find("match");
    shell
        .handle_key(
            KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    assert_eq!(shell.transcript_find.as_ref().unwrap().text(), "match");
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    assert_eq!(shell.transcript_selection, Some(0));
}
