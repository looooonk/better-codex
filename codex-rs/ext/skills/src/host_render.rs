use crate::host_outcome::SkillLoadOutcome;
use codex_skills::SkillMetadata;
use codex_otel::SessionTelemetry;
use codex_otel::THREAD_SKILLS_DESCRIPTION_TRUNCATED_CHARS_METRIC;
use codex_otel::THREAD_SKILLS_ENABLED_TOTAL_METRIC;
use codex_otel::THREAD_SKILLS_KEPT_TOTAL_METRIC;
use codex_otel::THREAD_SKILLS_TRUNCATED_METRIC;
use codex_protocol::protocol::SkillScope;
use codex_utils_output_truncation::approx_token_count;

mod allocation;
mod aliases;

use allocation::SkillLine;
use allocation::render_skill_lines_from_lines;
use aliases::aliased_render_is_better;
use aliases::build_aliased_available_skills;
use aliases::SkillPathAliases;
#[cfg(test)]
use aliases::build_alias_plan;
#[cfg(test)]
use aliases::render_skill_path_with_aliases;

const DEFAULT_SKILL_METADATA_CHAR_BUDGET: usize = 8_000;
const SKILL_METADATA_CONTEXT_WINDOW_PERCENT: usize = 2;
const MAX_DEFAULT_CONTEXT_SKILL_DESCRIPTION_CHARS: usize = 1_024;
const TRUNCATED_SKILL_DESCRIPTION_SUFFIX: &str = "...";
const SKILL_DESCRIPTION_TRUNCATION_WARNING_THRESHOLD_CHARS: usize = 100;
const APPROX_BYTES_PER_TOKEN: usize = 4;
pub const SKILL_DESCRIPTION_TRUNCATED_WARNING: &str = "Skill descriptions were shortened to fit the skills context budget. Codex can still see every skill, but some descriptions are shorter. Disable unused skills or plugins to leave more room for the rest.";
pub const SKILL_DESCRIPTION_TRUNCATED_WARNING_WITH_PERCENT: &str = "Skill descriptions were shortened to fit the 2% skills context budget. Codex can still see every skill, but some descriptions are shorter. Disable unused skills or plugins to leave more room for the rest.";
pub const SKILL_DESCRIPTIONS_REMOVED_WARNING_PREFIX: &str =
    "Exceeded skills context budget. All skill descriptions were removed and";
pub const SKILLS_INTRO_WITH_ABSOLUTE_PATHS: &str = "A skill is a set of instructions provided through a `SKILL.md` source. Below is the list of skills that can be used. Each entry includes a name, description, and source locator. `file` locators are on the host filesystem, `environment resource` locators are owned by an execution environment, `orchestrator resource` locators are opaque non-filesystem resources, and `custom resource` locators use their provider's access mechanism.";
const SKILLS_INTRO_WITH_ALIASES: &str = "A skill is a set of local instructions to follow that is stored in a `SKILL.md` file. Below is the list of skills that can be used. Each entry includes a name, description, and a short path that can be expanded into an absolute path using the skill roots table.";
pub const SKILLS_HOW_TO_USE_WITH_ABSOLUTE_PATHS: &str = r###"- Discovery: The list above is the skills available in this session (name + description + source locator). `file` entries live on the host filesystem, `environment resource` and `orchestrator resource` entries must be accessed through `skills.list` and `skills.read`, and `custom resource` entries use their provider's access mechanism.
- Trigger rules: If the user names a skill (with `$SkillName` or plain text) OR the task clearly matches a skill's description shown above, you must use that skill for that turn. Multiple mentions mean use them all. Do not carry skills across turns unless re-mentioned.
- Missing/blocked: If a named skill isn't in the list or its source can't be read, say so briefly and continue with the best fallback.
- How to use a skill (progressive disclosure):
  1) After deciding to use a skill, the main agent must read its `SKILL.md` completely before taking task actions. For a `file` entry, open the listed path. For an `environment resource`, call `skills.list` with `{"authority":{"kind":"executor"}}`; for an `orchestrator resource`, use `{"authority":{"kind":"orchestrator"}}`. Select the matching package and pass its exact authority, package, and `main_resource` to `skills.read`. Follow `next_cursor`; if a read is paginated, continue until EOF.
  2) When `SKILL.md` references another resource, use the same access mechanism. Resolve relative references beneath an executor skill's returned package and call `skills.read` with the same authority and package. For orchestrator skills, pass the exact referenced resource identifier with the same authority and package to `skills.read`; do not treat `skill://` identifiers as filesystem paths.
  3) If `SKILL.md` points to extra folders such as `references/`, use its routing instructions to identify the resources required for the task. The main agent must read each required instruction or reference file itself before acting on it. Do not delegate reading, summarizing, or interpreting skill instructions to a subagent. Subagents may still perform task work when the selected skill allows it.
  4) For filesystem-backed skills, prefer running or patching provided scripts instead of retyping large code blocks. For environment and orchestrator skills, use `skills.read` and the available tools; do not invent a local path.
  5) Reuse provided assets or templates through the same source access mechanism instead of recreating them.
