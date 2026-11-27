use super::ShellState;
use super::workspace_requests::WorkspaceRequest;
use codex_app_server_protocol::GuardianApprovalReviewStatus;
use codex_app_server_protocol::ItemGuardianApprovalReviewCompletedNotification;
use codex_protocol::approvals::GuardianAssessmentEvent;
use codex_protocol::approvals::GuardianAssessmentStatus;
use color_eyre::Result;
use color_eyre::eyre::bail;

const MAX_RECENT_DENIALS: usize = 10;

impl ShellState {
    pub(super) fn record_guardian_completion(
        &mut self,
        notification: ItemGuardianApprovalReviewCompletedNotification,
    ) {
        self.record_guardian_review(
            notification.review_id.clone(),
            notification.target_item_id.as_deref(),
            &notification.action,
            notification.review.clone(),
        );
        self.recent_guardian_denials
            .retain(|event| event.id != notification.review_id);
        if notification.review.status != GuardianApprovalReviewStatus::Denied {
            return;
        }
        let action = match notification.action.try_into() {
            Ok(action) => action,
            Err(error) => {
                self.push_error(format!("Could not retain denied action: {error}"));
                return;
            }
        };
        let review = notification.review;
        let event = GuardianAssessmentEvent {
            review_reason: None,
            model_context: None,
            id: notification.review_id,
            target_item_id: notification.target_item_id,
            plugin_id: None,
            script_path: None,
            turn_id: notification.turn_id,
            started_at_ms: notification.started_at_ms,
            completed_at_ms: Some(notification.completed_at_ms),
            status: GuardianAssessmentStatus::Denied,
            risk_level: review.risk_level.map(|risk| match risk {
                codex_app_server_protocol::GuardianRiskLevel::Low => {
                    codex_protocol::approvals::GuardianRiskLevel::Low
                }
                codex_app_server_protocol::GuardianRiskLevel::Medium => {
                    codex_protocol::approvals::GuardianRiskLevel::Medium
                }
                codex_app_server_protocol::GuardianRiskLevel::High => {
                    codex_protocol::approvals::GuardianRiskLevel::High
                }
                codex_app_server_protocol::GuardianRiskLevel::Critical => {
                    codex_protocol::approvals::GuardianRiskLevel::Critical
                }
            }),
            user_authorization: review.user_authorization.map(
                |authorization| match authorization {
                    codex_app_server_protocol::GuardianUserAuthorization::Unknown => {
                        codex_protocol::approvals::GuardianUserAuthorization::Unknown
                    }
                    codex_app_server_protocol::GuardianUserAuthorization::Low => {
                        codex_protocol::approvals::GuardianUserAuthorization::Low
                    }
                    codex_app_server_protocol::GuardianUserAuthorization::Medium => {
                        codex_protocol::approvals::GuardianUserAuthorization::Medium
                    }
                    codex_app_server_protocol::GuardianUserAuthorization::High => {
                        codex_protocol::approvals::GuardianUserAuthorization::High
                    }
                },
            ),
            rationale: review.rationale,
            decision_source: Some(match notification.decision_source {
                codex_app_server_protocol::AutoReviewDecisionSource::Agent => {
                    codex_protocol::approvals::GuardianAssessmentDecisionSource::Agent
                }
            }),
            action,
        };
        self.recent_guardian_denials.push_front(event);
        self.recent_guardian_denials.truncate(MAX_RECENT_DENIALS);
    }

    pub(super) fn guardian_approval_request(&self, id: &str) -> Result<WorkspaceRequest> {
        let Some(event) = self
            .recent_guardian_denials
            .iter()
            .find(|event| event.id == id)
        else {
            bail!("That denial is no longer available. Use /approve to list recent denials.");
        };
        Ok(WorkspaceRequest::GuardianApproval(Box::new(event.clone())))
    }

    pub(super) fn show_guardian_denials(&mut self) {
        if self.recent_guardian_denials.is_empty() {
            self.show_hint("No recent auto-review denials in this conversation.");
            return;
        }
        let mut lines = Vec::new();
        for event in &self.recent_guardian_denials {
            lines.push(format!(
                "{}: {}",
                event.id,
                super::guardian_activity::action_summary(&event.action.clone().into())
            ));
            lines.push(
                event
                    .rationale
                    .clone()
                    .unwrap_or_else(|| "No rationale was provided.".to_string()),
            );
        }
        lines.push("Use /approve <review-id> to allow one retry. The retry still goes through auto-review.".to_string());
        self.show_command_report("Recent auto-review denials", lines.join("\n"));
    }
}

#[cfg(test)]
#[path = "guardian_denials_tests.rs"]
mod tests;
