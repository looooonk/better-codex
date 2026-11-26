use super::ShellState;
use super::ToolBlockStatus;
use super::TranscriptKind;
use super::TranscriptLine;
use super::design::palette;
use super::tool_groups::ToolAction;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use codex_app_server_protocol::GuardianApprovalReview;
use codex_app_server_protocol::GuardianApprovalReviewAction;
use codex_app_server_protocol::GuardianApprovalReviewStatus;
use ratatui::style::Stylize;
use ratatui::text::Line;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ToolApproval {
    id: String,
    subject: String,
    review: GuardianApprovalReview,
}

impl ToolApproval {
    pub(super) fn lines(&self, parent: &str, width: u16) -> Vec<HyperlinkLine> {
        let (label, color) = match self.review.status {
            GuardianApprovalReviewStatus::InProgress => ("Reviewing", palette::cyan()),
            GuardianApprovalReviewStatus::Approved => ("Approved", palette::success()),
            GuardianApprovalReviewStatus::Denied => ("Denied", palette::error()),
            GuardianApprovalReviewStatus::TimedOut => ("Timed out", palette::warning()),
            GuardianApprovalReviewStatus::Aborted => ("Aborted", palette::warning()),
        };
        let mut heading = vec![
            "  ↳ Auto-review  ".fg(palette::muted()),
            label.fg(color).bold(),
        ];
        if !parent.contains(&self.subject) {
            heading.push(format!(" · {}", self.subject).fg(palette::muted()));
        }
        if let Some(risk) = self.review.risk_level {
            heading.push(
                format!(" · {risk:?} risk")
                    .to_lowercase()
                    .fg(palette::muted()),
            );
        }
        if let Some(auth) = self.review.user_authorization {
            heading.push(
                format!(" · {auth:?} authorization")
                    .to_lowercase()
                    .fg(palette::muted()),
            );
        }
        let width = usize::from(width.max(1));
        let indent = Line::from("    ");
        let mut rows = word_wrap_lines(
            [Line::from(heading)],
            RtOptions::new(width).subsequent_indent(indent.clone()),
        );
        if let Some(reason) = &self.review.rationale {
            rows.extend(word_wrap_lines(
                reason
                    .lines()
                    .map(|line| Line::from(line.to_owned().fg(palette::muted()))),
                RtOptions::new(width)
                    .initial_indent(indent.clone())
                    .subsequent_indent(indent),
            ));
        }
        rows.into_iter().map(HyperlinkLine::new).collect()
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
            subject: action_summary(action),
            review,
        };
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
            let mut line = TranscriptLine::new(TranscriptKind::Tool, approval.subject.clone())
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
