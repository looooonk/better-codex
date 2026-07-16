use super::*;
use pretty_assertions::assert_eq;

#[test]
fn settings_tabs_fill_the_strip_and_select_the_clicked_page() {
    let mut settings = SettingsState::default();
    settings.move_down();

    assert!(!settings.select_at(/*line*/ 0, /*column*/ 0, /*width*/ 49));
    assert_eq!(settings.selected_action(), SettingsAction::ReasoningEffort);

    assert!(!settings.select_at(/*line*/ 0, /*column*/ 8, /*width*/ 49));
    assert_eq!(settings.page, SettingsPage::Permissions);
    assert!(!settings.select_at(/*line*/ 1, /*column*/ 22, /*width*/ 49));
    assert_eq!(settings.page, SettingsPage::Appearance);
    assert!(!settings.select_at(/*line*/ 0, /*column*/ 35, /*width*/ 49));
    assert_eq!(settings.page, SettingsPage::Integrations);
    assert!(!settings.select_at(/*line*/ 0, /*column*/ 49, /*width*/ 49));
    assert_eq!(settings.page, SettingsPage::Integrations);
}

#[test]
fn settings_tabs_have_transparent_backgrounds() {
    let backgrounds = SettingsTabs::new(/*width*/ 49)
        .lines(SettingsPage::Model)
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.style.bg)
        .collect::<Vec<_>>();
    let expected = vec![None; backgrounds.len()];

    assert_eq!(backgrounds, expected);
}

#[test]
fn action_rows_select_only_the_clicked_action() {
    let mut settings = SettingsState::default();
    let line = settings.action_line_start().saturating_add(2);

    assert!(settings.select_at(line, /*column*/ 0, /*width*/ 49));
    assert_eq!(settings.selected_action(), SettingsAction::ServiceTier);
}
