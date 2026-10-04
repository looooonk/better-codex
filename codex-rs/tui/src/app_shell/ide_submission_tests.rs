use super::super::queued_messages::QueueMutation;
use super::*;
use pretty_assertions::assert_eq;

fn context() -> crate::ide_context::IdeContext {
    serde_json::from_value(serde_json::json!({"activeFile": null, "openTabs": [{"label": "lib.rs", "path": "src/lib.rs"}]})).unwrap()
}

fn steer(shell: &ShellState, turn_id: &str) -> backend::AppShellTurnSteer {
    backend::AppShellTurnSteer {
        thread_id: shell.thread_id,
        turn_id: turn_id.to_string(),
        client_user_message_id: "ide-steer".to_string(),
        items: vec![UserInput::Text {
            text: "captured message".to_string(),
            text_elements: Vec::new(),
        }],
    }
}

#[tokio::test]
async fn prepared_steer_keeps_a_new_draft_and_stale_turn_restores_the_submission() {
    let backend = RecordingBackend::default();
    let mut shell = ShellState::snapshot_fixture();
    shell.ide.enabled = true;
    shell.active_turn_id = Some("active".to_string());
    shell.composer.set_text("new draft");
    shell.complete_ide_steer(
        &backend,
        steer(&shell, "active"),
        "captured message".to_string(),
        Ok(context()),
    );
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.composer.submission_text(), "new draft");
    assert_eq!(
        backend.calls(),
        vec![RecordedBackendCall::TurnSteer {
            thread_id: shell.thread_id,
            turn_id: "active".to_string(),
            client_user_message_id: "ide-steer".to_string(),
            prompt: "captured message".to_string(),
        }]
    );
    backend.clear_calls();
    shell.complete_ide_steer(
        &backend,
        steer(&shell, "finished"),
        "captured message".to_string(),
        Ok(context()),
    );
    assert_eq!(
        shell.composer.submission_text(),
        "captured message\n\nnew draft"
    );
    assert!(backend.calls().is_empty());
    assert!(!shell.has_pending_backend_action(ActionGroup::TurnSteer));
}

#[tokio::test]
async fn queued_context_is_captured_once_and_survives_ambiguous_add_retry() {
    let backend = RecordingBackend::default();
    backend
        .queue_add_errors
        .lock()
        .unwrap()
        .push_back("response lost after commit".to_string());
    let mut shell = ShellState::snapshot_fixture();
    shell.ide.enabled = true;
    shell.composer.set_text("queued request");
    assert!(
        shell
            .composer
            .queue_current_message_with_client_id("ide-queue".to_string())
    );
    let input = vec![UserInput::Text {
        text: "queued request".to_string(),
        text_elements: Vec::new(),
    }];
    let mut expected = input.clone();
    crate::ide_context::apply_ide_context_to_user_input(&context(), &mut expected);
    shell.complete_ide_queue(
        &backend,
        shell.thread_id,
        QueueMutation::Add {
            input,
            client_user_message_id: "ide-queue".to_string(),
            attempts: 0,
            capture_ide: false,
        },
        Ok(context()),
    );
    complete_backend_actions(&mut shell, &backend).await;
    let submissions = backend.queued_submissions.lock().unwrap();
    let submissions = submissions.get(&shell.thread_id).unwrap();
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0].input, expected);
    assert_eq!(
        backend
            .calls()
            .iter()
            .filter(|call| matches!(call, RecordedBackendCall::QueueAdd { .. }))
            .count(),
        2
    );
}

#[test]
fn display_hides_only_the_native_prefix_and_preserves_stored_content() {
    let mut input = vec![
        UserInput::Text {
            text: "request".to_string(),
            text_elements: Vec::new(),
        },
        UserInput::LocalImage {
            path: "/tmp/diagram.png".into(),
            detail: None,
        },
    ];
    crate::ide_context::apply_ide_context_to_user_input(&context(), &mut input);
    let stored = input.clone();
    assert_eq!(
        format_user_inputs(&input),
        "request\n[image /tmp/diagram.png]"
    );
    assert_eq!(input, stored);
    assert_eq!(
        crate::ide_context::visible_request("literal\n## My request for Codex:\nkeep this"),
        "literal\n## My request for Codex:\nkeep this"
    );
}
