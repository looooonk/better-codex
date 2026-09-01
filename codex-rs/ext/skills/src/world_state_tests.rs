use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn unchanged_catalog_does_not_render_or_repeat_side_effects() {
    let renders = Arc::new(AtomicUsize::new(0));
    let render_count = Arc::clone(&renders);
    let contribution = executor_skills_world_state_section(
        Some("\n## Skills\n- demo\n".to_string()),
        true,
        Box::new(move || {
            render_count.fetch_add(1, Ordering::Relaxed);
        }),
    );

    assert!(
        contribution
            .render_diff(PreviousWorldStateSection::Absent)
            .is_some()
    );
    assert_eq!(1, renders.load(Ordering::Relaxed));
    assert!(
        contribution
            .render_diff(PreviousWorldStateSection::Known(contribution.snapshot()))
            .is_none()
    );
    assert_eq!(1, renders.load(Ordering::Relaxed));
}

#[test]
fn disabled_orchestrator_catalog_renders_hidden_update() {
    let contribution = orchestrator_skills_world_state_section(
        None,
        false,
        false,
        Box::new(|| {}),
    );
    let previous = serde_json::json!({
        "body": "previous",
        "includeInstructions": true,
        "enabled": true,
    });

    let rendered = contribution
        .render_diff(PreviousWorldStateSection::Known(&previous))
        .expect("disabled catalog should clear its previous body");

    assert_eq!(NO_ORCHESTRATOR_SKILLS_BODY, rendered.body());
}

#[test]
fn fully_omitted_host_catalog_still_reports_availability() {
    let contribution = host_skills_world_state_section(
        None,
        true,
        &SkillRenderReport {
            total_count: 2,
            included_count: 0,
            omitted_count: 2,
            truncated_description_chars: 100,
            truncated_description_count: 2,
        },
        Box::new(|| {}),
    );

    assert_eq!(
        OMITTED_HOST_SKILLS_BODY,
        contribution.snapshot()["body"]
            .as_str()
            .expect("omission marker body")
    );
}
