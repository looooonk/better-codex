use codex_utils_string::approx_token_count;
use codex_utils_string::truncate_middle_with_token_budget;

use super::ContextualUserFragment;

pub(crate) const MAX_EXTENSION_CONTEXT_FRAGMENTS: usize = 32;
pub(crate) const MAX_EXTENSION_CONTEXT_TOKENS: usize = 8_000;
const TRUNCATION_MARKER_TOKEN_RESERVE: usize = 32;

pub(crate) struct ExtensionContextFragment {
    role: &'static str,
    markers: (&'static str, &'static str),
    body: String,
}

impl ContextualUserFragment for ExtensionContextFragment {
    fn role(&self) -> &'static str {
        self.role
    }

    fn markers(&self) -> (&'static str, &'static str) {
        self.markers
    }

    fn body(&self) -> String {
        self.body.clone()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("", "")
    }
}

pub(crate) struct ExtensionContextBudget {
    remaining_fragments: usize,
    remaining_tokens: usize,
}

impl Default for ExtensionContextBudget {
    fn default() -> Self {
        Self {
            remaining_fragments: MAX_EXTENSION_CONTEXT_FRAGMENTS,
            remaining_tokens: MAX_EXTENSION_CONTEXT_TOKENS,
        }
    }
}

impl ExtensionContextBudget {
    pub(crate) fn admit(
        &mut self,
        fragment: Box<dyn ContextualUserFragment + Send>,
        expected_role: Option<&str>,
    ) -> Option<ExtensionContextFragment> {
        let role = fragment.role();
        if !matches!(role, "developer" | "user") || expected_role.is_some_and(|value| value != role)
        {
            tracing::warn!(
                role,
                expected_role,
                "dropping extension context with invalid role"
            );
            return None;
        }
        if self.remaining_fragments == 0 || self.remaining_tokens == 0 {
            return None;
        }

        let markers = fragment.markers();
        let body = fragment.body();
        let rendered_tokens = approx_token_count(&format!("{}{body}{}", markers.0, markers.1));
        let body = if rendered_tokens > self.remaining_tokens {
            let marker_tokens = approx_token_count(&format!("{}{}", markers.0, markers.1));
            if marker_tokens >= self.remaining_tokens {
                return None;
            }
            truncate_middle_with_token_budget(
                &body,
                self.remaining_tokens
                    .saturating_sub(marker_tokens)
                    .saturating_sub(TRUNCATION_MARKER_TOKEN_RESERVE),
            )
            .0
        } else {
            body
        };
        let tokens = approx_token_count(&format!("{}{body}{}", markers.0, markers.1));
        if tokens > self.remaining_tokens {
            return None;
        }

        self.remaining_fragments -= 1;
        self.remaining_tokens -= tokens;
        Some(ExtensionContextFragment {
            role,
            markers,
            body,
        })
    }
}

#[cfg(test)]
#[path = "extension_context_tests.rs"]
mod tests;
