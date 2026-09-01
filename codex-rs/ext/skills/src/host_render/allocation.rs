use std::borrow::Cow;

use codex_skills::SkillMetadata;

use super::MAX_DEFAULT_CONTEXT_SKILL_DESCRIPTION_CHARS;
use super::SkillMetadataBudget;
use super::SkillRenderReport;
use super::TRUNCATED_SKILL_DESCRIPTION_SUFFIX;

pub(super) fn render_skill_lines_from_lines(
    skill_lines: Vec<SkillLine<'_>>,
    total_count: usize,
    budget: SkillMetadataBudget,
) -> (Vec<String>, SkillRenderReport) {
    let full_cost = skill_lines.iter().fold(0usize, |used, line| {
        used.saturating_add(line.full_cost(budget))
    });
    if full_cost <= budget.limit() {
        let included = skill_lines
            .iter()
            .map(SkillLine::render_full)
            .collect::<Vec<_>>();

        return (
            included,
            skill_render_report(
                total_count,
                /*included_count*/ skill_lines.len(),
                /*omitted_count*/ 0,
                /*truncated_description_chars*/ 0,
                /*truncated_description_count*/ 0,
            ),
        );
    }

    let minimum_cost = skill_lines.iter().fold(0usize, |used, line| {
        used.saturating_add(line.minimum_cost(budget))
    });
    if minimum_cost <= budget.limit() {
        let rendered = render_lines_with_description_budget(
            budget,
            &skill_lines,
            budget.limit().saturating_sub(minimum_cost),
        );
        let (truncated_description_chars, truncated_description_count) =
            sum_description_truncation(&rendered);
        let included = rendered
            .into_iter()
            .map(|rendered| rendered.line)
            .collect::<Vec<_>>();

        return (
            included,
            skill_render_report(
                total_count,
                /*included_count*/ skill_lines.len(),
                /*omitted_count*/ 0,
                truncated_description_chars,
                truncated_description_count,
            ),
        );
    }

    render_minimum_skill_lines_until_budget(budget, skill_lines, total_count)
}
fn render_minimum_skill_lines_until_budget(
    budget: SkillMetadataBudget,
    skill_lines: Vec<SkillLine<'_>>,
    total_count: usize,
) -> (Vec<String>, SkillRenderReport) {
    let mut included = Vec::new();
    let mut used = 0usize;
    let mut omitted_count = 0usize;
    let mut truncated_description_chars = 0usize;
    let mut truncated_description_count = 0usize;
    for line in skill_lines {
        let line_cost = line.minimum_cost(budget);
        let description_char_count = line.description_char_count();
        if used.saturating_add(line_cost) <= budget.limit() {
            used = used.saturating_add(line_cost);
            included.push(line.render_minimum());
        } else {
            omitted_count = omitted_count.saturating_add(1);
        }

        truncated_description_chars =
            truncated_description_chars.saturating_add(description_char_count);
        if description_char_count > 0 {
            truncated_description_count = truncated_description_count.saturating_add(1);
        }
    }

    let report = skill_render_report(
        total_count,
        included.len(),
        omitted_count,
        truncated_description_chars,
        truncated_description_count,
    );

    (included, report)
}

fn skill_render_report(
    total_count: usize,
    included_count: usize,
    omitted_count: usize,
    truncated_description_chars: usize,
    truncated_description_count: usize,
) -> SkillRenderReport {
    SkillRenderReport {
        total_count,
        included_count,
        omitted_count,
        truncated_description_chars,
        truncated_description_count,
    }
}

impl SkillRenderReport {
    pub(super) fn average_truncated_description_chars(&self) -> usize {
        if self.total_count == 0 || self.truncated_description_chars == 0 {
            return 0;
        }

        self.truncated_description_chars
            .saturating_add(self.total_count.saturating_sub(1))
            / self.total_count
    }
}

pub(super) struct SkillLine<'a> {
    name: &'a str,
    description: Cow<'a, str>,
    path: String,
}

struct RenderedSkillLine {
    line: String,
    truncated_chars: usize,
}

struct DescriptionBudgetLine<'a> {
    line: &'a SkillLine<'a>,
    description_char_count: usize,
    extra_costs: Vec<usize>,
}

fn sum_description_truncation(rendered: &[RenderedSkillLine]) -> (usize, usize) {
    rendered
        .iter()
        .fold((0usize, 0usize), |(chars, count), line| {
            if line.truncated_chars == 0 {
                (chars, count)
            } else {
                (
                    chars.saturating_add(line.truncated_chars),
                    count.saturating_add(1),
                )
            }
        })
}

impl<'a> SkillLine<'a> {
    pub(super) fn new(skill: &'a SkillMetadata) -> Self {
        Self::with_path(
            skill,
            skill.path_to_skills_md.to_string_lossy().replace('\\', "/"),
        )
    }

