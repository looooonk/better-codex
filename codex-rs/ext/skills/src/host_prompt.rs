use std::collections::HashSet;

use codex_extension_api::ContextualUserFragment;
use codex_core_skills::HostSkillsSnapshot;
use codex_skills::SkillMetadata;
use codex_skills::normalize_skill_path;

use crate::fragments::SkillInstructions;

pub const MAX_EXPLICIT_SKILL_PROMPT_BYTES: usize = 3_600;
pub const MAX_EXPLICIT_SKILL_PROMPTS_TOTAL_BYTES: usize = 32_000;

/// Host skill prompts already supplied or superseded by an extension.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InjectedHostSkillPrompts {
    paths: HashSet<String>,
}

impl InjectedHostSkillPrompts {
    pub fn insert_path(&mut self, path: impl Into<String>) {
        let path = path.into();
        self.paths.insert(normalize_host_skill_path(&path));
        self.paths.insert(path);
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn contains_path(&self, path: &str) -> bool {
        self.paths.contains(path) || self.paths.contains(&normalize_host_skill_path(path))
    }
}

/// Prompt fragments and read outcomes for selected host skills.
pub struct HostSkillPrompts {
    pub fragments: Vec<Box<dyn ContextualUserFragment + Send>>,
    pub injected: Vec<SkillMetadata>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExplicitSkillPromptBudget {
    used_bytes: usize,
}

impl ExplicitSkillPromptBudget {
    pub fn try_reserve(&mut self, bytes: usize) -> bool {
        let Some(next_used_bytes) = self.used_bytes.checked_add(bytes) else {
            return false;
        };
        if next_used_bytes > MAX_EXPLICIT_SKILL_PROMPTS_TOTAL_BYTES {
            return false;
        }
        self.used_bytes = next_used_bytes;
        true
    }
}

fn normalize_host_skill_path(path: &str) -> String {
    normalize_skill_path(path).replace('\\', "/")
}

impl HostSkillsSnapshot {
    /// Reads selected host skills and builds bounded model-visible prompt fragments.
    #[tracing::instrument(
        level = "trace",
        skip_all,
        fields(selected_skill_count = selected_skills.len())
    )]
    pub async fn load_skill_prompts(&self, selected_skills: &[SkillMetadata]) -> HostSkillPrompts {
        let mut prompts = HostSkillPrompts {
            fragments: Vec::with_capacity(selected_skills.len()),
            injected: Vec::with_capacity(selected_skills.len()),
            warnings: Vec::new(),
        };

        for skill in selected_skills {
            match self.read_skill_text(skill).await {
                Ok(contents) => {
                    let path = skill.path_to_skills_md.to_string_lossy();
                    let Some((fragment, truncated)) =
                        SkillInstructions::bounded(&skill.name, &path, &contents, None)
                    else {
                        prompts.warnings.push(format!(
                            "Skill `{}` was omitted because its instructions could not fit within the main prompt context limit.",
                            skill.name
                        ));
                        continue;
                    };
                    if truncated {
                        prompts.warnings.push(format!(
                            "Skill `{}` exceeded the main prompt context limit and was truncated.",
                            skill.name
                        ));
                    }
                    prompts.fragments.push(Box::new(fragment));
                    prompts.injected.push(skill.clone());
                }
                Err(err) => prompts.warnings.push(format!(
                    "Failed to load skill {} at {}: {err:#}",
                    skill.name,
                    skill.path_to_skills_md.display()
                )),
            }
        }

        prompts
    }
}
