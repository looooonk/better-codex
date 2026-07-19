use std::collections::BTreeMap;

use crate::context::AdditionalContextDeveloperFragment;
use crate::context::AdditionalContextUserFragment;
use crate::context::ContextualUserFragment;
use crate::context::MAX_ADDITIONAL_CONTEXT_ITEMS;
use crate::context::MAX_ADDITIONAL_CONTEXT_TOTAL_TOKENS;
use crate::context::is_valid_additional_context_key;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::AdditionalContextEntry;
use codex_protocol::protocol::AdditionalContextKind;
use codex_utils_output_truncation::approx_token_count;

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredAdditionalContextEntry {
    text: String,
    kind: AdditionalContextKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AdditionalContextStore {
    values: BTreeMap<String, StoredAdditionalContextEntry>,
}

impl AdditionalContextStore {
    pub(crate) fn merge(
        &mut self,
        values: BTreeMap<String, AdditionalContextEntry>,
    ) -> Vec<ResponseInputItem> {
        let mut bounded_values = BTreeMap::new();
        let mut fragments = Vec::new();
        let mut total_tokens = 0usize;

        for (key, entry) in values {
            if bounded_values.len() == MAX_ADDITIONAL_CONTEXT_ITEMS {
                break;
            }
            if !is_valid_additional_context_key(&key) {
                continue;
            }

            let text = match entry.kind {
                AdditionalContextKind::Untrusted => {
                    AdditionalContextUserFragment::new(key.clone(), entry.value).render()
                }
                AdditionalContextKind::Application => {
                    AdditionalContextDeveloperFragment::new(key.clone(), entry.value).render()
                }
            };
            let tokens = approx_token_count(&text);
            if total_tokens.saturating_add(tokens) > MAX_ADDITIONAL_CONTEXT_TOTAL_TOKENS {
                continue;
            }
            total_tokens = total_tokens.saturating_add(tokens);

            let stored = StoredAdditionalContextEntry {
                text: text.clone(),
                kind: entry.kind,
            };
            if self.values.get(&key) != Some(&stored) {
                fragments.push(ResponseInputItem::Message {
                    role: match entry.kind {
                        AdditionalContextKind::Untrusted => "user",
                        AdditionalContextKind::Application => "developer",
                    }
                    .to_string(),
                    content: vec![ContentItem::InputText { text }],
                    phase: None,
                });
            }
            bounded_values.insert(key, stored);
        }

        self.values = bounded_values;
        fragments
    }
}

#[cfg(test)]
#[path = "additional_context_tests.rs"]
mod tests;