    pub(super) fn with_path(skill: &'a SkillMetadata, path: String) -> Self {
        let description = truncate_default_context_skill_description(skill.description.as_str());
        Self {
            name: skill.name.as_str(),
            description,
            path,
        }
    }

    pub(super) fn full_cost(&self, budget: SkillMetadataBudget) -> usize {
        line_cost(budget, &self.render_full())
    }

    pub(super) fn minimum_cost(&self, budget: SkillMetadataBudget) -> usize {
        line_cost(budget, &self.render_minimum())
    }

    pub(super) fn description_char_count(&self) -> usize {
        self.description.chars().count()
    }

    pub(super) fn render_full(&self) -> String {
        self.render_with_description(self.description.as_ref())
    }

    pub(super) fn render_minimum(&self) -> String {
        self.render_with_description("")
    }

    fn rendered_description_prefix_len(&self, description_chars: usize) -> usize {
        self.description
            .char_indices()
            .nth(description_chars)
            .map_or(self.description.len(), |(idx, _)| idx)
    }

    pub(super) fn render_with_description_chars(&self, description_chars: usize) -> String {
        if description_chars == 0 {
            format!("- {}: (file: {})", self.name, self.path)
        } else {
            let end = self.rendered_description_prefix_len(description_chars);
            let description = &self.description.as_ref()[..end];
            format!("- {}: {} (file: {})", self.name, description, self.path)
        }
    }

    pub(super) fn render_with_description(&self, description: &str) -> String {
        if description.is_empty() {
            format!("- {}: (file: {})", self.name, self.path)
        } else {
            format!("- {}: {} (file: {})", self.name, description, self.path)
        }
    }
}

fn truncate_default_context_skill_description(description: &str) -> Cow<'_, str> {
    if description
        .char_indices()
        .nth(MAX_DEFAULT_CONTEXT_SKILL_DESCRIPTION_CHARS)
        .is_none()
    {
        return Cow::Borrowed(description);
    }

    let prefix_chars = MAX_DEFAULT_CONTEXT_SKILL_DESCRIPTION_CHARS
        .saturating_sub(TRUNCATED_SKILL_DESCRIPTION_SUFFIX.chars().count());
    let prefix_end = description
        .char_indices()
        .nth(prefix_chars)
        .map_or(description.len(), |(index, _)| index);
    let mut truncated = description[..prefix_end].to_string();
    truncated.push_str(TRUNCATED_SKILL_DESCRIPTION_SUFFIX);
    Cow::Owned(truncated)
}

impl<'a> DescriptionBudgetLine<'a> {
    fn new(line: &'a SkillLine<'a>, budget: SkillMetadataBudget) -> Self {
        let minimum_line = line.render_minimum();
        let minimum_chars = minimum_line.chars().count().saturating_add(1);
        let minimum_bytes = minimum_line.len().saturating_add(1);
        let minimum_cost = budget.cost_from_counts(minimum_chars, minimum_bytes);

        let description_char_count = line.description_char_count();
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
            line,
            description_char_count,
            extra_costs,
        }
    }
}

fn line_cost(budget: SkillMetadataBudget, line: &str) -> usize {
    budget.cost(&format!("{line}\n"))
}

pub(super) fn lines_cost(budget: SkillMetadataBudget, lines: &[String]) -> usize {
    lines.iter().fold(0usize, |used, line| {
        used.saturating_add(line_cost(budget, line))
    })
}

fn render_lines_with_description_budget(
    budget: SkillMetadataBudget,
    skill_lines: &[SkillLine<'_>],
    limit: usize,
) -> Vec<RenderedSkillLine> {
    let budget_lines = skill_lines
        .iter()
        .map(|line| DescriptionBudgetLine::new(line, budget))
        .collect::<Vec<_>>();
    let mut char_allocations = vec![0usize; budget_lines.len()];
    let mut current_extra_costs = vec![0usize; budget_lines.len()];
    let mut remaining = limit;

    // Distribute description space one character at a time across skills.
    // Short descriptions naturally drop out, so their unused share can go to
    // longer descriptions instead of being stranded in a fixed per-skill quota.
    loop {
        let mut changed = false;
        for (index, line) in budget_lines.iter().enumerate() {
            if char_allocations[index] >= line.description_char_count {
                continue;
            }

            let current_cost = current_extra_costs[index];
            let next_chars = char_allocations[index].saturating_add(1);
            let next_cost = line.extra_costs[next_chars];
            let delta = next_cost.saturating_sub(current_cost);
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

    budget_lines
        .iter()
        .zip(char_allocations)
        .map(|(line, description_chars)| {
            let truncated_chars = line
                .description_char_count
                .saturating_sub(description_chars);
            RenderedSkillLine {
                line: line.line.render_with_description_chars(description_chars),
                truncated_chars,
            }
        })
        .collect()
}
