use super::*;
use crate::app_shell::composer::ComposerState;
use codex_app_server_protocol::ImageReference;
use codex_app_server_protocol::UserInput;
use pretty_assertions::assert_eq;

fn attachment(label: &str) -> ImageAttachment {
    ImageAttachment {
        label: label.to_string(),
        url: format!("data:image/png;base64,{label}").into(),
    }
}

#[test]
fn structured_images_survive_failed_submissions_and_queue_draft_navigation() {
    let mut composer = ComposerState::default();
    composer.set_text("queued text");
    assert!(composer.queue_current_message());
    composer.set_text("describe this");
    composer.attach_images(vec![attachment("first")]);
    let inputs = composer.submission_items(composer.text());
    assert_eq!(
        inputs,
        vec![
            UserInput::Text {
                text: "describe this".to_string(),
                text_elements: Vec::new()
            },
            UserInput::Image {
                image: ImageReference::Inline {
                    url: "data:image/png;base64,first".to_string()
                },
                detail: None
            },
        ]
    );
    assert!(composer.edit_previous_queued_message());
    assert!(!composer.has_images());
    composer.finish_queued_message_edit();
    assert_eq!(composer.submission_items(composer.text()), inputs);
    composer.clear();
    composer.set_text("new draft");
    composer.attach_images(vec![attachment("second")]);
    composer.restore_failed_submission("describe this");
    composer.restore_input_images(&inputs);
    assert_eq!(
        composer.submission_items(composer.text()),
        vec![
            UserInput::Text {
                text: "describe this\n\nnew draft".to_string(),
                text_elements: Vec::new()
            },
            inputs[1].clone(),
            UserInput::Image {
                image: ImageReference::Inline {
                    url: "data:image/png;base64,second".to_string()
                },
                detail: None
            },
        ]
    );
}

#[test]
fn failed_image_only_queue_restores_the_original_input() {
    let mut composer = ComposerState::default();
    composer.attach_images(vec![attachment("first")]);
    let expected = composer.submission_items("");
    assert!(composer.queue_current_message_with_client_id("image-only".to_string()));
    assert!(!composer.edit_previous_queued_message());
    let (text, selected, images) = composer
        .remove_queued_submission_for_client("image-only")
        .unwrap();
    assert!(!selected);
    composer.restore_failed_queued_submission(&text);
    composer.restore_queued_images(images);
    assert_eq!(composer.submission_items(composer.text()), expected);
}

#[tokio::test]
async fn image_loading_is_atomic_and_does_not_depend_on_server_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.png");
    image::RgbaImage::new(2, 2).save(&path).unwrap();
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    assert!(
        shell
            .attach_image_paths(vec![path.clone(), dir.path().join("missing.png")])
            .await
            .is_err()
    );
    assert!(!shell.composer.has_images());
    shell.attach_image_paths(vec![path.clone()]).await.unwrap();
    std::fs::remove_file(path).unwrap();
    let items = shell.composer.submission_items("");
    let [
        UserInput::Image {
            image: ImageReference::Inline { url },
            detail: None,
        },
    ] = items.as_slice()
    else {
        panic!("expected an inline image")
    };
    assert!(url.starts_with("data:image/png;base64,"));
    shell.run_detach_command("1").unwrap();
    assert!(shell.composer.is_empty());
}

#[test]
fn a_failed_turn_recovers_into_the_saved_draft_during_a_queue_edit() {
    let mut composer = ComposerState::default();
    composer.set_text("queued message");
    assert!(composer.queue_current_message());
    composer.attach_images(vec![attachment("failed")]);
    let failed = composer.submission_items("failed turn");
    composer.clear();
    composer.set_text("new draft");
    composer.attach_images(vec![attachment("draft")]);
    assert!(composer.edit_previous_queued_message());
    composer.restore_failed_submission("failed turn");
    composer.restore_input_images(&failed);
    assert_eq!(
        composer.submission_items(composer.text()),
        vec![UserInput::Text {
            text: "queued message".to_string(),
            text_elements: Vec::new()
        }]
    );
    composer.finish_queued_message_edit();
    assert_eq!(
        composer.submission_items(composer.text()),
        vec![
            UserInput::Text {
                text: "failed turn\n\nnew draft".to_string(),
                text_elements: Vec::new()
            },
            failed[1].clone(),
            UserInput::Image {
                image: ImageReference::Inline {
                    url: "data:image/png;base64,draft".to_string()
                },
                detail: None
            },
        ]
    );
}
