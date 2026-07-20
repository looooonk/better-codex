use super::*;
use codex_protocol::config_types::Settings;
use pretty_assertions::assert_eq;

#[test]
fn mode_changes_emit_instructions_and_instruction_removal_emits_a_revocation() {
    let default = collaboration_mode_state(ModeKind::Default, Some("pair with the user"));
    let plan = collaboration_mode_state(ModeKind::Plan, Some("make a plan"));
    let disabled_plan = collaboration_mode_state(ModeKind::Plan, None);
    let default_snapshot = default.snapshot();
    let plan_snapshot = plan.snapshot();

    assert_eq!(
        default
            .render_diff(PreviousSectionState::Absent)
            .map(|fragment| fragment.body()),
        Some("pair with the user".to_string()),
    );
    assert_eq!(
        plan.render_diff(PreviousSectionState::Known(&default_snapshot))
            .map(|fragment| fragment.body()),
        Some("make a plan".to_string()),
    );
    assert_eq!(
        disabled_plan
            .render_diff(PreviousSectionState::Known(&plan_snapshot))
            .map(|fragment| fragment.body()),
        Some(String::new()),
    );
}

#[test]
fn instruction_edits_within_the_same_active_mode_do_not_restate_the_mode() {
    let before = collaboration_mode_state(ModeKind::Default, Some("old instructions"));
    let after = collaboration_mode_state(ModeKind::Default, Some("new instructions"));
    let before_snapshot = before.snapshot();

    assert_eq!(before_snapshot, after.snapshot());
    assert!(
        after
            .render_diff(PreviousSectionState::Known(&before_snapshot))
            .is_none()
    );
}

fn collaboration_mode_state(
    mode: ModeKind,
    developer_instructions: Option<&str>,
) -> CollaborationModeState {
    CollaborationModeState::from_collaboration_mode(&CollaborationMode {
        mode,
        settings: Settings {
            model: "test-model".to_string(),
            reasoning_effort: None,
            developer_instructions: developer_instructions.map(str::to_string),
        },
    })
}
