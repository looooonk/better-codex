use super::*;
use pretty_assertions::assert_eq;

#[test]
fn hit_testing_skips_the_editor_and_feedback_before_later_actions() {
    let mut settings = SettingsState::default();
    settings.start_theme_edit("missing".to_owned());
    settings.set_error("Unknown theme");
    let start = *settings.selected_line_range().start();

    assert_eq!(
        (start..=start + 3)
            .map(|line| settings.action_at_line(line))
            .collect::<Vec<_>>(),
        vec![
            Some(SettingsAction::Theme),
            None,
            None,
            Some(SettingsAction::Animations)
        ]
    );
}

#[test]
fn enabling_without_a_compatible_terminal_never_persists_the_experiment() {
    let mut shell = ShellState::snapshot_fixture();
    let home = tempfile::tempdir().unwrap();
    shell.codex_home = home.path().into();
    shell.toggle_graphical_math();
    assert!(!shell.graphics.borrow().enabled());
    assert!(!shell.equation_preference().exists());
    assert!(matches!(
        shell.settings.feedback,
        Some(SettingsFeedback::Error(_))
    ));
}
