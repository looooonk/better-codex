use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Component;
use std::path::Path;

use codex_skills::SkillMetadata;
use codex_utils_absolute_path::AbsolutePathBuf;

use crate::host_outcome::SkillLoadOutcome;

use super::AvailableSkills;
use super::SkillMetadataBudget;
use super::allocation::SkillLine;
use super::allocation::lines_cost;
use super::build_available_skills_from_lines;
use super::ordered_skills_for_budget;
use super::render_available_skills_body;

pub(super) fn build_aliased_available_skills(
    outcome: &SkillLoadOutcome,
    skills: &[SkillMetadata],
    budget: SkillMetadataBudget,
) -> Option<AvailableSkills> {
    let plan = build_alias_plan(outcome, skills, budget)?;
    if plan.table_cost >= budget.limit() {
        return None;
    }

    let adjusted_limit = budget.limit().saturating_sub(plan.table_cost);
    let adjusted_budget = match budget {
        SkillMetadataBudget::Tokens(_) => SkillMetadataBudget::Tokens(adjusted_limit),
        SkillMetadataBudget::Characters(_) => SkillMetadataBudget::Characters(adjusted_limit),
    };
    let ordered_skills = ordered_skills_for_budget(skills);
    let skill_lines = ordered_skills
        .into_iter()
        .map(|skill| SkillLine::with_path(skill, render_skill_path_with_aliases(skill, &plan)))
        .collect::<Vec<_>>();
    build_available_skills_from_lines(skill_lines, skills.len(), adjusted_budget, plan.aliases)
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SkillPathAliases {
    pub(super) skill_root_lines: Vec<String>,
}

pub(super) struct AliasPlan {
    pub(super) aliases: SkillPathAliases,
    pub(super) root_aliases: HashMap<AbsolutePathBuf, String>,
    pub(super) alias_root_by_path: HashMap<AbsolutePathBuf, AbsolutePathBuf>,
    pub(super) table_cost: usize,
}

pub(super) fn build_alias_plan(
    outcome: &SkillLoadOutcome,
    skills: &[SkillMetadata],
    budget: SkillMetadataBudget,
) -> Option<AliasPlan> {
    let skill_paths = skills
        .iter()
        .map(|skill| skill.path_to_skills_md.clone())
        .collect::<HashSet<_>>();
    let skill_root_by_path = outcome
        .skill_root_by_path
        .iter()
        .filter(|(path, _)| skill_paths.contains(*path))
        .map(|(path, root)| (path.clone(), root.clone()))
        .collect::<HashMap<_, _>>();
    let used_roots = outcome
        .skill_roots
        .iter()
        .filter(|root| {
            skill_root_by_path
                .values()
                .any(|skill_root| skill_root == *root)
        })
        .cloned()
        .collect::<Vec<_>>();
    if used_roots.is_empty() {
        return None;
    }

    let plugin_version_skill_counts =
        plugin_version_skill_counts_for_skill_roots(skill_root_by_path.values());
    let alias_root_by_skill_root = used_roots
        .iter()
        .map(|root| {
            (
                root.clone(),
                alias_root_for_skill_root(root, &plugin_version_skill_counts),
            )
        })
        .collect::<HashMap<_, _>>();
    let alias_roots = ordered_alias_roots(&used_roots, &alias_root_by_skill_root)?;
    let root_aliases = alias_roots
        .iter()
        .enumerate()
        .map(|(index, alias_root)| (alias_root.clone(), format!("r{index}")))
        .collect::<HashMap<_, _>>();
    let alias_root_by_path = skill_root_by_path
        .iter()
        .filter_map(|(path, skill_root)| {
            alias_root_by_skill_root
                .get(skill_root)
                .map(|alias_root| (path.clone(), alias_root.clone()))
        })
        .collect::<HashMap<_, _>>();
    let skill_root_lines = build_skill_root_lines(&alias_roots);
    let table_cost = aliased_metadata_overhead_cost(budget, &skill_root_lines);

    Some(AliasPlan {
        aliases: SkillPathAliases { skill_root_lines },
        root_aliases,
        alias_root_by_path,
        table_cost,
    })
}

fn ordered_alias_roots(
    used_roots: &[AbsolutePathBuf],
    alias_root_by_skill_root: &HashMap<AbsolutePathBuf, AbsolutePathBuf>,
) -> Option<Vec<AbsolutePathBuf>> {
    let mut seen = HashSet::new();
    let mut alias_roots = Vec::new();
    for root in used_roots {
        let alias_root = alias_root_by_skill_root.get(root)?.clone();
        if seen.insert(alias_root.clone()) {
            alias_roots.push(alias_root);
        }
    }
    Some(alias_roots)
}

fn alias_root_for_skill_root(
    root: &AbsolutePathBuf,
    plugin_version_skill_counts: &HashMap<AbsolutePathBuf, usize>,
) -> AbsolutePathBuf {
    let Some(plugin_version_base) = plugin_version_base(root.as_path()) else {
        return root.clone();
    };
    let skill_count = plugin_version_skill_counts
        .get(&plugin_version_base)
        .copied()
        .unwrap_or_default();
    if skill_count > 1 {
        root.clone()
    } else {
        plugin_marketplace_base(root.as_path()).unwrap_or_else(|| root.clone())
    }
}

fn plugin_version_skill_counts_for_skill_roots<'a>(
    skill_roots: impl Iterator<Item = &'a AbsolutePathBuf>,
) -> HashMap<AbsolutePathBuf, usize> {
    let mut counts = HashMap::new();
    for root in skill_roots {
        if let Some(plugin_version_base) = plugin_version_base(root.as_path()) {
            let count = counts.entry(plugin_version_base).or_insert(0usize);
            *count = count.saturating_add(1);
        }
    }
    counts
}

