use super::*;
use crate::app_shell::attachments::ImageAttachment;
use pretty_assertions::assert_eq;

fn attach(shell: &mut ShellState) {
    shell.composer.attach_images(vec![ImageAttachment {
        label: "screenshot.png".to_string(),
        url: "data:image/png;base64,cGl4ZWxz".into(),
    }]);
}

#[tokio::test]
async fn image_only_submission_reaches_the_backend_and_retry_keeps_images() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = None;
    shell.dashboard_visible = false;
    shell.composer.clear();
    attach(&mut shell);
    let mut backend = RecordingBackend::default();
    backend.fail_next_turn_start("try again");
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(
        shell.composer.submission_items(""),
        vec![ApiUserInput::Image {
            image: codex_app_server_protocol::ImageReference::Inline {
                url: "data:image/png;base64,cGl4ZWxz".to_string()
            },
            detail: None
        }]
    );
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    assert!(!shell.composer.has_images());
    assert!(backend.calls().iter().any(
        |call| matches!(call, RecordedBackendCall::TurnStart { prompt, .. } if prompt == "[image]")
    ));
}

#[tokio::test]
async fn queue_persists_structured_image_payload_and_session_switch_preserves_draft() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.set_text("explain this");
    attach(&mut shell);
    let expected = shell.composer.submission_items(shell.composer.text());
    let backend = RecordingBackend::default();
    shell.queue_current_message(&backend);
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(
        backend.queued_submissions.lock().unwrap()[&shell.thread_id][0].input,
        expected
    );
    shell.composer.set_text("next image");
    attach(&mut shell);
    let next = shell.composer.submission_items(shell.composer.text());
    shell.replace_started_session(started_thread(
        "next",
        test_thread_id("01900000-0000-7000-8000-000000000203"),
        None,
    ));
    assert_eq!(shell.composer.submission_items(shell.composer.text()), next);
    insta::assert_snapshot!(
        "image_attachment_composer",
        render_shell(&shell, Rect::new(0, 0, 110, 24))
    );
}

#[tokio::test]
async fn local_slash_commands_keep_attached_images_until_explicit_detach() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    attach(&mut shell);
    let expected = shell.composer.submission_items("");
    shell
        .run_local_slash_command(
            LocalSlashCommand::parse("/model").unwrap(),
            "/model".to_string(),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    assert_eq!(shell.composer.submission_items(""), expected);
    shell
        .run_local_slash_command(
            LocalSlashCommand::parse("/detach 1").unwrap(),
            "/detach 1".to_string(),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    assert!(!shell.composer.has_images());
}

#[tokio::test]
async fn returning_from_side_keeps_both_parent_and_child_image_drafts() {
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = None;
    shell.composer.set_text("parent draft");
    attach(&mut shell);
    let mut backend = RecordingBackend::default();
    let parent_id = shell.thread_id;
    shell.complete_side_fork(
        &backend,
        parent_id,
        String::new(),
        Ok(started_thread(
            "Side",
            test_thread_id("11111111-1111-4111-8111-111111111111"),
            Some(parent_id),
        )),
    );
    shell.composer.set_text("side draft");
    attach(&mut shell);
    shell.return_from_side(&mut backend).await.unwrap();
    assert_eq!(
        (
            shell.thread_id,
            shell.composer.text(),
            shell.composer.image_count()
        ),
        (parent_id, "side draft\n\nparent draft", 2)
    );
}

#[test]
fn session_switch_keeps_an_image_draft_hidden_by_a_queued_message_edit() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.set_text("queued message");
    assert!(shell.composer.queue_current_message());
    shell.composer.set_text("image draft");
    attach(&mut shell);
    let expected = shell.composer.submission_items(shell.composer.text());
    assert!(shell.composer.edit_previous_queued_message());
    shell.replace_started_session(started_thread(
        "next",
        test_thread_id("01900000-0000-7000-8000-000000000203"),
        None,
    ));
    assert_eq!(
        shell.composer.submission_items(shell.composer.text()),
        expected
    );
}
