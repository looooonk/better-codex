use crate::host_render::AvailableSkills;
use crate::host_render::SKILLS_HOW_TO_USE_WITH_ABSOLUTE_PATHS;
use crate::host_render::SKILLS_HOW_TO_USE_WITH_ALIASES;
use crate::host_render::render_available_skills_body as render_legacy_available_skills_body;
use codex_extension_api::ContextualUserFragment;
use codex_protocol::protocol::SKILLS_INSTRUCTIONS_CLOSE_TAG;
use codex_protocol::protocol::SKILLS_INSTRUCTIONS_OPEN_TAG;
use serde::Serialize;

use crate::catalog_prompt::HOST_ALIAS_INSTRUCTIONS;
use crate::catalog_prompt::RESOURCE_ALIAS_INSTRUCTIONS;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SkillsUsage {
    Omit,
    Catalog,
    Combined {
        resource_aliases: bool,
        host_aliases: bool,
    },
}

const EXECUTOR_SKILLS_SECTION_OPEN_TAG: &str = "<skills_section source=\"executor\">";
const ORCHESTRATOR_SKILLS_SECTION_OPEN_TAG: &str = "<skills_section source=\"orchestrator\">";
const HOST_SKILLS_SECTION_OPEN_TAG: &str = "<skills_section source=\"host\">";
const SKILLS_SECTION_CLOSE_TAG: &str = "</skills_section>";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SkillsUpdateSection {
    Executor,
    Orchestrator,
    Host,
}

impl SkillsUpdateSection {
    fn open_tag(self) -> &'static str {
        match self {
            Self::Executor => EXECUTOR_SKILLS_SECTION_OPEN_TAG,
            Self::Orchestrator => ORCHESTRATOR_SKILLS_SECTION_OPEN_TAG,
            Self::Host => HOST_SKILLS_SECTION_OPEN_TAG,
        }
    }

    pub(crate) fn from_rendered_fragment(role: &str, text: &str) -> Option<Self> {
        if role != "developer" {
            return None;
        }
        let text = text.trim();
        let body = text
            .strip_prefix(SKILLS_INSTRUCTIONS_OPEN_TAG)?
            .strip_suffix(SKILLS_INSTRUCTIONS_CLOSE_TAG)?;
        [Self::Executor, Self::Orchestrator, Self::Host]
            .into_iter()
            .find(|section| {
                body.starts_with(section.open_tag()) && body.ends_with(SKILLS_SECTION_CLOSE_TAG)
            })
    }
}

pub(crate) struct SkillsUpdate {
    section: SkillsUpdateSection,
    body: String,
}

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
    pub(crate) fn new(section: SkillsUpdateSection, body: impl Into<String>) -> Self {
        Self {
            section,
            body: body.into(),
        }
    }

    pub(crate) fn rendered_text(section: SkillsUpdateSection, body: &str) -> String {
        let section_open_tag = section.open_tag();
        format!(
            "{SKILLS_INSTRUCTIONS_OPEN_TAG}{section_open_tag}{body}{SKILLS_SECTION_CLOSE_TAG}{SKILLS_INSTRUCTIONS_CLOSE_TAG}"
        )
    }
}

impl AvailableSkillsInstructions {
    pub(crate) fn from_skill_lines(
        prompt_kind: SkillPromptKind,
        skill_root_lines: Vec<String>,
        mut skill_lines: Vec<String>,
        usage: SkillsUsage,
    ) -> Self {
        match usage {
            SkillsUsage::Omit => {}
            SkillsUsage::Catalog => append_catalog_usage(&mut skill_lines, prompt_kind),
            SkillsUsage::Combined {
                resource_aliases,
                host_aliases,
            } => {
                skill_lines.push("### How to use skills".to_string());
                if resource_aliases {
                    skill_lines.push(RESOURCE_ALIAS_INSTRUCTIONS.to_string());
                }
                if host_aliases {
                    skill_lines.push(HOST_ALIAS_INSTRUCTIONS.to_string());
                }
                skill_lines.push(SkillPromptKind::Unaliased.usage_instructions().to_string());
            }
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

fn append_catalog_usage(skill_lines: &mut Vec<String>, prompt_kind: SkillPromptKind) {
    skill_lines.push("### How to use skills".to_string());
    if let Some(instructions) = prompt_kind.alias_instructions() {
        skill_lines.push(instructions.to_string());
    }
    skill_lines.push(prompt_kind.usage_instructions().to_string());
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
        self.body.clone()
    }

    fn render(&self) -> String {
        Self::rendered_text(self.section, &self.body)
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