fn aliased_metadata_overhead_cost(
    budget: SkillMetadataBudget,
    skill_root_lines: &[String],
) -> usize {
    let empty_skill_lines: &[String] = &[];
    let absolute_body = render_available_skills_body(&[], empty_skill_lines);
    let aliased_body = render_available_skills_body(skill_root_lines, empty_skill_lines);
    budget
        .cost(&aliased_body)
        .saturating_sub(budget.cost(&absolute_body))
}

fn build_skill_root_lines(roots: &[AbsolutePathBuf]) -> Vec<String> {
    roots
        .iter()
        .enumerate()
        .map(|(index, root)| {
            let root_str = root.to_string_lossy().replace('\\', "/");
            format!("- `r{index}` = `{root_str}`")
        })
        .collect()
}

fn plugin_marketplace_base(path: &Path) -> Option<AbsolutePathBuf> {
    let mut candidate = path;
    while let Some(parent) = candidate.parent() {
        if parent.file_name()?.to_str()? == "cache"
            && parent.parent()?.file_name()?.to_str()? == "plugins"
        {
            return AbsolutePathBuf::from_absolute_path(candidate).ok();
        }
        candidate = parent;
    }
    None
}

fn plugin_version_base(path: &Path) -> Option<AbsolutePathBuf> {
    let marketplace_base = plugin_marketplace_base(path)?;
    let mut relative_components = path
        .strip_prefix(marketplace_base.as_path())
        .ok()?
        .components();
    let plugin = match relative_components.next()? {
        Component::Normal(plugin) => plugin,
        _ => return None,
    };
    let version = match relative_components.next()? {
        Component::Normal(version) => version,
        _ => return None,
    };
    AbsolutePathBuf::from_absolute_path(marketplace_base.join(plugin).join(version)).ok()
}

pub(super) fn render_skill_path_with_aliases(skill: &SkillMetadata, plan: &AliasPlan) -> String {
    outcome_relative_skill_path(skill, plan)
        .unwrap_or_else(|| skill.path_to_skills_md.to_string_lossy().replace('\\', "/"))
}

fn outcome_relative_skill_path(skill: &SkillMetadata, plan: &AliasPlan) -> Option<String> {
    let alias_root = plan.alias_root_by_path.get(&skill.path_to_skills_md)?;
    let alias = plan.root_aliases.get(alias_root)?;
    let relative_path = skill
        .path_to_skills_md
        .as_path()
        .strip_prefix(alias_root.as_path())
        .ok()?;
    let relative_path = relative_path.to_string_lossy().replace('\\', "/");
    Some(format!("{alias}/{relative_path}"))
}

pub(super) fn aliased_render_is_better(
    aliased: &AvailableSkills,
    absolute: &AvailableSkills,
    budget: SkillMetadataBudget,
) -> bool {
    if aliased.report.included_count != absolute.report.included_count {
        return aliased.report.included_count > absolute.report.included_count;
    }
    if aliased.report.truncated_description_chars != absolute.report.truncated_description_chars {
        return aliased.report.truncated_description_chars
            < absolute.report.truncated_description_chars;
    }
    available_skills_cost(budget, aliased) < available_skills_cost(budget, absolute)
}

fn available_skills_cost(budget: SkillMetadataBudget, available: &AvailableSkills) -> usize {
    let metadata_cost = if available.skill_root_lines.is_empty() {
        0
    } else {
        aliased_metadata_overhead_cost(budget, &available.skill_root_lines)
    };
    metadata_cost.saturating_add(lines_cost(budget, &available.skill_lines))
}
