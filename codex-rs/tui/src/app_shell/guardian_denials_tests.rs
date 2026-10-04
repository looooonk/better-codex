use super::*;
use crate::app_shell::render::ShellView;
use crate::app_shell::workspace_requests::WorkspaceResponse;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn denial(id: &str) -> ItemGuardianApprovalReviewCompletedNotification {
    serde_json::from_value(serde_json::json!({
        "threadId": "01987b77-33b8-76e3-9a7f-a367513be004", "turnId": "turn",
        "startedAtMs": 1, "completedAtMs": 2, "reviewId": id, "targetItemId": "item",
        "decisionSource": "agent", "review": {
            "status": "denied", "riskLevel": "high", "userAuthorization": "low",
            "rationale": "This command changes files outside the requested directory."
        },
        "action": {"type": "command", "source": "shell", "command": "touch /tmp/reviewed-file", "cwd": "/workspace"}
    })).unwrap()
}

#[test]
fn approvals_keep_the_native_denial_and_are_removed_only_after_success() {
    let mut shell = ShellState::snapshot_fixture();
    shell.record_guardian_completion(denial("review-1"));
    let WorkspaceRequest::GuardianApproval(event) =
        shell.guardian_approval_request("review-1").unwrap()
    else {
        panic!("guardian request");
    };
    assert_eq!(*event, shell.recent_guardian_denials[0]);
    shell.complete_workspace_request(
        shell.thread_id,
        Err(color_eyre::eyre::eyre!("disconnected")),
    );
    assert!(shell.guardian_approval_request("review-1").is_ok());
    shell.complete_workspace_request(
        codex_protocol::ThreadId::new(),
        Ok(WorkspaceResponse::GuardianApproved("review-1".to_string())),
    );
    assert!(shell.guardian_approval_request("review-1").is_ok());
    shell.complete_workspace_request(
        shell.thread_id,
        Ok(WorkspaceResponse::GuardianApproved("review-1".to_string())),
    );
    assert!(shell.guardian_approval_request("review-1").is_err());
}

#[test]
fn recent_denials_are_bounded_deduplicated_and_exclude_approvals() {
    let mut shell = ShellState::snapshot_fixture();
    for id in 0..12 {
        shell.record_guardian_completion(denial(&format!("review-{id}")));
    }
    shell.record_guardian_completion(denial("review-3"));
    let mut approved = denial("review-11");
    approved.review.status = GuardianApprovalReviewStatus::Approved;
    shell.record_guardian_completion(approved);
    assert_eq!(
        shell
            .recent_guardian_denials
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        [
            "review-3",
            "review-10",
            "review-9",
            "review-8",
            "review-7",
            "review-6",
            "review-5",
            "review-4",
            "review-2"
        ]
    );
    assert!(shell.guardian_approval_request("review-0").is_err());
}

#[test]
fn guardian_denial_list_keeps_action_and_rationale_visible() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.record_guardian_completion(denial("review-1"));
    shell.show_guardian_denials();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 27,
    );
    let mut buffer = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut buffer);
    let lines = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>();
    insta::assert_snapshot!(lines.join("\n"));
}
