use super::*;
use codex_app_server_protocol::AsyncUserInputQuestion;
use pretty_assertions::assert_eq;

fn question_item() -> ThreadItem {
    ThreadItem::untimed(ThreadItemPayload::AgentMessage {
        id: "audience-and-note".into(),
        text: "Who is it for?\n- Team\n- Club\n\nWhat note?".into(),
        questions: Some(vec![
            AsyncUserInputQuestion {
                title: "Who is it for?".into(),
                options: Some(vec!["Team".into(), "Club".into()]),
            },
            AsyncUserInputQuestion {
                title: "What note?".into(),
                options: None,
            },
        ]),
        delivery: Some(codex_protocol::items::AgentMessageDelivery::Async),
        phase: None,
        memory_citation: None,
    })
}

#[test]
fn footer_hover_and_click_targets_follow_the_labels() {
    use question_popup::QuestionHit;
    use question_popup::QuestionPopup;
    use ratatui::buffer::Buffer;

    let ThreadItemPayload::AgentMessage {
        id,
        questions: Some(questions),
        ..
    } = question_item().payload
    else {
        unreachable!()
    };
    let mut pending = PendingUserInput::from_async(&id, questions);
    assert_eq!(pending.answer_current(), Ok(UserInputAdvance::Next));
    let area = Rect::new(0, 0, 80, 24);
    let popup = QuestionPopup::new(&pending, area);
    let body = request_popup::body(popup.panel);
    let y = body.bottom() - 1;
    let footer = "Previous  Submit";
    let mut targets = vec![None; usize::from(body.width)];
    for (label, action) in [
        ("Previous", QuestionHit::Back),
        ("Submit", QuestionHit::Submit),
    ] {
        let start = footer.find(label).unwrap();
        targets[start..start + label.len()].fill(Some(action));
    }
    let mut buf = Buffer::empty(area);
    for (x, &expected) in (body.x..body.right()).zip(&targets) {
        let pos = Position::new(x, y);
        assert_eq!(popup.hit(pos), expected);
        popup.render(&pending, Some(pos), &mut buf);
        for (offset, ch) in footer.char_indices() {
            let cell = &buf[(body.x + offset as u16, y)];
            assert_eq!(cell.symbol(), ch.to_string());
            let highlighted = expected.is_some() && targets[offset] == expected;
            assert_eq!(cell.bg == design::palette::border(), highlighted);
        }
    }
}

#[tokio::test]
async fn async_bundle_preserves_drafts_and_submits_once_after_a_retry() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.composer.set_text("unsent message");
    shell.composer.set_selection(0, 6);
    let original = shell.composer.clone();
    let mut backend = RecordingBackend::default();
    let config = test_config().await;
    let area = Rect::new(0, 0, 80, 24);
    for _ in 0..2 {
        shell.ingest_completed_item(question_item(), CompletedItemOrigin::Live);
    }
    assert!(shell.queued_interactive_requests.is_empty());
    let rendered = render_shell(&shell, area);
    assert_eq!(rendered.matches("Who is it for?").count(), 2); // Transcript and popup.
    let club = rendered_text_position(&rendered, "○ Club");
    shell
        .handle_mouse_selection_down(area, club, &config, &mut backend)
        .await
        .unwrap();
    shell.insert_pasted_text("first\r\nwrong");
    shell
        .handle_key_in_area(
            area,
            KeyEvent::new(KeyCode::Home, KeyModifiers::SHIFT),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    shell.insert_pasted_text("second");
    for code in [KeyCode::F(2), KeyCode::Enter] {
        shell
            .handle_key_in_area(
                area,
                KeyEvent::new(code, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .unwrap();
    }
    assert_eq!(shell.focused_editor().text(), "first\nsecond");
    insta::assert_snapshot!("bundled_question_editor", render_shell(&shell, area));
    assert!(backend.calls().is_empty());
    shell.clear_active_turn();
    backend.fail_next_turn_start("disconnected");
    shell
        .resolve_pending_user_input(&mut backend)
        .await
        .unwrap();
    assert!(render_shell(&shell, area).contains("disconnected"));
    assert_eq!(shell.composer, original);
    shell
        .resolve_pending_user_input(&mut backend)
        .await
        .unwrap();
    assert!(shell.pending_user_input.is_none());
    assert_eq!(shell.composer, original);
    assert_eq!(backend.calls().len(), 2);
    assert!(
        matches!(&backend.calls()[1], RecordedBackendCall::TurnStart { prompt, .. } if prompt == "Who is it for?\nClub\nWhat note?\nfirst\nsecond")
    );
}

#[test]
fn custom_numeric_replies_keep_the_async_question_identity() {
    let ThreadItemPayload::AgentMessage {
        id,
        questions: Some(questions),
        ..
    } = question_item().payload
    else {
        unreachable!()
    };
    let mut pending = PendingUserInput::from_async(&id, questions);
    pending.select(2);
    pending.draft_mut().editor.set_text("1");
    assert_eq!(pending.answer_current(), Ok(UserInputAdvance::Next));
    pending.draft_mut().editor.set_text("note");
    let UserInputAdvance::AsyncReply(items) = pending.answer_current().unwrap() else {
        panic!("expected async reply")
    };
    use codex_context_fragments::AnsweredQuestion;
    use codex_context_fragments::ContextualUserFragment;
    let (start, end) = AnsweredQuestion::type_markers();
    let replies: Vec<serde_json::Value> = items
        .iter()
        .map(|item| {
            let UserInput::Text { text, .. } = item else {
                panic!("expected text")
            };
            serde_json::from_str(text.strip_prefix(start).unwrap().strip_suffix(end).unwrap())
                .unwrap()
        })
        .collect();
    assert_eq!(
        replies,
        vec![
            json!([{"questionItemId": "[\"request_user_input_async\",\"audience-and-note\",0]", "question":"Who is it for?", "answer":"1"}]),
            json!([{"questionItemId": "[\"request_user_input_async\",\"audience-and-note\",1]", "question":"What note?", "answer":"note"}]),
        ]
    );
}

#[tokio::test]
async fn external_editor_returns_text_only_to_the_originating_question() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.ingest_completed_item(question_item(), CompletedItemOrigin::Live);
    shell.insert_pasted_text("Custom audience");
    shell
        .handle_key(
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    let request = shell.take_vim_input_request().unwrap();
    let (source, position) = request.question.unwrap();
    shell
        .complete_question_editor(
            shell.thread_id,
            source.clone(),
            position,
            Ok(vim_input::VimInputOutcome::Submit("Editors".into())),
            &mut backend,
        )
        .await
        .unwrap();
    shell
        .complete_question_editor(
            shell.thread_id,
            source,
            position,
            Ok(vim_input::VimInputOutcome::ReturnDraft("Old text".into())),
            &mut backend,
        )
        .await
        .unwrap();
    assert_eq!(
        shell
            .pending_user_input
            .as_ref()
            .unwrap()
            .question_position(),
        (2, 2)
    );
    assert!(shell.focused_editor().is_empty());
    assert!(backend.calls().is_empty());
}
