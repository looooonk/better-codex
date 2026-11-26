use super::ShellState;
use super::ToolBlockStatus;
use super::TranscriptKind;
use super::TranscriptLine;
use super::design::palette;
use super::tool_groups::ToolAction;
use codex_app_server_protocol::GuardianApprovalReview;
use codex_app_server_protocol::GuardianApprovalReviewAction;
use codex_app_server_protocol::GuardianApprovalReviewStatus;
use ratatui::style::Color;
use ratatui::style::Stylize;
use ratatui::text::Line;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ToolApproval {
    pub(super) id: String,
    pub(super) action: GuardianApprovalReviewAction,
    pub(super) review: GuardianApprovalReview,
}

impl ToolApproval {
    pub(super) fn status(&self) -> (&'static str, Color) {
        match self.review.status {
            GuardianApprovalReviewStatus::InProgress => ("Reviewing", palette::cyan()),
            GuardianApprovalReviewStatus::Approved => ("Approved", palette::success()),
            GuardianApprovalReviewStatus::Denied => ("Denied", palette::error()),
            GuardianApprovalReviewStatus::TimedOut => ("Timed out", palette::warning()),
            GuardianApprovalReviewStatus::Aborted => ("Aborted", palette::warning()),
        }
    }

    pub(super) fn disclosure(&self) -> Line<'static> {
        let (label, color) = self.status();
        Line::from(vec![
            "↳ Auto-review  ".fg(palette::muted()),
            label.fg(color),
            " ▸".fg(palette::muted()),
        ])
    }
}

impl ShellState {
    pub(super) fn record_guardian_review(
        &mut self,
        id: String,
        target: Option<&str>,
        action: &GuardianApprovalReviewAction,
        review: GuardianApprovalReview,
    ) {
        let parent_id = target
            .map(str::to_owned)
            .unwrap_or_else(|| format!("guardian:{id}"));
        let approval = ToolApproval {
            id,
            action: action.clone(),
            review,
        };
        if let Some(popup) = &mut self.tool_review
            && popup.approval.id == approval.id
        {
            popup.approval = approval.clone();
        }
        let status = match approval.review.status {
            GuardianApprovalReviewStatus::InProgress => ToolBlockStatus::Running,
            GuardianApprovalReviewStatus::Approved => ToolBlockStatus::Success,
            GuardianApprovalReviewStatus::Denied | GuardianApprovalReviewStatus::TimedOut => {
                ToolBlockStatus::Fail
            }
            GuardianApprovalReviewStatus::Aborted => ToolBlockStatus::Interrupted,
        };
        if let Some(parent) = self.transcript.iter_mut().rev().find(|line| {
            matches!(line.kind, TranscriptKind::Tool | TranscriptKind::Diff)
                && line.item_id.as_deref() == Some(&parent_id)
        }) {
            if matches!(parent.tool_action, Some(ToolAction::ApprovalReview)) {
                parent.tool_status = Some(status);
            }
            if let Some(previous) = parent
                .approvals
                .iter_mut()
                .find(|entry| entry.id == approval.id)
            {
                *previous = approval;
            } else {
                parent.approvals.push(approval);
            }
            parent.mark_render_changed();
        } else {
            // Approval can precede item/started. An independent network review has no parent at all.
            let mut line = TranscriptLine::new(TranscriptKind::Tool, action_summary(action))
                .item_id(parent_id)
                .tool_status(status);
            line.tool_action = Some(ToolAction::ApprovalReview);
            line.approvals.push(approval);
            self.upsert_line(line);
        }
    }
}

pub(super) fn action_summary(action: &GuardianApprovalReviewAction) -> String {
    match action {
        GuardianApprovalReviewAction::Command { command, .. } => command.clone(),
        GuardianApprovalReviewAction::Execve { program, argv, .. } => {
            if argv.is_empty() {
                program.clone()
            } else {
                shlex::try_join(argv.iter().map(String::as_str)).unwrap_or_else(|_| argv.join(" "))
            }
        }
        GuardianApprovalReviewAction::WriteStdin {
            process_id, stdin, ..
        } => format!("Send {stdin:?} to terminal {process_id}"),
        GuardianApprovalReviewAction::ApplyPatch { files, .. } => format!(
            "Modify {}",
            files
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        GuardianApprovalReviewAction::NetworkAccess { target, .. } => {
            format!("Network access to {target}")
        }
        GuardianApprovalReviewAction::McpToolCall {
            server, tool_name, ..
        } => format!("Tool {server}/{tool_name}"),
        GuardianApprovalReviewAction::RequestPermissions { reason, .. } => reason
            .clone()
            .unwrap_or_else(|| "Request additional permissions".to_string()),
    }
}
