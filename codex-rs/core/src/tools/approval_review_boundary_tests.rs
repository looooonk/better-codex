use codex_extension_api::ApprovalReviewBinding;
use codex_extension_api::ApprovalReviewCancellation;
use codex_extension_api::ExtensionFuture;
use codex_protocol::models::ContentItem;
use codex_protocol::models::SandboxPermissions;
use codex_protocol::protocol::GuardianRiskLevel;
use codex_protocol::protocol::GuardianUserAuthorization;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::time::Instant;

use super::*;

struct NeverCancelled;

impl ApprovalReviewCancellation for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }

    fn cancelled(&self) -> ExtensionFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}

fn input(action: ApprovalReviewAction) -> ApprovalReviewInput {
    ApprovalReviewInput {
        binding: ApprovalReviewBinding {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            action_id: "action-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            source: ToolCallSource::Direct,
            evidence_revision: 0,
        },
        action,
        history: Vec::new(),
        evidence: Vec::new(),
        images: Vec::new(),
        deadline: Instant::now(),
        cancellation: Arc::new(NeverCancelled),
    }
}

fn command(command: String) -> ApprovalReviewAction {
    ApprovalReviewAction::Command {
        source: codex_protocol::approvals::GuardianCommandSource::Shell,
        argv: vec![command.clone()],
        command,
        cwd: AbsolutePathBuf::from_absolute_path("/workspace").expect("absolute cwd"),
        sandbox_permissions: SandboxPermissions::UseDefault,
        additional_permissions: None,
        justification: None,
        tty: None,
    }
}

#[test]
fn oversized_action_and_images_fail_before_dispatch() {
    let Err(error) = prepare_approval_review_input(input(command("x".repeat(100_000)))) else {
        panic!("oversized action should fail");
    };
    assert_eq!(
        error,
        ApprovalReviewFailure::ActionTooLarge
    );

    let mut input = input(command("echo safe".to_string()));
    input.images.push(ApprovalReviewImage {
        data_url: format!(
            "data:image/png;base64,{}",
            "A".repeat(MAX_ENCODED_IMAGE_BYTES + 1)
        ),
    });
    let Err(error) = prepare_approval_review_input(input) else {
        panic!("oversized image should fail");
    };
    assert_eq!(error, ApprovalReviewFailure::InvalidInput);
}

#[test]
fn history_and_evidence_are_bounded_and_redacted() {
    let secret = "sk-abcdefghijklmnopqrstuvwxyz012345";
    let mut input = input(command("echo safe".to_string()));
    input.history = (0..100)
        .map(|index| ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: format!("history-{index} {secret} {}", "h".repeat(50_000)),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        })
        .collect();
    input.evidence = (0..100)
        .map(|index| ApprovalReviewEvidence {
            kind: "node_repl_output".to_string(),
            provenance: Some(format!("cell-{index}-{secret}")),
            text: format!("evidence-{index} {secret} {}", "e".repeat(50_000)),
        })
        .collect();

    let bounded = prepare_approval_review_input(input).expect("bounded input");
    let history = serde_json::to_string(&bounded.history).expect("serialize bounded history");
    let evidence = bounded
        .evidence
        .iter()
        .map(|entry| {
            format!(
                "{}{}{}",
                entry.kind,
                entry.provenance.as_deref().unwrap_or_default(),
                entry.text
            )
        })
        .collect::<String>();

    assert!(history.len() <= MAX_HISTORY_BYTES);
    assert!(evidence.len() <= MAX_EVIDENCE_BYTES);
    assert!(!history.contains(secret));
    assert!(!evidence.contains(secret));
}

#[test]
fn contributor_rationale_is_bounded_and_redacted() {
    let secret = "sk-abcdefghijklmnopqrstuvwxyz012345";
    let outcome = ApprovalReviewOutcome {
        risk_level: GuardianRiskLevel::High,
        user_authorization: GuardianUserAuthorization::Low,
        rationale: format!("{secret} {}", "r".repeat(100_000)),
    };
    let ApprovalReviewResult::Deny(outcome) =
        sanitize_approval_review_result(ApprovalReviewResult::Deny(outcome))
    else {
        panic!("denial should stay a denial");
    };

    assert!(outcome.rationale.len() <= MAX_RATIONALE_BYTES);
    assert!(!outcome.rationale.contains(secret));
}
