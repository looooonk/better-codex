use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use codex_protocol::protocol::SKILLS_INSTRUCTIONS_CLOSE_TAG;
use codex_protocol::protocol::SKILLS_INSTRUCTIONS_OPEN_TAG;
use serde_json::json;

use crate::fragments::SkillsUpdate;
use crate::fragments::SkillsUpdateSection;
use crate::render::SkillRenderReport;

pub(crate) const SKILLS_WORLD_STATE_ID: &str = "skills";
pub(crate) const ORCHESTRATOR_SKILLS_WORLD_STATE_ID: &str = "orchestrator_skills";
pub(crate) const HOST_SKILLS_WORLD_STATE_ID: &str = "host_skills";
const NO_EXECUTOR_SKILLS_BODY: &str =
    "\n## Skills update\nNo selected-environment skills are currently available.\n";
const HIDDEN_EXECUTOR_SKILLS_BODY: &str = "\n## Skills update\nSelected-environment skills are not listed automatically. Explicit skill mentions can still be resolved when available.\n";
const NO_ORCHESTRATOR_SKILLS_BODY: &str =
    "\n## Orchestrator skills update\nNo orchestrator skills are currently available.\n";
const HIDDEN_ORCHESTRATOR_SKILLS_BODY: &str = "\n## Orchestrator skills update\nOrchestrator skills are not listed automatically. Explicit skill mentions can still be resolved when available.\n";
const NO_HOST_SKILLS_BODY: &str =
    "\n## Host skills update\nNo host skills are currently available.\n";
const HIDDEN_HOST_SKILLS_BODY: &str = "\n## Host skills update\nHost skills are not listed automatically. Explicit skill mentions can still be resolved when available.\n";
const OMITTED_HOST_SKILLS_BODY: &str = "\n## Host skills update\nHost skills are available but omitted from the model-visible skills list because the skills context budget was exceeded.\n";

pub(crate) type CatalogRenderCallback = Box<dyn Fn() + Send + Sync>;

fn is_wrapped_skills_fragment(role: &str, text: &str) -> bool {
    role == "developer"
        && text.trim_start().starts_with(SKILLS_INSTRUCTIONS_OPEN_TAG)
        && text.trim_end().ends_with(SKILLS_INSTRUCTIONS_CLOSE_TAG)
}

fn is_executor_skills_fragment(role: &str, text: &str) -> bool {
    is_skills_fragment(SkillsUpdateSection::Executor, role, text)
}

fn is_orchestrator_skills_fragment(role: &str, text: &str) -> bool {
    is_skills_fragment(SkillsUpdateSection::Orchestrator, role, text)
}

fn is_host_skills_fragment(role: &str, text: &str) -> bool {
    is_skills_fragment(SkillsUpdateSection::Host, role, text)
}

fn is_skills_fragment(section: SkillsUpdateSection, role: &str, text: &str) -> bool {
    if let Some(rendered_section) = SkillsUpdateSection::from_rendered_fragment(role, text) {
        return rendered_section == section;
    }
    if !is_wrapped_skills_fragment(role, text) {
        return false;
    }
    match section {
        SkillsUpdateSection::Executor => {
            text.contains("(executor package:")
                || text.contains("(environment resource:")
                || text.contains(NO_EXECUTOR_SKILLS_BODY.trim())
                || text.contains(HIDDEN_EXECUTOR_SKILLS_BODY.trim())
        }
        SkillsUpdateSection::Orchestrator => {
            text.contains("(orchestrator package:")
                || text.contains(NO_ORCHESTRATOR_SKILLS_BODY.trim())
                || text.contains(HIDDEN_ORCHESTRATOR_SKILLS_BODY.trim())
        }
        SkillsUpdateSection::Host => {
            text.contains("(file:")
                || text.contains(NO_HOST_SKILLS_BODY.trim())
                || text.contains(HIDDEN_HOST_SKILLS_BODY.trim())
                || text.contains(OMITTED_HOST_SKILLS_BODY.trim())
        }
    }
}

fn rendered_skills_fragment(section: SkillsUpdateSection, body: &str) -> String {
    SkillsUpdate::rendered_text(section, body)
}

