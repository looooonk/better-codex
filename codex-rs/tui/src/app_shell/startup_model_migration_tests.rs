use super::*;
use crate::legacy_core::config::Config;
use crate::model_migration::migration_copy_for_models;
use codex_protocol::openai_models::ModelUpgrade;
use codex_protocol::openai_models::ReasoningEffort;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

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
    let mut available_models = crate::test_support::TEST_MODEL_PRESETS.clone();
    let current_model = "gpt-5.4";
    let target_model = "gpt-5.5";
    available_models
        .iter_mut()
        .find(|preset| preset.model == current_model)
        .expect("current preset present")
        .upgrade = Some(ModelUpgrade {
        id: target_model.to_string(),
        migration_config_key: "hide_test_migration_prompt".to_string(),
        model_link: None,
        upgrade_copy: None,
        migration_markdown: None,
    });

    let prompt = model_migration_prompt_data(&config, current_model, &available_models)
        .expect("migration prompt should be eligible");
    assert_eq!(prompt.target_model, target_model);

    config
        .notices
        .model_migrations
        .insert(current_model.to_string(), target_model.to_string());
    assert!(
        model_migration_prompt_data(&config, current_model, &available_models).is_none(),
        "seen migrations should not prompt again"
    );
}

fn model_migration_prompt_data_fixture(can_opt_out: bool) -> ModelMigrationPromptData {
    ModelMigrationPromptData {
        from_model: "gpt-5.1-codex-max".to_string(),
        target_model: "gpt-5.2-codex-max".to_string(),
        target_default_effort: ReasoningEffort::High,
        target_display_name: "GPT-5.2 Codex Max".to_string(),
        copy: migration_copy_for_models(
            "gpt-5.1-codex-max",
            "gpt-5.2-codex-max",
            /*model_link*/ None,
            /*migration_copy*/ None,
            /*migration_markdown*/ None,
            "GPT-5.2 Codex Max".to_string(),
            Some("The newer Codex model is recommended.".to_string()),
            can_opt_out,
        ),
    }
}
