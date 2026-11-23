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
