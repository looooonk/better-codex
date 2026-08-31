use std::sync::Arc;
use std::time::Duration;

use codex_protocol::protocol::GuardianAssessmentOutcome;
use codex_protocol::protocol::GuardianRiskLevel;
use codex_protocol::protocol::GuardianUserAuthorization;
use codex_secrets::redact_secrets;
use serde::Deserialize;
use tokio::time::Instant;

use crate::request::GuardianReviewError;
use crate::request::GuardianReviewRequest;
use crate::request::SamplingAttribution;
use crate::request::build_sampling_request;
use crate::sampler::LunaSampler;
use crate::sampler::LunaSamplingRequest;

const DEFAULT_REVIEW_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT_BYTES: usize = 4 * 1024;
const MAX_RATIONALE_BYTES: usize = 4 * 1024;

/// Strict structured result returned by the Luna reviewer.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GuardianReviewOutcome {
    pub score: f64,
    pub risk_level: GuardianRiskLevel,
    pub user_authorization: GuardianUserAuthorization,
    pub outcome: GuardianAssessmentOutcome,
    pub rationale: String,
}

/// Explicit shadow-review client. It is not installed as an execution contributor.
pub struct GuardianReviewClient {
    sampler: Arc<LunaSampler>,
    review_timeout: Duration,
}

impl GuardianReviewClient {
    pub fn new(sampler: Arc<LunaSampler>) -> Self {
        Self {
            sampler,
            review_timeout: DEFAULT_REVIEW_TIMEOUT,
        }
    }

    pub fn with_review_timeout(mut self, review_timeout: Duration) -> Self {
        self.review_timeout = review_timeout;
        self
    }

    /// Runs a non-authoritative review under one end-to-end deadline.
    pub async fn review(
        &self,
        request: GuardianReviewRequest,
    ) -> Result<GuardianReviewOutcome, GuardianReviewError> {
        let deadline = Instant::now() + self.review_timeout;
        tokio::time::timeout_at(deadline, async {
            let attribution = SamplingAttribution {
                client_metadata: self.sampler.client_metadata(&request.action.turn_id),
                service_tier: self.sampler.service_tier(),
                thread_id: self.sampler.thread_id().to_string(),
            };
            let request = build_sampling_request(attribution, request)?;
            let output = self
                .sampler
                .sample(LunaSamplingRequest { request, deadline })
                .await?;
            parse_outcome(&output)
        })
        .await
        .map_err(|_| GuardianReviewError::Deadline)?
    }
}

fn parse_outcome(output: &str) -> Result<GuardianReviewOutcome, GuardianReviewError> {
    if output.len() > MAX_OUTPUT_BYTES {
        return Err(GuardianReviewError::InvalidOutput);
    }
    let mut outcome = serde_json::from_str::<GuardianReviewOutcome>(output)
        .map_err(|_| GuardianReviewError::InvalidOutput)?;
    if !outcome.score.is_finite()
        || !(0.0..=1.0).contains(&outcome.score)
        || outcome.rationale.len() > MAX_RATIONALE_BYTES
    {
        return Err(GuardianReviewError::InvalidOutput);
    }
    let rationale = redact_secrets(outcome.rationale);
    outcome.rationale = take_bytes(&rationale, MAX_RATIONALE_BYTES).to_string();
    Ok(outcome)
}

fn take_bytes(value: &str, max_bytes: usize) -> &str {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &value[..end]
}

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;