- Coordination and sequencing:
  - If multiple skills apply, choose the minimal set that covers the request and state the order you'll use them.
  - Announce which skill(s) you're using and why (one short line). If you skip an obvious skill, say why.
- Context hygiene:
  - Progressive disclosure applies to selecting relevant files, not partially reading a selected instruction file. Do not load unrelated references, scripts, or assets.
  - Avoid deep reference-chasing: prefer opening only files directly linked from `SKILL.md` unless you're blocked.
  - When variants exist (frameworks, providers, domains), pick only the relevant reference file(s) and note that choice.
- Safety and fallback: If a skill can't be applied cleanly (missing files, unclear instructions), state the issue, pick the next-best approach, and continue."###;
pub const SKILLS_HOW_TO_USE_WITH_ALIASES: &str = r###"- Discovery: The list above is the skills available in this session (name + description + short path). Skill bodies live on disk at the listed paths after expanding the matching alias from `### Skill roots`.
- Trigger rules: If the user names a skill (with `$SkillName` or plain text) OR the task clearly matches a skill's description shown above, you must use that skill for that turn. Multiple mentions mean use them all. Do not carry skills across turns unless re-mentioned.
- Missing/blocked: If a named skill isn't in the list or the path can't be read, say so briefly and continue with the best fallback.
- How to use a skill (progressive disclosure):
  1) After deciding to use a skill, the main agent must expand the listed short `path` with the matching alias from `### Skill roots`, then open and read its `SKILL.md` completely before taking task actions. If a read is truncated or paginated, continue until EOF.
  2) When `SKILL.md` references relative paths (e.g., `scripts/foo.py`), resolve them relative to the directory containing that expanded `SKILL.md` first, and only consider other paths if needed.
  3) If `SKILL.md` points to extra folders such as `references/`, use its routing instructions to identify the files required for the task. The main agent must read each required instruction or reference file itself before acting on it. Do not delegate reading, summarizing, or interpreting skill instructions to a subagent. Subagents may still perform task work when the selected skill allows it.
  4) If `scripts/` exist, prefer running or patching them instead of retyping large code blocks.
  5) If `assets/` or templates exist, reuse them instead of recreating from scratch.
- Coordination and sequencing:
  - If multiple skills apply, choose the minimal set that covers the request and state the order you'll use them.
  - Announce which skill(s) you're using and why (one short line). If you skip an obvious skill, say why.
- Context hygiene:
  - Progressive disclosure applies to selecting relevant files, not partially reading a selected instruction file. Do not load unrelated references, scripts, or assets.
  - Avoid deep reference-chasing: prefer opening only files directly linked from `SKILL.md` unless you're blocked.
  - When variants exist (frameworks, providers, domains), pick only the relevant reference file(s) and note that choice.
- Safety and fallback: If a skill can't be applied cleanly (missing files, unclear instructions), state the issue, pick the next-best approach, and continue."###;

