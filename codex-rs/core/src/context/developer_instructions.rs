use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;

use super::ContextualUserFragment;

#[cfg(test)]
pub(crate) const DEVELOPER_CONFIGURATION_MAX_TOKENS: usize = 1_000;
const DEVELOPER_CONFIGURATION_TRUNCATION_TOKENS: usize = 950;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeveloperInstructions {
    instructions: String,
}

impl DeveloperInstructions {
    pub(crate) fn new(instructions: impl Into<String>) -> Self {
        Self {
            instructions: bound_developer_configuration_text(&instructions.into()),
        }
    }
}

impl ContextualUserFragment for DeveloperInstructions {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("", "")
    }

    fn body(&self) -> String {
        self.instructions.clone()
    }
}

pub(crate) fn bound_developer_configuration_text(text: &str) -> String {
    truncate_text(
        text,
        TruncationPolicy::Tokens(DEVELOPER_CONFIGURATION_TRUNCATION_TOKENS),
    )
}

#[cfg(test)]
#[path = "developer_instructions_tests.rs"]
mod tests;
