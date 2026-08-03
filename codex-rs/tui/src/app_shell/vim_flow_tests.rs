use super::LocalSlashCommand;
use super::LocalSlashCommandOutcome;
use super::RecordedBackendCall;
use super::RecordingBackend;
use super::ShellState;
use super::TranscriptKind;
use super::complete_backend_actions;
use super::render_shell;
use super::test_config;
use super::tool_user_input_request_with_auto_resolution;
use super::turn_completed_event;
use crate::app_shell::backend_actions::ActionGroup;
use crate::app_shell::backend_actions::BackendActionResult;
use crate::app_shell::user_input::PendingUserInput;
use crate::app_shell::vim_input::MAX_CONSECUTIVE_APP_SERVER_EVENTS;
use crate::app_shell::vim_input::VimInputOutcome;
use crate::app_shell::vim_input::VimInputRequest;
use crate::app_shell::vim_input::VimInputWaitOutcome;
use crate::app_shell::vim_input::wait_while_processing_events;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::test]
async fn vim_is_an_exact_local_command_and_never_reaches_the_backend() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.set_text("/vim");

    assert_eq!(
        (
            LocalSlashCommand::parse("  /vim  "),
            LocalSlashCommand::parse("/vim extra"),
        ),
        (Some(LocalSlashCommand::Vim), None)
    );

    let should_exit = shell
        .handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .expect("/vim should be handled locally");

    assert!(!should_exit);
    assert_eq!(
        (
            shell.take_vim_input_request(),
            shell.composer.text(),
            backend.calls(),
        ),
        (
            Some(VimInputRequest::empty(shell.thread_id)),
            "",
            Vec::new()
        )
    );
    shell.composer.move_up_or_recall_history();
    assert_eq!(shell.composer.text(), "/vim");

    assert!(shell.start_backend_action(
        ActionGroup::SessionSwitch,
        "resuming session",
        std::future::pending(),
    ));
    shell.request_vim_input();
    assert_eq!(
        (
            shell.take_vim_input_request(),
            shell
                .transcript
                .back()
                .map(|line| (line.kind, line.text.as_str())),
        ),
        (
            None,
            Some((
                TranscriptKind::Status,
                "wait for the pending session switch to finish",
            )),
        )
    );
}

#[tokio::test]
async fn ordinary_vim_exit_returns_the_edited_draft() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.dashboard_visible = false;
    shell.transcript.clear();
    shell.composer.clear();
    let originating_thread_id = shell.thread_id;
    let draft = "Explain this change:\n\n- preserve spacing";

    shell
        .complete_vim_input(
            originating_thread_id,
            Ok(VimInputOutcome::ReturnDraft(draft.to_string())),
            &config,
            &mut backend,
        )
        .await
        .expect("Vim draft should return to the composer");

    assert_eq!(
        (
            shell.composer.text(),
            shell.composer.cursor(),
            backend.calls(),
        ),
        (draft, draft.len(), Vec::new())
    );
    insta::assert_snapshot!(
        "vim_input_returned_draft",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 16,
            ),
        )
    );

    let switched_draft = "Do not send this to the replacement session.";
    shell.thread_id = ThreadId::new();
    shell.composer.clear();
    shell
        .complete_vim_input(
            originating_thread_id,
            Ok(VimInputOutcome::Submit(switched_draft.to_string())),
            &config,
            &mut backend,
        )
        .await
        .expect("cross-session Vim input should return as a draft");
    assert_eq!(
        (
            shell.composer.text(),
            shell
                .transcript
                .back()
                .map(|line| (line.kind, line.text.as_str())),
            backend.calls()
        ),
        (
            switched_draft,
            Some((
                TranscriptKind::Status,
                "session changed; Vim input returned without sending",
            )),
            Vec::new(),
        )
    );
}

