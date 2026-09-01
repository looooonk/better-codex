use super::*;
use codex_protocol::config_types::Settings;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

use super::super::WorldState;
use super::super::WorldStateSnapshot;

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
fn instruction_edits_within_the_same_active_mode_restate_the_mode() {
    let before = collaboration_mode_state(ModeKind::Default, Some("old instructions"));
    let after = collaboration_mode_state(ModeKind::Default, Some("new instructions"));
    let before_snapshot = before.snapshot();

    assert_ne!(before_snapshot, after.snapshot());
    assert_eq!(
        after
            .render_diff(PreviousSectionState::Known(&before_snapshot))
            .map(|fragment| fragment.body()),
        Some("new instructions".to_string())
    );
}

#[test]
fn stale_active_fragment_is_revoked_when_current_mode_has_no_instructions() {
    let active = collaboration_mode_state(ModeKind::Plan, Some("make a plan"));
    let inactive = collaboration_mode_state(ModeKind::Plan, None);
    let previous = WorldStateSnapshot {
        sections: BTreeMap::from([(
            CollaborationModeState::ID.to_string(),
            serde_json::to_value(active.snapshot()).expect("serialize active snapshot"),
        )]),
    };
    let stale_fragment =
        ContextualUserFragment::into(CollaborationModeInstructions::new("make a plan"));
    let mut world_state = WorldState::default();
    world_state.add_section(inactive);

    assert_eq!(
        world_state
            .render_history_diff(Some(&previous), &[stale_fragment])
            .into_iter()
            .map(|fragment| fragment.body())
            .collect::<Vec<_>>(),
        vec![String::new()]
    );
}

#[test]
fn legacy_snapshot_without_instruction_hash_restates_visible_instructions() {
    let previous: CollaborationModeSnapshot = serde_json::from_value(serde_json::json!({
        "mode": "default",
        "instructions_visible": true,
    }))
    .expect("legacy collaboration-mode snapshot should deserialize");
    let current = collaboration_mode_state(ModeKind::Default, Some("current instructions"));

    assert_eq!(previous.instructions_hash, None);
    assert_eq!(
        current
            .render_diff(PreviousSectionState::Known(&previous))
            .map(|fragment| fragment.body()),
        Some("current instructions".to_string())
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
