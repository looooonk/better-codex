use super::*;
use crate::app_shell::reasoning_ripple::ReasoningRipple;
use crate::app_shell::reasoning_ripple::ReasoningRippleTone;
use pretty_assertions::assert_eq;

#[test]
fn failed_reasoning_update_does_not_change_effort_or_start_ripple() {
    let mut shell = ShellState::snapshot_fixture();
    let update = SettingsUpdate {
        change: SettingsChange::ReasoningEffort {
            effort: Some(ReasoningEffort::Max),
            ripple_tone: Some(ReasoningRippleTone::Max),
            thread_effort: Some(ReasoningEffort::Max),
        },
        edit: None,
        selector: None,
    };

    shell.complete_settings_update(update, Err(color_eyre::eyre::eyre!("write failed")));

    assert_eq!(
        (shell.reasoning_effort, shell.reasoning_ripple.is_none()),
        (None, true),
    );
}

#[test]
fn disabled_animations_suppress_and_clear_reasoning_ripple() {
    let mut shell = ShellState::snapshot_fixture();
    shell.animations = false;
    let update = SettingsUpdate {
        change: SettingsChange::ReasoningEffort {
            effort: Some(ReasoningEffort::Ultra),
            ripple_tone: Some(ReasoningRippleTone::Ultra),
            thread_effort: Some(ReasoningEffort::Ultra),
        },
        edit: None,
        selector: None,
    };
    shell.complete_settings_update(update, Ok(()));
    let suppressed_state = (
        shell.reasoning_effort.clone(),
        shell.reasoning_ripple.is_none(),
    );

    shell.animations = true;
    shell.reasoning_ripple = Some(ReasoningRipple::new(
        ReasoningRippleTone::Max,
        std::time::Instant::now(),
    ));
    let update = SettingsUpdate {
        change: SettingsChange::Animations(false),
        edit: None,
        selector: None,
    };
    shell.complete_settings_update(update, Ok(()));

    assert_eq!(
        (
            suppressed_state,
            shell.animations,
            shell.reasoning_ripple.is_none(),
        ),
        ((Some(ReasoningEffort::Ultra), true), false, true),
    );
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
