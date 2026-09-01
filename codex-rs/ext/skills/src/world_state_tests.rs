use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use pretty_assertions::assert_eq;

use super::*;

fn assert_catalog_matchers(
    contribution: &WorldStateSectionContribution,
    section: SkillsUpdateSection,
    current_body: &str,
    later_body: &str,
) {
    let current = rendered_skills_fragment(section, current_body);
    let later = rendered_skills_fragment(section, later_body);

    assert!(contribution.matches_retained_fragment("developer", &current));
    assert!(!contribution.matches_retained_fragment("developer", &later));
    assert!(contribution.matches_section_fragment("developer", &later));
}

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
    let contribution = orchestrator_skills_world_state_section(None, false, false, Box::new(|| {}));
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

#[test]
fn catalog_retention_matches_exact_current_fragment_and_later_section_values() {
    let executor_body = "\n## Skills\n- demo: (executor package: skill://executor/demo)\n";
    let executor =
        executor_skills_world_state_section(Some(executor_body.to_string()), true, Box::new(|| {}));
    assert_catalog_matchers(
        &executor,
        SkillsUpdateSection::Executor,
        executor_body,
        "\n## Skills\n- demo: (executor package: skill://executor/demo)\n- extra: (executor package: skill://executor/extra)\n",
    );

    let orchestrator_body =
        "\n## Skills\n- demo: (orchestrator package: skill://orchestrator/demo)\n";
    let orchestrator = orchestrator_skills_world_state_section(
        Some(orchestrator_body.to_string()),
        true,
        true,
        Box::new(|| {}),
    );
    assert_catalog_matchers(
        &orchestrator,
        SkillsUpdateSection::Orchestrator,
        orchestrator_body,
        "\n## Skills\n- demo: (orchestrator package: skill://orchestrator/demo)\n- extra: (orchestrator package: skill://orchestrator/extra)\n",
    );

    let host_body = "\n## Skills\n- demo: (file: /tmp/demo/SKILL.md)\n";
    let host = host_skills_world_state_section(
        Some(host_body.to_string()),
        true,
        &SkillRenderReport {
            total_count: 1,
            included_count: 1,
            omitted_count: 0,
            truncated_description_chars: 0,
            truncated_description_count: 0,
        },
        Box::new(|| {}),
    );
    assert_catalog_matchers(
        &host,
        SkillsUpdateSection::Host,
        host_body,
        "\n## Skills\n- demo: (file: /tmp/demo/SKILL.md)\n- extra: (file: /tmp/extra/SKILL.md)\n",
    );
}

#[test]
fn tagged_catalog_identity_ignores_other_section_locators_in_descriptions() {
    let executor_body = "\n## Skills\n- executor: reads (file: /tmp/demo/SKILL.md) (executor package: skill://executor/demo)\n";
    let host_body = "\n## Skills\n- host: reads (executor package: skill://executor/demo) (file: /tmp/demo/SKILL.md)\n";
    let executor =
        executor_skills_world_state_section(Some(executor_body.to_string()), true, Box::new(|| {}));
    let host = host_skills_world_state_section(
        Some(host_body.to_string()),
        true,
        &SkillRenderReport {
            total_count: 1,
            included_count: 1,
            omitted_count: 0,
            truncated_description_chars: 0,
            truncated_description_count: 0,
        },
        Box::new(|| {}),
    );
    let executor_fragment = rendered_skills_fragment(SkillsUpdateSection::Executor, executor_body);
    let host_fragment = rendered_skills_fragment(SkillsUpdateSection::Host, host_body);

    assert_eq!(
        (
            executor.matches_section_fragment("developer", &executor_fragment),
            executor.matches_section_fragment("developer", &host_fragment),
            host.matches_section_fragment("developer", &executor_fragment),
            host.matches_section_fragment("developer", &host_fragment),
        ),
        (true, false, false, true)
    );
}

#[test]
fn empty_catalog_reasserts_and_retains_its_revocation_fragment() {
    let contribution = executor_skills_world_state_section(None, true, Box::new(|| {}));
    let stale = rendered_skills_fragment(
        SkillsUpdateSection::Executor,
        "\n## Skills\n- demo: (executor package: skill://executor/demo)\n",
    );
    let retained = rendered_skills_fragment(SkillsUpdateSection::Executor, NO_EXECUTOR_SKILLS_BODY);

    assert_eq!(
        (
            contribution.matches_section_fragment("developer", &stale),
            contribution.matches_retained_fragment("developer", &stale),
            contribution.matches_section_fragment("developer", &retained),
            contribution.matches_retained_fragment("developer", &retained),
            contribution
                .render_diff(PreviousWorldStateSection::Absent)
                .map(|fragment| fragment.body()),
            contribution
                .render_diff(PreviousWorldStateSection::Unknown)
                .map(|fragment| fragment.body()),
        ),
        (
            true,
            false,
            true,
            true,
            None,
            Some(NO_EXECUTOR_SKILLS_BODY.to_string()),
        )
    );
}
