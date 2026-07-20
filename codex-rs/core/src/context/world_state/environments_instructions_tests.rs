use super::*;
use pretty_assertions::assert_eq;

#[test]
fn guidance_is_emitted_only_when_it_becomes_enabled() {
    let disabled = EnvironmentsInstructionsState::new(/*enabled*/ false);
    let enabled = EnvironmentsInstructionsState::new(/*enabled*/ true);
    let previously_disabled = disabled.snapshot();
    let previously_enabled = enabled.snapshot();

    assert!(disabled.render_diff(PreviousSectionState::Absent).is_none());
    assert_eq!(
        enabled
            .render_diff(PreviousSectionState::Known(&previously_disabled))
            .map(|fragment| (fragment.role(), fragment.body())),
        Some(("developer", EnvironmentsInstructions.body(),)),
    );
    assert!(
        enabled
            .render_diff(PreviousSectionState::Known(&previously_enabled))
            .is_none()
    );
}
