use std::sync::Arc;

use codex_extension_api::ApprovalReviewFailure;
use codex_extension_api::ApprovalReviewResult;
use codex_protocol::approvals::GuardianAssessmentAction;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::GuardianAssessmentDecisionSource;
use codex_protocol::protocol::GuardianAssessmentEvent;
use codex_protocol::protocol::GuardianAssessmentStatus;
use codex_protocol::protocol::WarningEvent;

use crate::guardian::new_guardian_review_id;
use crate::guardian::record_guardian_denial;
use crate::guardian::record_guardian_non_denial;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::turn_timing::now_unix_timestamp_ms;

pub(super) struct ApprovalReviewLifecycle {
    id: String,
    target_item_id: String,
    started_at_ms: i64,
    action: GuardianAssessmentAction,
}

impl ApprovalReviewLifecycle {
    pub(super) async fn begin(
        session: &Session,
        turn: &TurnContext,
        target_item_id: String,
        action: GuardianAssessmentAction,
    ) -> Self {
        let lifecycle = Self {
            id: new_guardian_review_id(),
            target_item_id,
            started_at_ms: now_unix_timestamp_ms(),
            action,
        };
        session
            .send_event(
                turn,
                EventMsg::GuardianAssessment(GuardianAssessmentEvent {
                    id: lifecycle.id.clone(),
                    target_item_id: Some(lifecycle.target_item_id.clone()),
                    turn_id: turn.sub_id.clone(),
                    started_at_ms: lifecycle.started_at_ms,
                    completed_at_ms: None,
                    status: GuardianAssessmentStatus::InProgress,
                    risk_level: None,
                    user_authorization: None,
                    rationale: None,
                    decision_source: None,
                    action: lifecycle.action.clone(),
                }),
            )
            .await;
        lifecycle
    }

    pub(super) async fn finish(
        self,
        session: &Arc<Session>,
        turn: &Arc<TurnContext>,
        result: &ApprovalReviewResult,
    ) {
        let (status, risk_level, user_authorization, rationale, explicit_denial) = match result {
            ApprovalReviewResult::Allow(outcome) => (
                GuardianAssessmentStatus::Approved,
                Some(outcome.risk_level),
                Some(outcome.user_authorization),
                Some(outcome.rationale.clone()),
                false,
            ),
            ApprovalReviewResult::Deny(outcome) => (
                GuardianAssessmentStatus::Denied,
                Some(outcome.risk_level),
                Some(outcome.user_authorization),
                Some(outcome.rationale.clone()),
                true,
            ),
            ApprovalReviewResult::ManualReview(ApprovalReviewFailure::Deadline) => (
                GuardianAssessmentStatus::TimedOut,
                None,
                None,
                Some("automatic approval review timed out; manual review is required".to_string()),
                false,
            ),
            ApprovalReviewResult::ManualReview(failure) => (
                GuardianAssessmentStatus::Denied,
                None,
                None,
                Some(format!(
                    "automatic approval review failed closed ({failure:?}); manual review is required"
                )),
                false,
            ),
            ApprovalReviewResult::Cancelled => {
                (GuardianAssessmentStatus::Aborted, None, None, None, false)
            }
        };
        if let Some(rationale) = rationale.as_ref() {
            session
                .send_event(
                    turn,
                    EventMsg::GuardianWarning(WarningEvent {
                        message: rationale.clone(),
                    }),
                )
                .await;
        }
        session
            .send_event(
                turn,
                EventMsg::GuardianAssessment(GuardianAssessmentEvent {
                    id: self.id,
                    target_item_id: Some(self.target_item_id),
                    turn_id: turn.sub_id.clone(),
                    started_at_ms: self.started_at_ms,
                    completed_at_ms: Some(now_unix_timestamp_ms()),
                    status,
                    risk_level,
                    user_authorization,
                    rationale,
                    decision_source: Some(GuardianAssessmentDecisionSource::Agent),
                    action: self.action,
                }),
            )
            .await;
        if explicit_denial {
            record_guardian_denial(session, turn, &turn.sub_id).await;
        } else {
            record_guardian_non_denial(session, &turn.sub_id).await;
        }
    }
}
