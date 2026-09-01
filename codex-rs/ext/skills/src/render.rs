use codex_protocol::protocol::SkillScope;
use codex_utils_string::approx_token_count;
use codex_utils_string::take_bytes_at_char_boundary;

use crate::aliases::AliasPlan;
use crate::aliases::build_catalog_alias_plan;
use crate::catalog::SkillCatalog;
use crate::catalog::SkillCatalogEntry;
use crate::catalog::SkillSourceKind;
use crate::catalog_prompt::SkillPromptKind;
use crate::catalog_prompt::render_available_skills_body;
use crate::fragments::AvailableSkillsInstructions;

mod allocation;
mod combined;

use allocation::RenderedSkillLine;
use allocation::RenderedSkillLines;
use allocation::SkillLine;
use allocation::render_skill_lines;
pub(crate) use allocation::truncate_catalog_skill_description;
pub(crate) use combined::render_combined_available_skills;

const DEFAULT_SKILL_METADATA_CHAR_BUDGET: usize = 8_000;
const MAX_SKILL_PROMPT_BYTES: usize = 8_000;
const SKILL_METADATA_CONTEXT_WINDOW_PERCENT: usize = 2;
const MAX_CATALOG_SKILL_DESCRIPTION_CHARS: usize = 1_024;
const TRUNCATED_SKILL_DESCRIPTION_SUFFIX: &str = "...";
const SKILL_DESCRIPTION_TRUNCATION_WARNING_THRESHOLD_CHARS: usize = 100;
const APPROX_BYTES_PER_TOKEN: usize = 4;
const SKILL_DESCRIPTION_TRUNCATED_WARNING: &str = "Skill descriptions were shortened to fit the skills context budget. Codex can still see every skill, but some descriptions are shorter. Disable unused skills or plugins to leave more room for the rest.";
const SKILL_DESCRIPTIONS_REMOVED_WARNING_PREFIX: &str =
    "Exceeded skills context budget. All skill descriptions were removed and";
pub(crate) const MAX_SKILL_NAME_BYTES: usize = 256;
pub(crate) const MAX_SKILL_PATH_BYTES: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillCatalogRenderPolicy {
    CoreCompatible,
    ExtensionCompatible,
}

impl SkillCatalogRenderPolicy {
    fn description(self, entry: &SkillCatalogEntry) -> &str {
        match self {
            Self::CoreCompatible => entry.description.as_str(),
            Self::ExtensionCompatible => entry
                .short_description
                .as_deref()
                .unwrap_or(entry.description.as_str()),
        }
    }

    fn order_entries(self, entries: &mut [&SkillCatalogEntry]) {
        match self {
            Self::CoreCompatible => {
                let scope_rank = |entry: &SkillCatalogEntry| match entry.prompt_scope() {
                    Some(SkillScope::System) => 0,
                    Some(SkillScope::Admin) => 1,
                    Some(SkillScope::Repo) => 2,
                    Some(SkillScope::User) => 3,
                    None => 4,
                };
                entries.sort_by(|a, b| {
                    scope_rank(a)
                        .cmp(&scope_rank(b))
                        .then_with(|| a.name.cmp(&b.name))
                        .then_with(|| a.main_prompt.as_str().cmp(b.main_prompt.as_str()))
                });
            }
            Self::ExtensionCompatible => {}
        }
    }

