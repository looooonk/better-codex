use std::borrow::Cow;

use crate::catalog::SkillCatalogEntry;
use crate::catalog::SkillSourceKind;

use super::MAX_CATALOG_SKILL_DESCRIPTION_CHARS;
use super::SkillCatalogRenderPolicy;
use super::SkillMetadataBudget;
use super::TRUNCATED_SKILL_DESCRIPTION_SUFFIX;
use super::metadata_line_cost;

pub(super) struct SkillLine<'a> {
    name: &'a str,
    description: Cow<'a, str>,
    locator: String,
    locator_kind: &'static str,
}

impl<'a> SkillLine<'a> {
    pub(super) fn new(entry: &'a SkillCatalogEntry, policy: SkillCatalogRenderPolicy) -> Self {
        let locator = match &entry.authority.kind {
            SkillSourceKind::Executor | SkillSourceKind::Orchestrator => entry.id.0.as_str(),
            SkillSourceKind::Host | SkillSourceKind::Custom(_) => entry.rendered_path(),
        };
        Self::with_locator(entry, policy, locator.to_string())
    }

    pub(super) fn with_locator(
        entry: &'a SkillCatalogEntry,
        policy: SkillCatalogRenderPolicy,
        locator: String,
    ) -> Self {
        let description = policy.description(entry);
        Self {
            name: entry.name.as_str(),
            description: truncate_catalog_skill_description(description),
            locator,
            locator_kind: match &entry.authority.kind {
                SkillSourceKind::Host => "file",
                SkillSourceKind::Executor => "executor package",
                SkillSourceKind::Orchestrator => "orchestrator package",
                SkillSourceKind::Custom(_) => "custom resource",
            },
        }
    }

    fn full_cost(&self, budget: SkillMetadataBudget) -> usize {
        metadata_line_cost(budget, &self.render_full())
    }

    pub(super) fn minimum_cost(&self, budget: SkillMetadataBudget) -> usize {
        metadata_line_cost(budget, &self.render_minimum())
    }

    fn description_char_count(&self) -> usize {
        self.description.chars().count()
    }

    fn render_full(&self) -> String {
        self.render_with_description(self.description.as_ref())
    }

    fn render_minimum(&self) -> String {
        self.render_with_description("")
    }

    pub(super) fn render_with_description_chars(&self, description_chars: usize) -> String {
        let end = self
            .description
            .char_indices()
            .nth(description_chars)
            .map_or(self.description.len(), |(index, _)| index);
        self.render_with_description(&self.description[..end])
    }

    fn render_with_description(&self, description: &str) -> String {
        let name = self.name;
        let locator = self.locator.as_str();
        let locator_kind = self.locator_kind;
        if description.is_empty() {
            format!("- {name}: ({locator_kind}: {locator})")
        } else {
            format!("- {name}: {description} ({locator_kind}: {locator})")
        }
    }
}

pub(super) struct RenderedSkillLine {
    pub(super) line: String,
}

pub(super) struct RenderedSkillLines {
    pub(super) lines: Vec<RenderedSkillLine>,
    pub(super) omitted_count: usize,
    pub(super) truncated_description_chars: usize,
    pub(super) truncated_description_count: usize,
}

struct DescriptionBudgetLine {
    description_char_count: usize,
    extra_costs: Vec<usize>,
}