#[tokio::test]
async fn vim_submissions_dispatch_local_commands_and_preserve_the_draft() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    let existing_draft = "Keep this composer draft.";
    shell.transcript.clear();
    shell.composer.set_text(existing_draft);

    let clear_outcome = shell
        .complete_vim_input(
            shell.thread_id,
            Ok(VimInputOutcome::Submit("\u{a0}/clear\u{a0}".to_string())),
            &config,
            &mut backend,
        )
        .await
        .expect("Vim /clear should run locally");
    let clear_status = shell
        .transcript
        .back()
        .map(|line| (line.kind, line.text.clone()));
    let exit_outcome = shell
        .complete_vim_input(
            shell.thread_id,
            Ok(VimInputOutcome::Submit("/exit".to_string())),
            &config,
            &mut backend,
        )
        .await
        .expect("Vim /exit should run locally");
    let reopen_outcome = shell
        .complete_vim_input(
            shell.thread_id,
            Ok(VimInputOutcome::Submit("/vim".to_string())),
            &config,
            &mut backend,
        )
        .await
        .expect("Vim /vim should run locally");
    let composer_text = shell.composer.text().to_string();
    let pending_vim_input = shell.take_vim_input_request();
    let thread_id = shell.thread_id;
    let backend_calls = backend.calls();

    assert_eq!(
        (
            clear_outcome,
            clear_status,
            exit_outcome,
            reopen_outcome,
            composer_text,
            pending_vim_input,
            backend_calls,
        ),
        (
            LocalSlashCommandOutcome::Continue,
            Some((
                TranscriptKind::System,
                "visible transcript cleared".to_string(),
            )),
            LocalSlashCommandOutcome::Exit,
            LocalSlashCommandOutcome::Continue,
            existing_draft.to_string(),
            Some(VimInputRequest::empty(thread_id)),
            Vec::new(),
        )
    );
}

#[tokio::test]
async fn vim_submission_starts_an_idle_turn_and_preserves_an_existing_draft() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let backend = RecordingBackend::default();
    let prompt = "  preserve this indentation\n\nand this blank line";
    let existing_draft = "Keep this recovered draft.";
    shell.active_turn_id = None;
    shell.transcript.clear();
    shell.composer.set_text(existing_draft);

    shell
        .complete_vim_input(
            shell.thread_id,
            Ok(VimInputOutcome::Submit(prompt.to_string())),
            &config,
            &mut backend.clone(),
        )
        .await
        .expect("Vim input should start a turn");
    complete_backend_actions(&mut shell, &backend).await;

    assert_eq!(backend.calls(), vec![expected_turn_start(&shell, prompt)]);
    assert_eq!(
        (
            shell.composer.text(),
            shell
                .transcript
                .back()
                .map(|line| (line.kind, line.text.as_str())),
        ),
        (existing_draft, Some((TranscriptKind::User, prompt)))
    );
}

#[tokio::test]
async fn vim_submission_steers_an_active_turn_with_exact_input() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    let prompt = "Use the Vim-edited follow-up.";
    shell.active_turn_id = Some("turn-active".to_string());
    shell.transcript.clear();
    shell.composer.clear();

    shell
        .complete_vim_input(
            shell.thread_id,
            Ok(VimInputOutcome::Submit(prompt.to_string())),
            &config,
            &mut backend,
        )
        .await
        .expect("Vim input should steer the active turn");

    let calls = backend.calls();
    let [
        RecordedBackendCall::TurnSteer {
            client_user_message_id,
            ..
        },
    ] = calls.as_slice()
    else {
        panic!("expected one turn steer call");
    };
    assert_eq!(
        calls,
        vec![RecordedBackendCall::TurnSteer {
            thread_id: shell.thread_id,
            turn_id: "turn-active".to_string(),
            client_user_message_id: client_user_message_id.clone(),
            prompt: prompt.to_string(),
        }]
    );
    assert_eq!(
        (
            shell.composer.text(),
            shell
                .transcript
                .back()
                .map(|line| (line.kind, line.text.as_str())),
        ),
        ("", Some((TranscriptKind::User, prompt)))
    );
}

