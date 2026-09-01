use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::PoisonError;

use codex_secrets::redact_secrets;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_token_count;
use codex_utils_output_truncation::truncate_text;

const MAX_RETAINED_BYTES: usize = 128 * 1024;
const MAX_RECORDS: usize = 40;
const MAX_TEXT_TOKENS: usize = 1_000;
const MAX_PROVENANCE_BYTES: usize = 128;
const MAX_IMAGES: usize = 2;
const MAX_ENCODED_IMAGE_BYTES: usize = 12 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NodeReplReviewEvidenceItem {
    Text(String),
    Image { data_url: String },
}

#[allow(dead_code, reason = "the stage 6 decision bridge consumes correlated records")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NodeReplReviewEvidenceRecord {
    pub(crate) sequence: u64,
    pub(crate) cell_id: String,
    pub(crate) runtime_tool_call_id: String,
    pub(crate) provenance: String,
    pub(crate) items: Vec<NodeReplReviewEvidenceItem>,
}

impl NodeReplReviewEvidenceRecord {
    fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(self.cell_id.capacity())
            .saturating_add(self.runtime_tool_call_id.capacity())
            .saturating_add(self.provenance.capacity())
            .saturating_add(
                self.items
                    .capacity()
                    .saturating_mul(std::mem::size_of::<NodeReplReviewEvidenceItem>()),
            )
            .saturating_add(self.items.iter().fold(0_usize, |bytes, item| {
                bytes.saturating_add(match item {
                    NodeReplReviewEvidenceItem::Text(text) => text.capacity(),
                    NodeReplReviewEvidenceItem::Image { data_url } => data_url.capacity(),
                })
            }))
    }

    pub(crate) fn has_cell_id(&self, cell_id: &str) -> bool {
        self.cell_id == bounded_identifier(cell_id)
    }
}

#[allow(dead_code, reason = "the stage 6 decision bridge consumes bounded snapshots")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NodeReplReviewEvidenceSnapshot {
    pub(crate) sequence: u64,
    pub(crate) omitted_records: u64,
    pub(crate) records: Vec<NodeReplReviewEvidenceRecord>,
}

#[derive(Debug, Default)]
struct NodeReplReviewEvidenceState {
    records: VecDeque<NodeReplReviewEvidenceRecord>,
    next_sequence: u64,
    retained_bytes: usize,
}

#[derive(Debug, Default)]
pub(crate) struct NodeReplReviewEvidence(Mutex<NodeReplReviewEvidenceState>);

impl NodeReplReviewEvidence {
    pub(crate) fn record(
        &self,
        cell_id: &str,
        runtime_tool_call_id: &str,
        items: Vec<NodeReplReviewEvidenceItem>,
    ) {
        let items = bounded_items(items);
        let bounded_cell_id = bounded_identifier(cell_id);
        let bounded_runtime_tool_call_id = bounded_identifier(runtime_tool_call_id);
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.next_sequence = state.next_sequence.saturating_add(1);
        let record = NodeReplReviewEvidenceRecord {
            sequence: state.next_sequence,
            cell_id: bounded_cell_id,
            runtime_tool_call_id: bounded_runtime_tool_call_id,
            provenance: format!(
                "tool=node_repl/js cell={} call={}",
                bounded_provenance(cell_id),
                bounded_provenance(runtime_tool_call_id)
            ),
            items,
        };
        let retained_bytes = record.retained_bytes();
        while state.records.len() >= MAX_RECORDS
            || state.retained_bytes.saturating_add(retained_bytes) > MAX_RETAINED_BYTES
        {
            let Some(evicted) = state.records.pop_front() else {
                return;
            };
            state.retained_bytes = state
                .retained_bytes
                .saturating_sub(evicted.retained_bytes());
        }
        state.retained_bytes = state.retained_bytes.saturating_add(retained_bytes);
        state.records.push_back(record);
    }

