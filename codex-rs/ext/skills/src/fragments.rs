use crate::host_render::AvailableSkills;
use crate::host_render::SKILLS_HOW_TO_USE_WITH_ABSOLUTE_PATHS;
use crate::host_render::SKILLS_HOW_TO_USE_WITH_ALIASES;
use crate::host_render::render_available_skills_body as render_legacy_available_skills_body;
use codex_extension_api::ContextualUserFragment;
use codex_protocol::protocol::SKILLS_INSTRUCTIONS_CLOSE_TAG;
use codex_protocol::protocol::SKILLS_INSTRUCTIONS_OPEN_TAG;
use serde::Serialize;

use crate::catalog_prompt::SkillPromptKind;
use crate::catalog_prompt::render_available_skills_body;
use crate::host_prompt::MAX_EXPLICIT_SKILL_PROMPT_BYTES;
use crate::tools::SkillToolAuthority;
const MAX_SKILL_NAME_BYTES: usize = 256;
const MAX_SKILL_PATH_BYTES: usize = 1_024;
const TRUNCATION_SUFFIX: &str = "\n\n[skill prompt truncated]";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvailableSkillsInstructions {
    prompt_kind: Option<SkillPromptKind>,
    skill_root_lines: Vec<String>,
    skill_lines: Vec<String>,
}

pub(crate) struct SkillsUpdate(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SkillInstructions {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) contents: String,
    pub(crate) resource_access: Option<SkillResourceAccess>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SkillResourceAccess {
    pub(crate) authority: SkillToolAuthority,
    pub(crate) package: String,
    pub(crate) main_resource: String,
}

impl SkillsUpdate {
    pub(crate) fn new(body: impl Into<String>) -> Self {
        Self(body.into())
    }
}

impl AvailableSkillsInstructions {
    pub(crate) fn from_skill_lines(
        prompt_kind: SkillPromptKind,
        skill_root_lines: Vec<String>,
        mut skill_lines: Vec<String>,
        include_skills_usage_instructions: bool,
    ) -> Self {
        if include_skills_usage_instructions {
            skill_lines.push("### How to use skills".to_string());
            if let Some(instructions) = prompt_kind.alias_instructions() {
                skill_lines.push(instructions.to_string());
            }
            skill_lines.push(prompt_kind.usage_instructions().to_string());
        }
        Self {
            prompt_kind: Some(prompt_kind),
            skill_root_lines,
            skill_lines,
        }
    }

    pub(crate) fn from_available_skills(
        available: AvailableSkills,
        include_skills_usage_instructions: bool,
    ) -> Self {
        let mut skill_lines = available.skill_lines;
        if include_skills_usage_instructions {
            skill_lines.push("### How to use skills".to_string());
            let instructions = if available.skill_root_lines.is_empty() {
                SKILLS_HOW_TO_USE_WITH_ABSOLUTE_PATHS
            } else {
                SKILLS_HOW_TO_USE_WITH_ALIASES
            };
            skill_lines.push(instructions.to_string());
        }
        Self {
            prompt_kind: None,
            skill_root_lines: available.skill_root_lines,
            skill_lines,
        }
    }
}

impl ContextualUserFragment for AvailableSkillsInstructions {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (SKILLS_INSTRUCTIONS_OPEN_TAG, SKILLS_INSTRUCTIONS_CLOSE_TAG)
    }

    fn body(&self) -> String {
        match self.prompt_kind {
            Some(prompt_kind) => {
                render_available_skills_body(prompt_kind, &self.skill_root_lines, &self.skill_lines)
            }
            None => render_legacy_available_skills_body(&self.skill_root_lines, &self.skill_lines),
        }
    }
}

impl ContextualUserFragment for SkillsUpdate {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (SKILLS_INSTRUCTIONS_OPEN_TAG, SKILLS_INSTRUCTIONS_CLOSE_TAG)
    }

    fn body(&self) -> String {
        self.0.clone()
    }
}

impl SkillInstructions {
    pub(crate) fn bounded(
        name: &str,
        path: &str,
        contents: &str,
        resource_access: Option<SkillResourceAccess>,
    ) -> Option<(Self, bool)> {
        let (name, name_truncated) = truncate_utf8_to_bytes(name, MAX_SKILL_NAME_BYTES);
        let (path, path_truncated) = truncate_utf8_to_bytes(path, MAX_SKILL_PATH_BYTES);
        let empty = Self {
            name,
            path,
            contents: String::new(),
            resource_access,
        };
        let max_contents_bytes = MAX_EXPLICIT_SKILL_PROMPT_BYTES
            .checked_sub(empty.render().len())?
            .saturating_sub(TRUNCATION_SUFFIX.len());
        let (mut contents, contents_truncated) =
            truncate_utf8_to_bytes(contents, max_contents_bytes);
        if contents_truncated {
            contents.push_str(TRUNCATION_SUFFIX);
        }
        let instructions = Self { contents, ..empty };
        (instructions.rendered_bytes() <= MAX_EXPLICIT_SKILL_PROMPT_BYTES).then_some((
            instructions,
            name_truncated || path_truncated || contents_truncated,
        ))
    }

    pub(crate) fn rendered_bytes(&self) -> usize {
        self.render().len()
    }
}

impl ContextualUserFragment for SkillInstructions {
    fn role(&self) -> &'static str {
        "user"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<skill>", "</skill>")
    }

    fn body(&self) -> String {
        let resource_access = self
            .resource_access
            .as_ref()
            .map(|access| {
                let metadata = serde_json::to_string(access)
                    .expect("skill resource access should always serialize");
                format!("\n<resource_access>{metadata}</resource_access>")
            })
            .unwrap_or_default();
        format!(
            "\n<name>{}</name>\n<path>{}</path>{resource_access}\n{}\n",
            self.name, self.path, self.contents
        )
    }
}

fn truncate_utf8_to_bytes(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    let mut end = max_bytes.min(value.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    (value[..end].to_string(), true)
}

#[cfg(test)]
#[path = "fragments_tests.rs"]
mod tests;
