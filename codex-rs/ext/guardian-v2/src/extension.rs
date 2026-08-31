use std::marker::PhantomData;
use std::sync::Arc;

use codex_extension_api::ApprovalReviewContributor;
use codex_extension_api::ApprovalReviewEvidence;
use codex_extension_api::ApprovalReviewFailure;
use codex_extension_api::ApprovalReviewInput;
use codex_extension_api::ApprovalReviewOutcome;
use codex_extension_api::ApprovalReviewResult;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_protocol::protocol::GuardianAssessmentOutcome;

use crate::GuardianEvidenceEntry;
use crate::GuardianReviewAction;
use crate::GuardianReviewClient;
use crate::GuardianReviewError;
use crate::GuardianReviewImage;
use crate::GuardianReviewRequest;
use crate::LunaSampler;
use crate::LunaSamplerConfig;

/// Host inputs used to configure the reviewer for one thread.
pub struct GuardianV2ThreadConfigInput<'a, C> {
    pub config: &'a C,
    pub session_source: &'a codex_protocol::protocol::SessionSource,
    pub session_id: &'a str,
    pub thread_id: &'a str,
    pub originator: &'a str,
}

enum GuardianV2ThreadState {
    Ready(Arc<GuardianReviewClient>),
    Unavailable,
}

struct GuardianV2Extension<C, F> {
    config: F,
    marker: PhantomData<fn() -> C>,
}

impl<C, F> GuardianV2Extension<C, F> {
    fn new(config: F) -> Self {
        Self {
            config,
            marker: PhantomData,
        }
    }
}

impl<C, F> ThreadLifecycleContributor<C> for GuardianV2Extension<C, F>
where
    C: Sync,
    F: for<'a> Fn(GuardianV2ThreadConfigInput<'a, C>) -> Option<LunaSamplerConfig> + Send + Sync,
{
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, C>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(config) = (self.config)(GuardianV2ThreadConfigInput {
                config: input.config,
                session_source: input.session_source,
                session_id: input.session_store.level_id(),
                thread_id: input.thread_store.level_id(),
                originator: input.originator,
            }) else {
                return;
            };
            let state = match LunaSampler::connect(config).await {
                Ok(sampler) => GuardianV2ThreadState::Ready(Arc::new(GuardianReviewClient::new(
                    Arc::new(sampler),
                ))),
                Err(error) => {
                    tracing::warn!(%error, "Guardian V2 sampler is unavailable");
                    GuardianV2ThreadState::Unavailable
                }
            };
            input.thread_store.insert(state);
        })
    }
}

impl<C, F> ApprovalReviewContributor for GuardianV2Extension<C, F>
where
    C: Sync,
    F: for<'a> Fn(GuardianV2ThreadConfigInput<'a, C>) -> Option<LunaSamplerConfig> + Send + Sync,
{
    fn review<'a>(
        &'a self,
        _session_store: &'a codex_extension_api::ExtensionData,
        thread_store: &'a codex_extension_api::ExtensionData,
        input: ApprovalReviewInput,
    ) -> ExtensionFuture<'a, ApprovalReviewResult> {
        Box::pin(async move {
            if input.binding.thread_id != thread_store.level_id() {
                return ApprovalReviewResult::ManualReview(ApprovalReviewFailure::InvalidInput);
            }
            let Some(state) = thread_store.get::<GuardianV2ThreadState>() else {
                return ApprovalReviewResult::ManualReview(ApprovalReviewFailure::NotInstalled);
            };
            let GuardianV2ThreadState::Ready(client) = state.as_ref() else {
                return ApprovalReviewResult::ManualReview(
                    ApprovalReviewFailure::SamplerUnavailable,
                );
            };
            let ApprovalReviewInput {
                binding,
                action,
                history,
                evidence,
                images,
                deadline,
                cancellation,
            } = input;
            let images = images
                .into_iter()
                .map(|image| GuardianReviewImage::from_sanitized_data_url(image.data_url))
                .collect::<Result<Vec<_>, _>>();
            let Ok(images) = images else {
                return ApprovalReviewResult::ManualReview(ApprovalReviewFailure::InvalidInput);
            };
            let request = GuardianReviewRequest {
                action: GuardianReviewAction {
                    review_id: binding.attempt_id,
                    turn_id: binding.turn_id,
                    action_id: binding.action_id,
                    action: action.assessment_action(),
                    request_payload: action.request_payload(),
                },
                history,
                evidence: evidence.into_iter().map(review_evidence).collect(),
                images,
            };
            let review = client.review_before(request, deadline);
            tokio::pin!(review);
            let outcome = tokio::select! {
                _ = cancellation.cancelled() => return ApprovalReviewResult::Cancelled,
                outcome = &mut review => outcome,
            };
            match outcome {
                Ok(outcome) => {
                    let result = ApprovalReviewOutcome {
                        risk_level: outcome.risk_level,
                        user_authorization: outcome.user_authorization,
                        rationale: outcome.rationale,
                    };
                    match outcome.outcome {
                        GuardianAssessmentOutcome::Allow => ApprovalReviewResult::Allow(result),
                        GuardianAssessmentOutcome::Deny => ApprovalReviewResult::Deny(result),
                    }
                }
                Err(error) => ApprovalReviewResult::ManualReview(review_failure(&error)),
            }
        })
    }
}

fn review_evidence(evidence: ApprovalReviewEvidence) -> GuardianEvidenceEntry {
    GuardianEvidenceEntry {
        kind: evidence.kind,
        provenance: evidence.provenance,
        text: evidence.text,
    }
}

fn review_failure(error: &GuardianReviewError) -> ApprovalReviewFailure {
    match error {
        GuardianReviewError::ActionTooLarge => ApprovalReviewFailure::ActionTooLarge,
        GuardianReviewError::RequestTooLarge => ApprovalReviewFailure::RequestTooLarge,
        GuardianReviewError::InvalidIdentifier | GuardianReviewError::InvalidImage => {
            ApprovalReviewFailure::InvalidInput
        }
        GuardianReviewError::Serialization(_) => ApprovalReviewFailure::InvalidInput,
        GuardianReviewError::Sampler(_) => ApprovalReviewFailure::SamplerUnavailable,
        GuardianReviewError::Deadline => ApprovalReviewFailure::Deadline,
        GuardianReviewError::InvalidOutput => ApprovalReviewFailure::InvalidOutput,
    }
}

/// Installs the default-off Guardian V2 lifecycle and approval contributors.
pub fn install<C, F>(registry: &mut ExtensionRegistryBuilder<C>, config: F)
where
    C: Sync + 'static,
    F: for<'a> Fn(GuardianV2ThreadConfigInput<'a, C>) -> Option<LunaSamplerConfig>
        + Send
        + Sync
        + 'static,
{
    let extension = Arc::new(GuardianV2Extension::new(config));
    registry.thread_lifecycle_contributor(extension.clone());
    registry.approval_review_contributor(extension);
}
