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
fn settings_tabs_center_labels() {
    let tabs = SettingsTabs::new(/*width*/ 77);
    let lines = tabs.lines(SettingsPage::Model);
    let labels = lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(
        labels,
        "    Model     │    Permissions     │    Appearance     │    Integrations     "
    );
    assert_eq!(labels.chars().count(), 77);
}

#[test]
fn settings_tab_ranges_exclude_delimiters() {
    let tabs = SettingsTabs::new(/*width*/ 49);

    assert_eq!(tabs.column_range(SettingsPage::Model), Some(0..7));
    assert_eq!(tabs.column_range(SettingsPage::Permissions), Some(8..21));
    assert_eq!(tabs.column_range(SettingsPage::Appearance), Some(22..34));
    assert_eq!(tabs.column_range(SettingsPage::Integrations), Some(35..49));
    assert_eq!(tabs.page_at(/*column*/ 7), None);
    assert_eq!(tabs.page_at(/*column*/ 21), None);
    assert_eq!(tabs.page_at(/*column*/ 34), None);
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

#[test]
fn settings_pages_have_a_consistent_height() {
    let view = SettingsView {
        model: "gpt-5-codex".to_string(),
        reasoning_effort: None,
        service_tier: None,
        approval_policy: AskForApproval::OnRequest,
        theme: None,
        animations: true,
        show_tooltips: true,
        mcp_inventory: McpInventorySummary::default(),
        plugin_inventory: PluginInventorySummary::default(),
    };
    let mut settings = SettingsState::default();
    let mut line_counts = Vec::new();
    let mut rendered_pages = Vec::new();
    for page in SettingsPage::ALL {
        settings.set_page(page);
        let lines = settings.lines(&view, /*width*/ 49);
        line_counts.push(lines.len());
        rendered_pages.push(format!(
            "{}\n{}",
            page.label(),
            lines
                .iter()
                .map(|line| line
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_string())
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    assert_eq!(
        line_counts,
        vec![SETTINGS_PAGE_LINE_COUNT; SettingsPage::ALL.len()]
    );
    insta::assert_snapshot!(rendered_pages.join("\n\n"));
}
