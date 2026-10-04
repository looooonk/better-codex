use super::ShellState;
use super::ToolBlockStatus;
use codex_app_server_protocol::GuardianApprovalReview;
use codex_app_server_protocol::GuardianApprovalReviewStatus;

impl ShellState {
    pub(super) fn record_guardian_review(
        &mut self,
        review_id: String,
        review: GuardianApprovalReview,
    ) {
        let (label, status) = match review.status {
            GuardianApprovalReviewStatus::InProgress => ("Reviewing", ToolBlockStatus::Running),
            GuardianApprovalReviewStatus::Approved => ("Approved", ToolBlockStatus::Success),
            GuardianApprovalReviewStatus::Denied => ("Denied", ToolBlockStatus::Fail),
            GuardianApprovalReviewStatus::TimedOut => ("Timed out", ToolBlockStatus::Fail),
            GuardianApprovalReviewStatus::Aborted => ("Aborted", ToolBlockStatus::Fail),
        };
        let id = format!("guardian:{review_id}");
        self.push_tool_with_status_for_item(
            id.clone(),
            format!("Approval review: {label}"),
            status,
        );
        if let Some(rationale) = review.rationale.filter(|text| !text.is_empty()) {
            self.push_output_with_status_for_item(id, rationale, status);
        }
    }
}