pub fn render_available_skills_body(skill_root_lines: &[String], skill_lines: &[String]) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("## Skills".to_string());
    if skill_root_lines.is_empty() {
        lines.push(SKILLS_INTRO_WITH_ABSOLUTE_PATHS.to_string());
    } else {
        lines.push(SKILLS_INTRO_WITH_ALIASES.to_string());
        lines.push("### Skill roots".to_string());
        lines.extend(skill_root_lines.iter().cloned());
    }
    lines.push("### Available skills".to_string());
    lines.extend(skill_lines.iter().cloned());

    format!("\n{}\n", lines.join("\n"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillMetadataBudget {
    Tokens(usize),
    Characters(usize),
}

impl SkillMetadataBudget {
    fn limit(self) -> usize {
        match self {
            Self::Tokens(limit) | Self::Characters(limit) => limit,
        }
    }

    fn cost(self, text: &str) -> usize {
        match self {
            Self::Tokens(_) => approx_token_count(text),
            Self::Characters(_) => text.chars().count(),
        }
    }

    fn cost_from_counts(self, chars: usize, bytes: usize) -> usize {
        match self {
            Self::Tokens(_) => approx_token_count_from_bytes(bytes),
            Self::Characters(_) => chars,
        }
    }
}

fn approx_token_count_from_bytes(bytes: usize) -> usize {
    bytes.saturating_add(APPROX_BYTES_PER_TOKEN.saturating_sub(1)) / APPROX_BYTES_PER_TOKEN
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRenderReport {
    pub total_count: usize,
    pub included_count: usize,
    pub omitted_count: usize,
    pub truncated_description_chars: usize,
    pub truncated_description_count: usize,
}

#[derive(Clone, Copy)]
pub enum SkillRenderSideEffects<'a> {
    None,
    ThreadStart {
        session_telemetry: &'a SessionTelemetry,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableSkills {
    pub skill_root_lines: Vec<String>,
    pub skill_lines: Vec<String>,
    pub report: SkillRenderReport,
    pub warning_message: Option<String>,
}

pub fn default_skill_metadata_budget(context_window: Option<i64>) -> SkillMetadataBudget {
    context_window
        .and_then(|window| usize::try_from(window).ok())
        .filter(|window| *window > 0)
        .map(|window| {
            SkillMetadataBudget::Tokens(
                window
                    .saturating_mul(SKILL_METADATA_CONTEXT_WINDOW_PERCENT)
                    .saturating_div(100)
                    .max(1),
            )
        })
        .unwrap_or(SkillMetadataBudget::Characters(
            DEFAULT_SKILL_METADATA_CHAR_BUDGET,
        ))
}

pub fn build_available_skills(
    outcome: &SkillLoadOutcome,
    budget: SkillMetadataBudget,
    side_effects: SkillRenderSideEffects<'_>,
) -> Option<AvailableSkills> {
    let skills = outcome.allowed_skills_for_implicit_invocation();
    if skills.is_empty() {
        record_skill_render_side_effects(
            side_effects,
            /*total_count*/ 0,
            /*included_count*/ 0,
            /*omitted_count*/ 0,
            /*truncated_description_chars*/ 0,
        );
        return None;
    }

    let absolute_lines = ordered_absolute_skill_lines(&skills);
    let absolute = build_available_skills_from_lines(
        absolute_lines,
        skills.len(),
        budget,
        SkillPathAliases::default(),
    )?;

    let selected =
        if absolute.report.omitted_count == 0 && absolute.report.truncated_description_chars == 0 {
            absolute
        } else if let Some(aliased) = build_aliased_available_skills(outcome, &skills, budget) {
            if aliased_render_is_better(&aliased, &absolute, budget) {
                aliased
            } else {
                absolute
            }
        } else {
            absolute
        };

    record_available_skills_side_effects(&selected, budget, side_effects);
    Some(selected)
}

fn build_available_skills_from_lines(
    skill_lines: Vec<SkillLine<'_>>,
    total_count: usize,
    budget: SkillMetadataBudget,
    path_aliases: SkillPathAliases,
) -> Option<AvailableSkills> {
    if total_count == 0 {
        return None;
    }

    let (skill_lines, report) = render_skill_lines_from_lines(skill_lines, total_count, budget);
    let warning_message = if report.omitted_count > 0 {
        let skill_word = if report.omitted_count == 1 {
            "skill"
        } else {
            "skills"
        };
        let verb = if report.omitted_count == 1 {
            "was"
        } else {
            "were"
        };
        Some(format!(
            "{} {} additional {} {} not included in the model-visible skills list.",
            budget_warning_prefix(budget, SKILL_DESCRIPTIONS_REMOVED_WARNING_PREFIX),
            report.omitted_count,
            skill_word,
            verb
        ))
    } else if report.average_truncated_description_chars()
        > SKILL_DESCRIPTION_TRUNCATION_WARNING_THRESHOLD_CHARS
    {
        Some(
            match budget {
                SkillMetadataBudget::Tokens(_) => SKILL_DESCRIPTION_TRUNCATED_WARNING_WITH_PERCENT,
                SkillMetadataBudget::Characters(_) => SKILL_DESCRIPTION_TRUNCATED_WARNING,
            }
            .to_string(),
        )
    } else {
        None
    };
    let available = AvailableSkills {
        skill_root_lines: path_aliases.skill_root_lines,
        skill_lines,
        report,
        warning_message,
    };
    Some(available)
}

fn record_available_skills_side_effects(
    available: &AvailableSkills,
    budget: SkillMetadataBudget,
    side_effects: SkillRenderSideEffects<'_>,
) {
    record_skill_render_side_effects(
        side_effects,
        available.report.total_count,
        available.report.included_count,
        available.report.omitted_count,
        available.report.truncated_description_chars,
    );
    if available.report.omitted_count > 0 || available.report.truncated_description_chars > 0 {
        tracing::info!(
            budget_limit = budget.limit(),
            total_skills = available.report.total_count,
            included_skills = available.report.included_count,
            omitted_skills = available.report.omitted_count,
            truncated_description_chars_per_skill =
                available.report.average_truncated_description_chars(),
            truncated_skill_descriptions = available.report.truncated_description_count,
            "truncated skill metadata to fit skills context budget"
        );
    }
}

fn budget_warning_prefix(budget: SkillMetadataBudget, prefix: &str) -> String {
    match budget {
        SkillMetadataBudget::Tokens(_) => prefix.replacen(
            "Exceeded skills context budget.",
            "Exceeded skills context budget of 2%.",
            1,
        ),
        SkillMetadataBudget::Characters(_) => prefix.to_string(),
    }
}

fn record_skill_render_side_effects(
    side_effects: SkillRenderSideEffects<'_>,
    total_count: usize,
    included_count: usize,
    omitted_count: usize,
    truncated_description_chars: usize,
) {
    match side_effects {
        SkillRenderSideEffects::None => {}
        SkillRenderSideEffects::ThreadStart { session_telemetry } => {
            session_telemetry.histogram(
                THREAD_SKILLS_ENABLED_TOTAL_METRIC,
                i64::try_from(total_count).unwrap_or(i64::MAX),
                &[],
            );
            session_telemetry.histogram(
                THREAD_SKILLS_KEPT_TOTAL_METRIC,
                i64::try_from(included_count).unwrap_or(i64::MAX),
                &[],
            );
            session_telemetry.histogram(
                THREAD_SKILLS_TRUNCATED_METRIC,
                if omitted_count > 0 { 1 } else { 0 },
                &[],
            );
            session_telemetry.histogram(
                THREAD_SKILLS_DESCRIPTION_TRUNCATED_CHARS_METRIC,
                i64::try_from(truncated_description_chars).unwrap_or(i64::MAX),
                &[],
            );
        }
    }
}

fn ordered_absolute_skill_lines(skills: &[SkillMetadata]) -> Vec<SkillLine<'_>> {
    ordered_skills_for_budget(skills)
        .into_iter()
        .map(SkillLine::new)
        .collect()
}

fn ordered_skills_for_budget(skills: &[SkillMetadata]) -> Vec<&SkillMetadata> {
    let mut ordered = skills.iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| {
        prompt_scope_rank(a.scope)
            .cmp(&prompt_scope_rank(b.scope))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path_to_skills_md.cmp(&b.path_to_skills_md))
    });
    ordered
}

fn prompt_scope_rank(scope: SkillScope) -> u8 {
    match scope {
        SkillScope::System => 0,
        SkillScope::Admin => 1,
        SkillScope::Repo => 2,
        SkillScope::User => 3,
    }
}

#[cfg(test)]
#[path = "host_render_tests.rs"]
mod tests;
