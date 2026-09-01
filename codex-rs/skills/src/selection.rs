use std::collections::HashMap;
use std::collections::HashSet;

use codex_protocol::user_input::UserInput;
use codex_utils_absolute_path::AbsolutePathBuf;

use crate::SkillMetadata;
use crate::ToolMentionKind;
use crate::ToolMentions;
use crate::build_skill_name_counts;
use crate::extract_tool_mentions;
use crate::normalize_skill_path;
use crate::tool_kind_for_path;

/// Supplies ordered skills, disabled identities, and discovery paths for explicit selection.
///
/// Implementations must preserve discovery order and map canonical skill identities to the
/// logical paths shown to callers when those paths differ.
pub trait ExplicitSkillLookup {
    fn skills(&self) -> &[SkillMetadata];

    fn disabled_paths(&self) -> &HashSet<AbsolutePathBuf>;

    fn skill_discovery_path_for_path(&self, path: &AbsolutePathBuf) -> Option<&AbsolutePathBuf>;

    fn is_skill_enabled(&self, skill: &SkillMetadata) -> bool {
        !self.disabled_paths().contains(&skill.path_to_skills_md)
    }
}

/// Collects explicit structured and textual mentions while preserving skill discovery order.
pub fn collect_explicit_skill_mentions(
    inputs: &[UserInput],
    loaded_skills: &impl ExplicitSkillLookup,
    connector_slug_counts: &HashMap<String, usize>,
) -> Vec<SkillMetadata> {
    let skill_name_counts =
        build_skill_name_counts(loaded_skills.skills(), loaded_skills.disabled_paths()).0;
    let context = SkillSelectionContext {
        loaded_skills,
        skill_name_counts: &skill_name_counts,
        connector_slug_counts,
    };
    let mut selected = Vec::new();
    let mut seen_names = HashSet::new();
    let mut seen_paths = HashSet::new();
    let mut blocked_plain_names = HashSet::new();

    for input in inputs {
        if let UserInput::Skill { name, path, .. } = input {
            blocked_plain_names.insert(name.clone());
            let Ok(path) = AbsolutePathBuf::relative_to_current_dir(path) else {
                continue;
            };
            let Some(skill) = context.loaded_skills.skills().iter().find(|skill| {
                skill.path_to_skills_md == path
                    || context
                        .loaded_skills
                        .skill_discovery_path_for_path(&skill.path_to_skills_md)
                        .is_some_and(|discovery_path| discovery_path == &path)
            }) else {
                continue;
            };
            if !context.loaded_skills.is_skill_enabled(skill)
                || seen_paths.contains(&skill.path_to_skills_md)
            {
                continue;
            }
            seen_paths.insert(skill.path_to_skills_md.clone());
            seen_names.insert(skill.name.clone());
            selected.push(skill.clone());
        }
    }

    for input in inputs {
        if let UserInput::Text { text, .. } = input {
            select_skills_from_mentions(
                &context,
                &blocked_plain_names,
                &extract_tool_mentions(text),
                &mut seen_names,
                &mut seen_paths,
                &mut selected,
            );
        }
    }
    selected
}

struct SkillSelectionContext<'a> {
    loaded_skills: &'a dyn ExplicitSkillLookup,
    skill_name_counts: &'a HashMap<String, usize>,
    connector_slug_counts: &'a HashMap<String, usize>,
}

fn select_skills_from_mentions(
    context: &SkillSelectionContext<'_>,
    blocked_plain_names: &HashSet<String>,
    mentions: &ToolMentions<'_>,
    seen_names: &mut HashSet<String>,
    seen_paths: &mut HashSet<AbsolutePathBuf>,
    selected: &mut Vec<SkillMetadata>,
) {
    if mentions.is_empty() {
        return;
    }
    let mentioned_paths = mentions
        .paths()
        .filter(|path| {
            !matches!(
                tool_kind_for_path(path),
                ToolMentionKind::App | ToolMentionKind::Mcp | ToolMentionKind::Plugin
            )
        })
        .map(normalize_host_skill_path)
        .collect::<HashSet<_>>();

    for skill in context.loaded_skills.skills() {
        if !context.loaded_skills.is_skill_enabled(skill)
            || seen_paths.contains(&skill.path_to_skills_md)
        {
            continue;
        }
        let canonical_path = normalize_host_skill_path(&skill.path_to_skills_md.to_string_lossy());
        let matches_discovery_path = context
            .loaded_skills
            .skill_discovery_path_for_path(&skill.path_to_skills_md)
            .is_some_and(|path| {
                mentioned_paths.contains(&normalize_host_skill_path(&path.to_string_lossy()))
            });
        if mentioned_paths.contains(&canonical_path) || matches_discovery_path {
            seen_paths.insert(skill.path_to_skills_md.clone());
            seen_names.insert(skill.name.clone());
            selected.push(skill.clone());
        }
    }

    for skill in context.loaded_skills.skills() {
        if !context.loaded_skills.is_skill_enabled(skill)
            || seen_paths.contains(&skill.path_to_skills_md)
            || blocked_plain_names.contains(&skill.name)
            || !mentions.contains_plain_name(&skill.name)
        {
            continue;
        }
        let skill_count = context
            .skill_name_counts
            .get(&skill.name)
            .copied()
            .unwrap_or(0);
        let connector_count = context
            .connector_slug_counts
            .get(&skill.name.to_ascii_lowercase())
            .copied()
            .unwrap_or(0);
        if skill_count == 1 && connector_count == 0 && seen_names.insert(skill.name.clone()) {
            seen_paths.insert(skill.path_to_skills_md.clone());
            selected.push(skill.clone());
        }
    }
}

fn normalize_host_skill_path(path: &str) -> String {
    normalize_skill_path(path).replace('\\', "/")
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
