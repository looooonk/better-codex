use super::*;
use crate::app_shell::reasoning_aura::ReasoningAura;
use crate::app_shell::reasoning_aura::ReasoningAuraTone;
use pretty_assertions::assert_eq;

#[test]
fn failed_reasoning_update_does_not_change_effort_or_activate_aura() {
    let mut shell = ShellState::snapshot_fixture();
    let update = SettingsUpdate {
        change: SettingsChange::ReasoningEffort {
            effort: Some(ReasoningEffort::Max),
            aura_tone: Some(ReasoningAuraTone::Max),
            thread_effort: Some(ReasoningEffort::Max),
        },
        edit: None,
        selector: None,
    };

    shell.complete_settings_update(update, Err(color_eyre::eyre::eyre!("write failed")));

    assert_eq!(
        (shell.reasoning_effort, shell.reasoning_aura.is_none()),
        (None, true),
    );
}

#[test]
fn disabled_animations_suppress_and_clear_reasoning_aura() {
    let mut shell = ShellState::snapshot_fixture();
    shell.animations = false;
    let update = SettingsUpdate {
        change: SettingsChange::ReasoningEffort {
            effort: Some(ReasoningEffort::Ultra),
            aura_tone: Some(ReasoningAuraTone::Ultra),
            thread_effort: Some(ReasoningEffort::Ultra),
        },
        edit: None,
        selector: None,
    };
    shell.complete_settings_update(update, Ok(()));
    let suppressed_state = (
        shell.reasoning_effort.clone(),
        shell.reasoning_aura.is_none(),
    );

    shell.animations = true;
    shell.reasoning_aura = Some(ReasoningAura::new(
        ReasoningAuraTone::Max,
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
            shell.reasoning_aura.is_none(),
        ),
        ((Some(ReasoningEffort::Ultra), true), false, true),
    );
}