pub(crate) fn executor_skills_world_state_section(
    body: Option<String>,
    include_instructions: bool,
    on_render: CatalogRenderCallback,
) -> WorldStateSectionContribution {
    skills_world_state_section(
        SkillsUpdateSection::Executor,
        body,
        include_instructions,
        /*enabled*/ None,
        NO_EXECUTOR_SKILLS_BODY,
        HIDDEN_EXECUTOR_SKILLS_BODY,
        on_render,
    )
    .with_legacy_matcher(is_executor_skills_fragment)
    .with_section_fragment_matcher(is_executor_skills_fragment)
}

pub(crate) fn orchestrator_skills_world_state_section(
    body: Option<String>,
    include_instructions: bool,
    enabled: bool,
    on_render: CatalogRenderCallback,
) -> WorldStateSectionContribution {
    skills_world_state_section(
        SkillsUpdateSection::Orchestrator,
        body,
        include_instructions,
        Some(enabled),
        NO_ORCHESTRATOR_SKILLS_BODY,
        if enabled {
            HIDDEN_ORCHESTRATOR_SKILLS_BODY
        } else {
            NO_ORCHESTRATOR_SKILLS_BODY
        },
        on_render,
    )
    .with_section_fragment_matcher(is_orchestrator_skills_fragment)
}

fn skills_world_state_section(
    section: SkillsUpdateSection,
    body: Option<String>,
    include_instructions: bool,
    enabled: Option<bool>,
    no_skills_body: &'static str,
    hidden_skills_body: &'static str,
    on_render: CatalogRenderCallback,
) -> WorldStateSectionContribution {
    let id = match section {
        SkillsUpdateSection::Executor => SKILLS_WORLD_STATE_ID,
        SkillsUpdateSection::Orchestrator => ORCHESTRATOR_SKILLS_WORLD_STATE_ID,
        SkillsUpdateSection::Host => HOST_SKILLS_WORLD_STATE_ID,
    };
    let mut snapshot = json!({
        "body": body,
        "includeInstructions": include_instructions,
    });
    if let Some(enabled) = enabled {
        snapshot["enabled"] = json!(enabled);
    }
    let retained_body = body.as_deref().unwrap_or(if include_instructions {
        no_skills_body
    } else {
        hidden_skills_body
    });
    let retained_fragment = rendered_skills_fragment(section, retained_body);

    let contribution = WorldStateSectionContribution::new(id, snapshot, move |previous| {
        if let PreviousWorldStateSection::Known(previous) = &previous {
            let previous_body = previous.get("body").and_then(serde_json::Value::as_str);
            let previous_include_instructions = previous
                .get("includeInstructions")
                .and_then(serde_json::Value::as_bool);
            let previous_enabled = previous.get("enabled").and_then(serde_json::Value::as_bool);
            if previous_body == body.as_deref()
                && previous_include_instructions == Some(include_instructions)
                && previous_enabled == enabled
            {
                return None;
            }
        }

        let body = match body.as_deref() {
            Some(body) => body,
            None if matches!(previous, PreviousWorldStateSection::Absent) => return None,
            None if !include_instructions => hidden_skills_body,
            None => no_skills_body,
        };
        on_render();
        Some(RenderedWorldStateFragment::new(SkillsUpdate::new(
            section, body,
        )))
    });
    contribution.with_retained_fragment_matcher(move |role, text| {
        role == "developer" && text == retained_fragment
    })
}

pub(crate) fn host_skills_world_state_section(
    body: Option<String>,
    include_instructions: bool,
    report: &SkillRenderReport,
    on_render: CatalogRenderCallback,
) -> WorldStateSectionContribution {
    let body = body.or_else(|| {
        (report.included_count == 0 && report.omitted_count > 0)
            .then(|| OMITTED_HOST_SKILLS_BODY.to_string())
    });
    skills_world_state_section(
        SkillsUpdateSection::Host,
        body,
        include_instructions,
        /*enabled*/ None,
        NO_HOST_SKILLS_BODY,
        HIDDEN_HOST_SKILLS_BODY,
        on_render,
    )
    .with_section_fragment_matcher(is_host_skills_fragment)
}

#[cfg(test)]
#[path = "world_state_tests.rs"]
mod tests;