impl DescriptionBudgetLine {
    fn new(line: &SkillLine<'_>, budget: SkillMetadataBudget) -> Self {
        let minimum_line = line.render_minimum();
        let minimum_chars = minimum_line.chars().count().saturating_add(1);
        let minimum_bytes = minimum_line.len().saturating_add(1);
        let minimum_cost = budget.cost_from_counts(minimum_chars, minimum_bytes);
        let description_char_count = line.description.chars().count();
        let mut extra_costs = Vec::with_capacity(description_char_count.saturating_add(1));
        extra_costs.push(0);
        let mut prefix_chars = 0usize;
        let mut prefix_bytes = 0usize;
        for ch in line.description.chars() {
            prefix_chars = prefix_chars.saturating_add(1);
            prefix_bytes = prefix_bytes.saturating_add(ch.len_utf8());
            let rendered_chars = minimum_chars.saturating_add(prefix_chars).saturating_add(1);
            let rendered_bytes = minimum_bytes.saturating_add(prefix_bytes).saturating_add(1);
            let cost = budget
                .cost_from_counts(rendered_chars, rendered_bytes)
                .saturating_sub(minimum_cost);
            extra_costs.push(cost);
        }
        Self {
            description_char_count,
            extra_costs,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum SkillLineAllocation {
    Omitted,
    DescriptionChars(usize),
}

pub(super) fn render_skill_lines(
    skill_lines: Vec<SkillLine<'_>>,
    budget: SkillMetadataBudget,
) -> RenderedSkillLines {
    let allocations = allocate_skill_lines(&skill_lines, budget);
    render_allocated_skill_lines(&skill_lines, &allocations)
}

pub(super) fn allocate_skill_lines(
    skill_lines: &[SkillLine<'_>],
    budget: SkillMetadataBudget,
) -> Vec<SkillLineAllocation> {
    let full_cost = skill_lines.iter().fold(0usize, |used, line| {
        used.saturating_add(line.full_cost(budget))
    });
    if full_cost <= budget.limit() {
        return skill_lines
            .iter()
            .map(|line| SkillLineAllocation::DescriptionChars(line.description_char_count()))
            .collect();
    }
    let minimum_cost = skill_lines.iter().fold(0usize, |used, line| {
        used.saturating_add(line.minimum_cost(budget))
    });
    if minimum_cost <= budget.limit() {
        return allocate_description_chars(
            budget,
            skill_lines,
            budget.limit().saturating_sub(minimum_cost),
        )
        .into_iter()
        .map(SkillLineAllocation::DescriptionChars)
        .collect();
    }
    let mut used = 0usize;
    skill_lines
        .iter()
        .map(|line| {
            let next_used = used.saturating_add(line.minimum_cost(budget));
            if next_used <= budget.limit() {
                used = next_used;
                SkillLineAllocation::DescriptionChars(0)
            } else {
                SkillLineAllocation::Omitted
            }
        })
        .collect()
}

pub(super) fn render_allocated_skill_lines(
    skill_lines: &[SkillLine<'_>],
    allocations: &[SkillLineAllocation],
) -> RenderedSkillLines {
    let mut lines = Vec::new();
    let mut omitted_count = 0usize;
    let mut truncated_description_chars = 0usize;
    let mut truncated_description_count = 0usize;
    for (line, allocation) in skill_lines.iter().zip(allocations) {
        let description_char_count = line.description_char_count();
        match allocation {
            SkillLineAllocation::Omitted => {
                omitted_count = omitted_count.saturating_add(1);
                truncated_description_chars =
                    truncated_description_chars.saturating_add(description_char_count);
                if description_char_count > 0 {
                    truncated_description_count = truncated_description_count.saturating_add(1);
                }
            }
            SkillLineAllocation::DescriptionChars(description_chars) => {
                let truncated_chars = description_char_count.saturating_sub(*description_chars);
                if truncated_chars > 0 {
                    truncated_description_chars =
                        truncated_description_chars.saturating_add(truncated_chars);
                    truncated_description_count = truncated_description_count.saturating_add(1);
                }
                lines.push(RenderedSkillLine {
                    line: line.render_with_description_chars(*description_chars),
                });
            }
        }
    }
    RenderedSkillLines {
        lines,
        omitted_count,
        truncated_description_chars,
        truncated_description_count,
    }
}

fn allocate_description_chars(
    budget: SkillMetadataBudget,
    skill_lines: &[SkillLine<'_>],
    limit: usize,
) -> Vec<usize> {
    let budget_lines = skill_lines
        .iter()
        .map(|line| DescriptionBudgetLine::new(line, budget))
        .collect::<Vec<_>>();
    let mut char_allocations = vec![0usize; budget_lines.len()];
    let mut current_extra_costs = vec![0usize; budget_lines.len()];
    let mut remaining = limit;
    loop {
        let mut changed = false;
        for (index, line) in budget_lines.iter().enumerate() {
            if char_allocations[index] >= line.description_char_count {
                continue;
            }
            let next_chars = char_allocations[index].saturating_add(1);
            let next_cost = line.extra_costs[next_chars];
            let delta = next_cost.saturating_sub(current_extra_costs[index]);
            if delta <= remaining {
                char_allocations[index] = next_chars;
                current_extra_costs[index] = next_cost;
                remaining = remaining.saturating_sub(delta);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    char_allocations
}

pub(crate) fn truncate_catalog_skill_description(description: &str) -> Cow<'_, str> {
    if description
        .char_indices()
        .nth(MAX_CATALOG_SKILL_DESCRIPTION_CHARS)
        .is_none()
    {
        return Cow::Borrowed(description);
    }

    let prefix_chars = MAX_CATALOG_SKILL_DESCRIPTION_CHARS
        .saturating_sub(TRUNCATED_SKILL_DESCRIPTION_SUFFIX.chars().count());
    let prefix_end = description
        .char_indices()
        .nth(prefix_chars)
        .map_or(description.len(), |(index, _)| index);
    let mut truncated = description[..prefix_end].to_string();
    truncated.push_str(TRUNCATED_SKILL_DESCRIPTION_SUFFIX);
    Cow::Owned(truncated)
}
