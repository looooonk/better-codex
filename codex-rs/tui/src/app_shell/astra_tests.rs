use super::*;

#[test]
fn astra_catalog_drives_model_reasoning_and_fast_selectors() {
    let mut shell = ShellState::snapshot_fixture();
    shell.model = "gpt-6-astra".to_string();
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
        ThreadItem::AgentMessage {
            id: "question-1".to_string(),
            text: "Which approach?\n- Small change\n- Full rewrite".to_string(),
            phase: Some(codex_protocol::models::MessagePhase::Commentary),
            memory_citation: None,
        },
        CompletedItemOrigin::Live,
    );
    insta::assert_snapshot!(
        "astra_async_question",
        render_shell(&shell, Rect::new(0, 0, 100, 30))
    );
}