    #[allow(dead_code, reason = "the stage 6 decision bridge consumes bounded snapshots")]
    pub(crate) fn snapshot(&self) -> NodeReplReviewEvidenceSnapshot {
        let state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        NodeReplReviewEvidenceSnapshot {
            sequence: state.next_sequence,
            omitted_records: state
                .next_sequence
                .saturating_sub(u64::try_from(state.records.len()).unwrap_or(u64::MAX)),
            records: state.records.iter().cloned().collect(),
        }
    }
}

fn bounded_items(items: Vec<NodeReplReviewEvidenceItem>) -> Vec<NodeReplReviewEvidenceItem> {
    let mut bounded = Vec::new();
    let mut text_tokens = 0_usize;
    let mut image_count = 0_usize;
    let mut image_bytes = 0_usize;
    for item in items {
        match item {
            NodeReplReviewEvidenceItem::Text(text) => {
                let text = redact_secrets(text).replace("</", "<\\/");
                let remaining = MAX_TEXT_TOKENS.saturating_sub(text_tokens);
                if remaining == 0 || text.trim().is_empty() {
                    continue;
                }
                let mut text = truncate_to_token_budget(&text, remaining);
                text.shrink_to_fit();
                text_tokens = text_tokens.saturating_add(approx_token_count(&text));
                if !text.trim().is_empty() {
                    bounded.push(NodeReplReviewEvidenceItem::Text(text));
                }
            }
            NodeReplReviewEvidenceItem::Image { mut data_url } => {
                let Some(encoded_bytes) = encoded_image_bytes(&data_url) else {
                    continue;
                };
                if image_count >= MAX_IMAGES
                    || image_bytes.saturating_add(encoded_bytes) > MAX_ENCODED_IMAGE_BYTES
                {
                    continue;
                }
                image_count += 1;
                image_bytes = image_bytes.saturating_add(encoded_bytes);
                data_url.shrink_to_fit();
                bounded.push(NodeReplReviewEvidenceItem::Image { data_url });
            }
        }
    }
    bounded.shrink_to_fit();
    bounded
}

fn truncate_to_token_budget(text: &str, budget_tokens: usize) -> String {
    let mut truncation_budget = budget_tokens;
    loop {
        let candidate = truncate_text(text, TruncationPolicy::Tokens(truncation_budget));
        let candidate_tokens = approx_token_count(&candidate);
        if candidate_tokens <= budget_tokens {
            return candidate;
        }

        let excess_tokens = candidate_tokens.saturating_sub(budget_tokens);
        let next_budget = truncation_budget.saturating_sub(excess_tokens.max(1));
        if next_budget == 0 {
            let candidate = truncate_text(text, TruncationPolicy::Tokens(0));
            return (approx_token_count(&candidate) <= budget_tokens)
                .then_some(candidate)
                .unwrap_or_default();
        }
        truncation_budget = next_budget;
    }
}

fn encoded_image_bytes(data_url: &str) -> Option<usize> {
    let (metadata, encoded) = data_url.split_once(',')?;
    (metadata
        .get(..11)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:image/"))
        && metadata
            .split(';')
            .any(|part| part.eq_ignore_ascii_case("base64"))
        && !encoded.is_empty())
    .then_some(encoded.len())
}

fn bounded_provenance(value: &str) -> String {
    let value = redact_secrets(value.to_string())
        .replace(['\n', '\r', '[', ']', '='], "_")
        .replace("</", "<\\/");
    take_bytes(&value, MAX_PROVENANCE_BYTES).to_string()
}

fn bounded_identifier(value: &str) -> String {
    let value = redact_secrets(value.to_string());
    take_bytes(&value, MAX_PROVENANCE_BYTES).to_string()
}

fn take_bytes(value: &str, max_bytes: usize) -> &str {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &value[..end]
}

#[cfg(test)]
#[path = "node_repl_review_evidence_tests.rs"]
mod tests;