#[tokio::test]
async fn a_pending_turn_start_completes_before_vim_submission() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
    let mut backend = RecordingBackend {
        turn_start_gate: Some(std::sync::Arc::clone(&gate)),
        ..RecordingBackend::default()
    };
    let first_prompt = "Start the pending turn.";
    let vim_prompt = "Steer it with the Vim input.";
    shell.active_turn_id = None;
    shell.submit_prompt(&backend, first_prompt.to_string());

    let editor = async { Ok(VimInputOutcome::Submit(vim_prompt.to_string())) };
    let outcome = tokio::time::timeout(
        Duration::from_millis(/*millis*/ 50),
        wait_while_processing_events(&mut shell, &mut backend, editor),
    )
    .await
    .expect("Vim should return before the pending turn starts");
    let VimInputWaitOutcome::Completed(result) = outcome else {
        panic!("app-server should remain connected");
    };
    shell
        .complete_vim_input(shell.thread_id, result, &config, &mut backend)
        .await
        .expect("Vim input should queue behind the pending turn");
    assert_eq!(shell.pending_prompt_submission.as_deref(), Some(vim_prompt));

    gate.add_permits(/*permits*/ 1);
    complete_backend_actions(&mut shell, &backend).await;
    shell
        .dispatch_pending_prompt_submission(&mut backend)
        .await
        .expect("queued Vim input should send after the pending turn starts");

    let calls = backend.calls();
    assert_eq!(
        calls.first(),
        Some(&expected_turn_start(&shell, first_prompt))
    );
    assert!(matches!(
        calls.get(1),
        Some(RecordedBackendCall::TurnSteer { prompt, .. }) if prompt == vim_prompt
    ));
}

#[tokio::test]
async fn app_server_disconnect_ends_the_vim_wait() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend {
        disconnect_when_events_empty: true,
        ..RecordingBackend::default()
    };
    let editor = std::future::pending::<color_eyre::Result<VimInputOutcome>>();

    assert!(matches!(
        wait_while_processing_events(&mut shell, &mut backend, editor).await,
        VimInputWaitOutcome::AppServerDisconnected
    ));
}

#[tokio::test]
async fn a_sustained_event_burst_yields_to_a_completed_editor() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.active_turn_id = Some("turn-active".to_string());
    for _ in 0..=MAX_CONSECUTIVE_APP_SERVER_EVENTS {
        backend.push_event(turn_completed_event(
            shell.thread_id,
            "turn-active",
            TurnStatus::Completed,
        ));
    }

    let outcome = wait_while_processing_events(&mut shell, &mut backend, async {
        Ok(VimInputOutcome::Cancelled)
    })
    .await;

    assert!(matches!(
        outcome,
        VimInputWaitOutcome::Completed(Ok(VimInputOutcome::Cancelled))
    ));
    assert_eq!(
        backend
            .events
            .lock()
            .expect("event queue should lock")
            .len(),
        1
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn backend_actions_and_user_input_auto_resolution_run_while_vim_is_open() {
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.pending_user_input = PendingUserInput::from_request(
        &tool_user_input_request_with_auto_resolution(/*auto_resolution_ms*/ 60),
    );
    assert!(shell.start_backend_action(
        ActionGroup::Compaction,
        "starting context compaction",
        async { BackendActionResult::Compaction { result: Ok(()) } },
    ));
    let editor = async {
        tokio::time::sleep(Duration::from_millis(/*millis*/ 170)).await;
        Ok(VimInputOutcome::Cancelled)
    };

    let outcome = wait_while_processing_events(&mut shell, &mut backend, editor).await;

    assert!(matches!(
        outcome,
        VimInputWaitOutcome::Completed(Ok(VimInputOutcome::Cancelled))
    ));
    assert!(shell.pending_user_input.is_none());
    assert!(!shell.has_pending_backend_actions());
    assert_eq!(
        backend
            .resolved_requests
            .lock()
            .expect("resolved requests should lock")
            .clone(),
        vec![(RequestId::Integer(43), json!({ "answers": {} }))]
    );
}

fn expected_turn_start(shell: &ShellState, prompt: &str) -> RecordedBackendCall {
    RecordedBackendCall::TurnStart {
        thread_id: shell.thread_id,
        prompt: prompt.to_string(),
        cwd: PathBuf::from("/workspace/better-codex"),
        model: "gpt-5-codex".to_string(),
        effort: None,
        collaboration_mode: None,
    }
}
