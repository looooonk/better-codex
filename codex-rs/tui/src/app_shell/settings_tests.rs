use super::*;
use pretty_assertions::assert_eq;

#[test]
fn headings_and_tab_gaps_do_not_activate_settings() {
    let mut settings = SettingsState::default();
    settings.move_down();

    assert!(!settings.select_at(/*line*/ 0, /*column*/ 0));
    assert_eq!(settings.selected_action(), SettingsAction::ReasoningEffort);

    assert!(!settings.select_at(/*line*/ 1, /*column*/ 21));
    assert_eq!(settings.page, SettingsPage::Permissions);
    assert!(!settings.select_at(/*line*/ 1, /*column*/ 7));
    assert_eq!(settings.page, SettingsPage::Permissions);

    assert!(!settings.select_at(/*line*/ 2, /*column*/ 11));
    assert_eq!(settings.page, SettingsPage::Appearance);
    assert!(!settings.select_at(/*line*/ 2, /*column*/ 13));
    assert_eq!(settings.page, SettingsPage::Appearance);
    assert!(!settings.select_at(/*line*/ 2, /*column*/ 27));
    assert_eq!(settings.page, SettingsPage::Integrations);
}

#[test]
fn action_rows_select_only_the_clicked_action() {
    let mut settings = SettingsState::default();
    let line = settings.action_line_start().saturating_add(2);

    assert!(settings.select_at(line, /*column*/ 0));
    assert_eq!(settings.selected_action(), SettingsAction::ServiceTier);
}
