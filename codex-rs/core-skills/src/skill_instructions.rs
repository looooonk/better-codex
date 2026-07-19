use codex_context_fragments::ContextualUserFragment;

use crate::injection::SkillInjection;

pub const MAX_EXPLICIT_SKILL_PROMPT_BYTES: usize = 3_600;
pub const MAX_EXPLICIT_SKILL_PROMPTS_TOTAL_BYTES: usize = 32_000;

const MAX_SKILL_NAME_BYTES: usize = 256;
const MAX_SKILL_PATH_BYTES: usize = 1_024;
const TRUNCATION_SUFFIX: &str = "\n\n[skill prompt truncated]";

#[derive(Debug, Clone, PartialEq)]
pub struct SkillInstructions {
    name: String,
    path: String,
    contents: String,
}

impl SkillInstructions {
    pub fn bounded(name: &str, path: &str, contents: &str) -> (Self, bool) {
        let (name, name_truncated) = truncate_utf8_to_bytes(name, MAX_SKILL_NAME_BYTES);
        let (path, path_truncated) = truncate_utf8_to_bytes(path, MAX_SKILL_PATH_BYTES);
        let empty = Self {
            name,
            path,
            contents: String::new(),
        };
        let max_contents_bytes = MAX_EXPLICIT_SKILL_PROMPT_BYTES
            .saturating_sub(empty.render().len())
            .saturating_sub(TRUNCATION_SUFFIX.len());
        let (mut contents, contents_truncated) =
            truncate_utf8_to_bytes(contents, max_contents_bytes);
        if contents_truncated {
            contents.push_str(TRUNCATION_SUFFIX);
        }

        (
            Self { contents, ..empty },
            name_truncated || path_truncated || contents_truncated,
        )
    }

    pub fn rendered_bytes(&self) -> usize {
        self.render().len()
    }
}

impl From<&SkillInjection> for SkillInstructions {
    fn from(skill: &SkillInjection) -> Self {
        Self::bounded(&skill.name, &skill.path, &skill.contents).0
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
        format!(
            "\n<name>{}</name>\n<path>{}</path>\n{}\n",
            self.name, self.path, self.contents
        )
    }
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
