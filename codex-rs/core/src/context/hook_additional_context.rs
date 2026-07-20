use std::sync::Mutex;
use std::sync::PoisonError;

use codex_protocol::items::HookPromptFragment;
use codex_protocol::items::build_hook_prompt_message;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_token_count;
use codex_utils_output_truncation::truncate_text;

use super::ContextualUserFragment;

pub(crate) const HOOK_CONTEXT_OMITTED_MESSAGE: &str =
    "Additional hook context was omitted because it exceeded this turn's context limit.";

const HOOK_CONTEXT_ITEM_MAX_TOKENS: usize = 2_500;
const HOOK_CONTEXT_ACCEPTED_TOKENS: usize = 7_800;
const HOOK_CONTEXT_TOTAL_TOKENS: usize = 8_000;
const HOOK_CONTEXT_OMISSION_RESERVE_TOKENS: usize =
    HOOK_CONTEXT_TOTAL_TOKENS - HOOK_CONTEXT_ACCEPTED_TOKENS;
const HOOK_CONTEXT_OMISSION_RUN_ID: &str = "context-limit";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HookContextAdmission {
    Accepted,
    FirstOmitted,
    Omitted,
}

#[derive(Debug, Default)]
pub(crate) struct HookContextBudget {
    state: Mutex<HookContextBudgetState>,
}

#[derive(Debug, Default)]
struct HookContextBudgetState {
    accepted_tokens: usize,
    omission_recorded: bool,
}

impl HookContextBudget {
    pub(crate) fn admit(&self, tokens: usize) -> HookContextAdmission {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.omission_recorded {
            return HookContextAdmission::Omitted;
        }

        let next_tokens = state.accepted_tokens.saturating_add(tokens);
        if next_tokens <= HOOK_CONTEXT_ACCEPTED_TOKENS {
            state.accepted_tokens = next_tokens;
            return HookContextAdmission::Accepted;
        }

        state.accepted_tokens = state
            .accepted_tokens
            .saturating_add(HOOK_CONTEXT_OMISSION_RESERVE_TOKENS);
        state.omission_recorded = true;
        HookContextAdmission::FirstOmitted
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HookAdditionalContext {
    text: String,
}

impl HookAdditionalContext {
    pub(crate) fn new(text: impl Into<String>) -> Self {
        Self {
            text: truncate_text(
                &text.into(),
                TruncationPolicy::Tokens(HOOK_CONTEXT_ITEM_MAX_TOKENS),
            ),
        }
    }

    pub(crate) fn token_count(&self) -> usize {
        approx_token_count(&self.text)
    }
}

impl ContextualUserFragment for HookAdditionalContext {
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
        self.text.clone()
    }
}

pub(crate) fn build_bounded_hook_prompt_message(
    budget: &HookContextBudget,
    fragments: Vec<HookPromptFragment>,
) -> (Option<ResponseItem>, bool) {
    let mut accepted = Vec::new();
    let mut first_omitted = false;

    for mut fragment in fragments {
        fragment.text = truncate_text(
            &fragment.text,
            TruncationPolicy::Tokens(HOOK_CONTEXT_ITEM_MAX_TOKENS),
        );
        let Some(message) = build_hook_prompt_message(std::slice::from_ref(&fragment)) else {
            continue;
        };
        match budget.admit(response_item_token_count(&message)) {
            HookContextAdmission::Accepted => accepted.push(fragment),
            HookContextAdmission::FirstOmitted => {
                accepted.push(HookPromptFragment::from_single_hook(
                    HOOK_CONTEXT_OMITTED_MESSAGE,
                    HOOK_CONTEXT_OMISSION_RUN_ID,
                ));
                first_omitted = true;
            }
            HookContextAdmission::Omitted => {}
        }
    }

    (build_hook_prompt_message(&accepted), first_omitted)
}

fn response_item_token_count(item: &ResponseItem) -> usize {
    let ResponseItem::Message { content, .. } = item else {
        unreachable!("hook prompt builder should return a message");
    };
    content
        .iter()
        .map(|item| match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                approx_token_count(text)
            }
            ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => 0,
        })
        .fold(0usize, usize::saturating_add)
}
