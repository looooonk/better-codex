use super::*;
use pretty_assertions::assert_eq;

#[test]
fn failed_reasoning_update_does_not_change_effort() {
    let mut shell = ShellState::snapshot_fixture();
    let update = SettingsUpdate {
        change: SettingsChange::ReasoningEffort {
            effort: Some(ReasoningEffort::Max),
            thread_effort: Some(ReasoningEffort::Max),
        },
        edit: None,
        selector: None,
    };

    shell.complete_settings_update(update, Err(color_eyre::eyre::eyre!("write failed")));

    assert_eq!(shell.reasoning_effort, None);
}

#[tokio::test]
async fn settings_wait_for_pending_workspace_actions() {
    let mut shell = ShellState::snapshot_fixture();
    shell.start_backend_action(
        ActionGroup::Workspace,
        "pending",
        std::future::pending::<BackendActionResult>(),
    );
    shell.start_settings_update(SettingsChange::Animations(false), async {
        panic!("blocked settings must not be submitted")
    });
    assert!(!shell.has_pending_backend_action(ActionGroup::Settings));
    assert!(shell.has_pending_backend_action(ActionGroup::Workspace));
    shell.dashboard_visible = false;
    let area = ratatui::layout::Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 24,
    );
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    crate::app_shell::render::ShellView { shell: &shell }.render(area, &mut buffer);
    let rendered = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("settings_wait_for_workspace_action", rendered);
}