    fn includes_omission_notice(self) -> bool {
        match self {
            Self::CoreCompatible => false,
            Self::ExtensionCompatible => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillMetadataBudget {
    Tokens(usize),
    Characters(usize),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct SkillRenderReport {
    pub(crate) total_count: usize,
    pub(crate) included_count: usize,
    pub(crate) omitted_count: usize,
    pub(crate) truncated_description_chars: usize,
    pub(crate) truncated_description_count: usize,
}

impl SkillRenderReport {
    pub(crate) fn warning_message(&self) -> Option<String> {
        if self.omitted_count > 0 {
            let skill_word = if self.omitted_count == 1 {
                "skill"
            } else {
                "skills"
            };
            let verb = if self.omitted_count == 1 {
                "was"
            } else {
                "were"
            };
            return Some(format!(
                "{} {} additional {} {} not included in the model-visible skills list.",
                SKILL_DESCRIPTIONS_REMOVED_WARNING_PREFIX, self.omitted_count, skill_word, verb
            ));
        }

        (self.average_truncated_description_chars()
            > SKILL_DESCRIPTION_TRUNCATION_WARNING_THRESHOLD_CHARS)
            .then(|| SKILL_DESCRIPTION_TRUNCATED_WARNING.to_string())
    }

    pub(crate) fn average_truncated_description_chars(&self) -> usize {
        if self.total_count == 0 || self.truncated_description_chars == 0 {
            return 0;
        }

        self.truncated_description_chars
            .saturating_add(self.total_count.saturating_sub(1))
            / self.total_count
    }
}

pub(crate) fn skill_metadata_budget(context_window: Option<i64>) -> SkillMetadataBudget {
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

fn metadata_line_cost(budget: SkillMetadataBudget, line: &str) -> usize {
    let line = format!("{line}\n");
    match budget {
        SkillMetadataBudget::Tokens(_) => approx_token_count(&line),
        SkillMetadataBudget::Characters(_) => line.chars().count(),
    }
}

impl SkillMetadataBudget {
    pub(crate) fn limit(self) -> usize {
        match self {
            Self::Tokens(limit) | Self::Characters(limit) => limit,
        }
    }

    fn cost_from_counts(self, chars: usize, bytes: usize) -> usize {
        match self {
            Self::Tokens(_) => {
                bytes.saturating_add(APPROX_BYTES_PER_TOKEN.saturating_sub(1))
                    / APPROX_BYTES_PER_TOKEN
            }
            Self::Characters(_) => chars,
        }
    }

    fn cost(self, text: &str) -> usize {
        match self {
            Self::Tokens(_) => approx_token_count(text),
            Self::Characters(_) => text.chars().count(),
        }
    }
}

struct RenderedCatalog {
    prompt_kind: SkillPromptKind,
    skill_root_lines: Vec<String>,
    skill_lines: Vec<String>,
    report: SkillRenderReport,
}

pub(crate) struct AvailableSkillsRender {
    prompt_kind: SkillPromptKind,
    skill_root_lines: Vec<String>,
    skill_lines: Vec<String>,
    preserve_empty_fragment: bool,
    pub(crate) report: SkillRenderReport,
}

#[derive(Default)]
pub(crate) struct RenderedSkillCatalogs {
    pub(crate) executor: Option<AvailableSkillsRender>,
    pub(crate) orchestrator: Option<AvailableSkillsRender>,
    pub(crate) host: Option<AvailableSkillsRender>,
}

impl AvailableSkillsRender {
    pub(crate) fn into_fragment(
        self,
        include_skills_usage_instructions: bool,
    ) -> Option<AvailableSkillsInstructions> {
        (self.preserve_empty_fragment || !self.skill_lines.is_empty()).then(|| {
            AvailableSkillsInstructions::from_skill_lines(
                self.prompt_kind,
                self.skill_root_lines,
                self.skill_lines,
                include_skills_usage_instructions,
            )
        })
    }
}

#[tracing::instrument(
    level = "trace",
    skip_all,
    fields(catalog_entry_count = catalog.entries.len())
)]
pub(crate) fn render_available_skills(
    catalog: &SkillCatalog,
    policy: SkillCatalogRenderPolicy,
    budget: SkillMetadataBudget,
    include_skills_usage_instructions: bool,
) -> Option<AvailableSkillsRender> {
    let mut entries = catalog
        .entries
        .iter()
        .filter(|entry| entry.is_model_visible())
        .collect::<Vec<_>>();
    policy.order_entries(&mut entries);
    if entries.is_empty() {
        return None;
    }

    let absolute = render_catalog(
        entries
            .iter()
            .map(|entry| SkillLine::new(entry, policy))
            .collect(),
        budget,
        Vec::new(),
        SkillPromptKind::Unaliased,
        policy,
    );
    let selected =
        if absolute.report.omitted_count == 0 && absolute.report.truncated_description_chars == 0 {
            absolute
        } else if let Some(aliased) =
            build_aliased_catalog(&entries, policy, budget, include_skills_usage_instructions)
            && aliased_render_is_better(
                &aliased,
                &absolute,
                budget,
                include_skills_usage_instructions,
            )
        {
            aliased
        } else {
            absolute
        };

    Some(AvailableSkillsRender {
        prompt_kind: selected.prompt_kind,
        skill_root_lines: selected.skill_root_lines,
        skill_lines: selected.skill_lines,
        preserve_empty_fragment: policy == SkillCatalogRenderPolicy::CoreCompatible,
        report: selected.report,
    })
}

pub(crate) fn render_extension_catalog(
    catalog: &SkillCatalog,
    include_skills_usage_instructions: bool,
    context_window: Option<i64>,
) -> (Option<AvailableSkillsInstructions>, SkillRenderReport) {
    let Some(rendered) = render_available_skills(
        catalog,
        SkillCatalogRenderPolicy::ExtensionCompatible,
        skill_metadata_budget(context_window),
        include_skills_usage_instructions,
    ) else {
        return (None, SkillRenderReport::default());
    };
    let report = rendered.report.clone();
    let fragment = rendered.into_fragment(include_skills_usage_instructions);
    if report.omitted_count > 0 || report.truncated_description_chars > 0 {
        tracing::info!(
            total_skills = report.total_count,
            included_skills = report.included_count,
            omitted_skills = report.omitted_count,
            truncated_description_chars = report.truncated_description_chars,
            "truncated extension skill metadata to fit skills context budget"
        );
    }
    (fragment, report)
}

fn render_catalog(
    skill_lines: Vec<SkillLine<'_>>,
    budget: SkillMetadataBudget,
    skill_root_lines: Vec<String>,
    prompt_kind: SkillPromptKind,
    policy: SkillCatalogRenderPolicy,
) -> RenderedCatalog {
    let total_count = skill_lines.len();
    let RenderedSkillLines {
        lines: mut rendered_lines,
        omitted_count: mut omitted,
        truncated_description_chars,
        truncated_description_count,
    } = render_skill_lines(skill_lines, budget);
    let mut total_cost = rendered_lines.iter().fold(0usize, |used, rendered| {
        used.saturating_add(metadata_line_cost(budget, &rendered.line))
    });

    if omitted > 0 && policy.includes_omission_notice() {
        loop {
            let marker = omission_marker(omitted);
            if total_cost.saturating_add(metadata_line_cost(budget, &marker)) <= budget.limit() {
                rendered_lines.push(RenderedSkillLine { line: marker });
                break;
            }
            let Some(rendered) = rendered_lines.pop() else {
                break;
            };
            total_cost = total_cost.saturating_sub(metadata_line_cost(budget, &rendered.line));
            omitted = omitted.saturating_add(1);
        }
    }

    RenderedCatalog {
        prompt_kind,
        skill_root_lines,
        skill_lines: rendered_lines
            .into_iter()
            .map(|rendered| rendered.line)
            .collect(),
        report: SkillRenderReport {
            total_count,
            included_count: total_count.saturating_sub(omitted),
            omitted_count: omitted,
            truncated_description_chars,
            truncated_description_count,
        },
    }
}

#[cfg(test)]
fn available_skills_fragment(
    catalog: &SkillCatalog,
    include_skills_usage_instructions: bool,
    policy: SkillCatalogRenderPolicy,
    budget: SkillMetadataBudget,
) -> Option<AvailableSkillsInstructions> {
    render_available_skills(catalog, policy, budget, include_skills_usage_instructions)?
        .into_fragment(include_skills_usage_instructions)
}

fn build_aliased_catalog(
    entries: &[&SkillCatalogEntry],
    policy: SkillCatalogRenderPolicy,
    budget: SkillMetadataBudget,
    include_skills_usage_instructions: bool,
) -> Option<RenderedCatalog> {
    let catalog = CatalogLines::aliased(entries, policy);
    if catalog.root_lines.is_empty() {
        return None;
    }
    let table_cost = aliased_metadata_overhead_cost(
        budget,
        catalog.prompt_kind,
        &catalog.root_lines,
        include_skills_usage_instructions,
    );
    if table_cost >= budget.limit() {
        return None;
    }

    let adjusted_limit = budget.limit().saturating_sub(table_cost);
    let adjusted_budget = match budget {
        SkillMetadataBudget::Tokens(_) => SkillMetadataBudget::Tokens(adjusted_limit),
        SkillMetadataBudget::Characters(_) => SkillMetadataBudget::Characters(adjusted_limit),
    };
    Some(render_catalog(
        catalog.skills,
        adjusted_budget,
        catalog.root_lines,
        catalog.prompt_kind,
        policy,
    ))
}

fn build_alias_plan(entries: &[&SkillCatalogEntry]) -> Option<AliasPlan> {
    build_catalog_alias_plan(entries)
}

fn render_skill_locator_with_aliases(entry: &SkillCatalogEntry, plan: &AliasPlan) -> String {
    let locator = match &entry.authority.kind {
        SkillSourceKind::Executor | SkillSourceKind::Orchestrator => entry.id.0.as_str(),
        SkillSourceKind::Host | SkillSourceKind::Custom(_) => entry.rendered_path(),
    };
    if entry.alias_root().is_none() {
        return locator.to_string();
    }
    plan.shorten(locator).unwrap_or_else(|| locator.to_string())
}

fn aliased_metadata_overhead_cost(
    budget: SkillMetadataBudget,
    prompt_kind: SkillPromptKind,
    skill_root_lines: &[String],
    include_skills_usage_instructions: bool,
) -> usize {
    let empty_skill_lines: &[String] = &[];
    let absolute_body =
        render_available_skills_body(SkillPromptKind::Unaliased, &[], empty_skill_lines);
    let aliased_body =
        render_available_skills_body(prompt_kind, skill_root_lines, empty_skill_lines);
    let alias_instruction_cost = if include_skills_usage_instructions {
        prompt_kind
            .alias_instructions()
            .map_or(0, |instructions| metadata_line_cost(budget, instructions))
    } else {
        0
    };
    budget
        .cost(&aliased_body)
        .saturating_add(alias_instruction_cost)
        .saturating_sub(budget.cost(&absolute_body))
}

fn aliased_render_is_better(
    aliased: &RenderedCatalog,
    absolute: &RenderedCatalog,
    budget: SkillMetadataBudget,
    include_skills_usage_instructions: bool,
) -> bool {
    if aliased.report.included_count != absolute.report.included_count {
        return aliased.report.included_count > absolute.report.included_count;
    }
    if aliased.report.truncated_description_chars != absolute.report.truncated_description_chars {
        return aliased.report.truncated_description_chars
            < absolute.report.truncated_description_chars;
    }
    rendered_catalog_cost(budget, aliased, include_skills_usage_instructions)
        < rendered_catalog_cost(budget, absolute, include_skills_usage_instructions)
}

fn rendered_catalog_cost(
    budget: SkillMetadataBudget,
    rendered: &RenderedCatalog,
    include_skills_usage_instructions: bool,
) -> usize {
    let metadata_cost = if rendered.skill_root_lines.is_empty() {
        0
    } else {
        aliased_metadata_overhead_cost(
            budget,
            rendered.prompt_kind,
            &rendered.skill_root_lines,
            include_skills_usage_instructions,
        )
    };
    rendered
        .skill_lines
        .iter()
        .fold(metadata_cost, |used, line| {
            used.saturating_add(metadata_line_cost(budget, line))
        })
}

fn omission_marker(omitted: usize) -> String {
    let skill_word = if omitted == 1 { "skill" } else { "skills" };
    format!("- {omitted} additional {skill_word} omitted from this bounded skills list.")
}

pub(crate) fn truncate_main_prompt_contents(contents: &str) -> (String, bool) {
    truncate_utf8_to_bytes(contents, MAX_SKILL_PROMPT_BYTES)
}

pub(crate) fn truncate_utf8_to_bytes(contents: &str, max_bytes: usize) -> (String, bool) {
    let truncated = take_bytes_at_char_boundary(contents, max_bytes);
    (truncated.to_string(), truncated.len() < contents.len())
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
