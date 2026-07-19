use super::ContextualUserFragment;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;

const USER_INSTRUCTIONS_BODY_MAX_TOKENS: usize = 8_000;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UserInstructions {
    pub(crate) directory: Option<String>,
    pub(crate) text: String,
}

impl ContextualUserFragment for UserInstructions {
    fn role(&self) -> &'static str {
        "user"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("# AGENTS.md instructions", "</INSTRUCTIONS>")
    }

    fn body(&self) -> String {
        let directory = self
            .directory
            .as_ref()
            .map(|directory| format!(" for {directory}"))
            .unwrap_or_default();
        let body = format!("{directory}\n\n<INSTRUCTIONS>\n{}\n", self.text);
        truncate_text(
            &body,
            TruncationPolicy::Tokens(USER_INSTRUCTIONS_BODY_MAX_TOKENS),
        )
    }
}
