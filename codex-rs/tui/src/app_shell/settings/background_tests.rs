use super::*;
use crate::app_shell::reasoning_aura::ReasoningAura;
use crate::app_shell::reasoning_aura::ReasoningAuraTone;
use pretty_assertions::assert_eq;

#[test]
fn successful_reasoning_update_activates_requested_aura() {
    for (tone, effort) in [
        (ReasoningAuraTone::Max, ReasoningEffort::Max),
        (ReasoningAuraTone::Ultra, ReasoningEffort::Ultra),
    ] {
        let mut shell = ShellState::snapshot_fixture();
        let update = SettingsUpdate {
            change: SettingsChange::ReasoningEffort {
                effort: Some(effort.clone()),
                aura_tone: Some(tone),
                thread_effort: Some(effort.clone()),
            },
            edit: None,
            selector: None,
        };

        shell.complete_settings_update(update, Ok(()));

        assert_eq!(
            (
                shell.reasoning_effort.clone(),
                shell.reasoning_aura.is_some(),
            ),
            (Some(effort), true),
        );
    }
}

#[test]
fn model_effort_update_activates_and_clears_aura() {
    let mut shell = ShellState::snapshot_fixture();
    shell.reasoning_effort = Some(ReasoningEffort::High);
    let update = SettingsUpdate {
        change: SettingsChange::Model {
            model: "max-model".to_string(),
            effort: Some(ReasoningEffort::Max),
            aura_tone: Some(ReasoningAuraTone::Max),
            service_tier: None,
        },
        edit: None,
        selector: None,
    };
    shell.complete_settings_update(update, Ok(()));
    let max_aura_active = shell.reasoning_aura.is_some();

    let update = SettingsUpdate {
        change: SettingsChange::Model {
            model: "high-model".to_string(),
            effort: Some(ReasoningEffort::High),
            aura_tone: None,
            service_tier: None,
        },
        edit: None,
        selector: None,
    };
    shell.complete_settings_update(update, Ok(()));

    assert_eq!(
        (
            max_aura_active,
            shell.model,
            shell.reasoning_effort,
            shell.reasoning_aura.is_none(),
        ),
        (
            true,
            "high-model".to_string(),
            Some(ReasoningEffort::High),
            true,
        ),
    );
}

#[test]
fn failed_reasoning_update_does_not_activate_aura() {
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
        (shell.reasoning_effort, shell.reasoning_aura.is_some()),
        (None, false),
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
    let suppressed = shell.reasoning_aura.is_none();

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

    assert_eq!((suppressed, shell.reasoning_aura.is_none()), (true, true));
}
