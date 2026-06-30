use super::*;
use crate::legacy_core::config::Config;
use crate::model_migration::ModelMigrationCopy;
use codex_protocol::openai_models::ReasoningEffort;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Stylize;
use ratatui::text::Line;

#[test]
fn model_migration_selection_keys_move_between_choices() {
    let mut state = ModelMigrationOnboardingState::new(model_migration_prompt_data_fixture(
        /*can_opt_out*/ true,
    ));

    assert_eq!(
        handle_model_migration_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &mut state),
        ModelMigrationKeyAction::Redraw
    );
    assert_eq!(state.selected(), ModelMigrationSelection::KeepCurrentModel);

    assert_eq!(
        handle_model_migration_key(
            KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
            &mut state
        ),
        ModelMigrationKeyAction::Redraw
    );
    assert_eq!(state.selected(), ModelMigrationSelection::Exit);
}

#[test]
fn model_migration_view_renders_native_choices() {
    let state = ModelMigrationOnboardingState::new(model_migration_prompt_data_fixture(
        /*can_opt_out*/ true,
    ));
    let backend = TestBackend::new(/*width*/ 100, /*height*/ 28);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    terminal
        .draw(|frame| {
            ModelMigrationOnboardingView { state: &state }.render(frame.area(), frame.buffer_mut());
        })
        .expect("draw model migration onboarding");
    insta::assert_snapshot!(terminal.backend().to_string());
}

#[tokio::test]
async fn model_migration_prompt_data_respects_seen_decision() {
    let codex_home = tempfile::tempdir().expect("create temp codex home");
    let mut config = Config::load_default_with_cli_overrides_for_codex_home(
        codex_home.path().to_path_buf(),
        Vec::new(),
    )
    .await
    .expect("load test config");
    let available_models = crate::test_support::TEST_MODEL_PRESETS.clone();
    let current = available_models
        .iter()
        .find(|preset| preset.model == "gpt-5.3-codex")
        .expect("current preset present");
    let upgrade = current.upgrade.as_ref().expect("upgrade configured");

    let prompt = model_migration_prompt_data(&config, &current.model, &available_models)
        .expect("migration prompt should be eligible");
    assert_eq!(prompt.target_model, upgrade.id);

    config
        .notices
        .model_migrations
        .insert(current.model.clone(), upgrade.id.clone());
    assert!(
        model_migration_prompt_data(&config, &current.model, &available_models).is_none(),
        "seen migrations should not prompt again"
    );
}

fn model_migration_prompt_data_fixture(can_opt_out: bool) -> ModelMigrationPromptData {
    ModelMigrationPromptData {
        from_model: "gpt-5.1-codex-max".to_string(),
        target_model: "gpt-5.2-codex-max".to_string(),
        target_default_effort: ReasoningEffort::High,
        target_display_name: "GPT-5.2 Codex Max".to_string(),
        copy: ModelMigrationCopy {
            heading: vec!["Codex just got an upgrade.".bold()],
            content: vec![
                Line::from("We recommend switching to the newer Codex model."),
                Line::from(""),
                Line::from("You can continue using the current model if you prefer."),
            ],
            can_opt_out,
            markdown: None,
        },
    }
}
