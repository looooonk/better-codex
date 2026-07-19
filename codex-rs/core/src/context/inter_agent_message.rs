use std::sync::Mutex;
use std::sync::PoisonError;

use codex_protocol::models::AgentMessageInputContent;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_token_count;
use codex_utils_output_truncation::truncate_text;

pub(crate) const INTER_AGENT_PAYLOAD_MAX_TOKENS: usize = 7_000;
pub(crate) const INTER_AGENT_TURN_OMITTED_MESSAGE: &str =
    "Additional inter-agent messages were omitted because they exceeded this turn's context limit.";

const INTER_AGENT_CONTENT_TRUNCATION_TOKENS: usize = 7_400;
const INTER_AGENT_MODEL_CONTENT_MAX_TOKENS: usize = 7_500;
const INTER_AGENT_ITEM_OVERHEAD_TOKENS: usize = 100;
const INTER_AGENT_MESSAGES_ACCEPTED_TOKENS: usize = 7_800;
const INTER_AGENT_MESSAGES_TOTAL_TOKENS: usize = 8_000;
const OVERSIZED_ENCRYPTED_PAYLOAD_MESSAGE: &str =
    "An encrypted inter-agent message was omitted because it exceeded the context limit.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InterAgentMessageAdmission {
    Accepted,
    FirstOmitted,
    Omitted,
}

#[derive(Debug, Default)]
pub(crate) struct InterAgentMessageBudget {
    state: Mutex<InterAgentMessageBudgetState>,
}

#[derive(Debug, Default)]
struct InterAgentMessageBudgetState {
    accepted_tokens: usize,
    omission_recorded: bool,
}

impl InterAgentMessageBudget {
    pub(crate) fn admit(&self, content_tokens: usize) -> InterAgentMessageAdmission {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let item_tokens = content_tokens.saturating_add(INTER_AGENT_ITEM_OVERHEAD_TOKENS);
        let next_tokens = state.accepted_tokens.saturating_add(item_tokens);
        if next_tokens <= INTER_AGENT_MESSAGES_ACCEPTED_TOKENS {
            state.accepted_tokens = next_tokens;
            return InterAgentMessageAdmission::Accepted;
        }
        if state.omission_recorded {
            return InterAgentMessageAdmission::Omitted;
        }

        let omission_tokens = approx_token_count(INTER_AGENT_TURN_OMITTED_MESSAGE)
            .saturating_add(INTER_AGENT_ITEM_OVERHEAD_TOKENS);
        if state.accepted_tokens.saturating_add(omission_tokens) > INTER_AGENT_MESSAGES_TOTAL_TOKENS
        {
            return InterAgentMessageAdmission::Omitted;
        }
        state.accepted_tokens = state.accepted_tokens.saturating_add(omission_tokens);
        state.omission_recorded = true;
        InterAgentMessageAdmission::FirstOmitted
    }
}

pub(crate) fn bound_inter_agent_model_content(
    content: &mut Vec<AgentMessageInputContent>,
) -> usize {
    let content_tokens = content
        .iter()
        .map(|part| match part {
            AgentMessageInputContent::InputText { text } => approx_token_count(text),
            AgentMessageInputContent::EncryptedContent { encrypted_content } => {
                approx_token_count(encrypted_content)
            }
        })
        .fold(0usize, usize::saturating_add);
    if content_tokens <= INTER_AGENT_MODEL_CONTENT_MAX_TOKENS {
        return content_tokens;
    }

    if content
        .iter()
        .any(|part| matches!(part, AgentMessageInputContent::EncryptedContent { .. }))
    {
        *content = vec![AgentMessageInputContent::InputText {
            text: OVERSIZED_ENCRYPTED_PAYLOAD_MESSAGE.to_string(),
        }];
        return approx_token_count(OVERSIZED_ENCRYPTED_PAYLOAD_MESSAGE);
    }

    let text = content
        .iter()
        .map(|part| match part {
            AgentMessageInputContent::InputText { text } => text.as_str(),
            AgentMessageInputContent::EncryptedContent { .. } => unreachable!(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let text = truncate_text(
        &text,
        TruncationPolicy::Tokens(INTER_AGENT_CONTENT_TRUNCATION_TOKENS),
    );
    let tokens = approx_token_count(&text);
    *content = vec![AgentMessageInputContent::InputText { text }];
    tokens
}
