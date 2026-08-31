use std::sync::Arc;
use std::time::Instant;

use codex_protocol::approvals::GuardianAssessmentAction;
use codex_protocol::approvals::GuardianCommandSource;
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::SandboxPermissions;
use codex_protocol::protocol::GuardianRiskLevel;
use codex_protocol::protocol::GuardianUserAuthorization;
use codex_protocol::request_permissions::RequestPermissionProfile;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde_json::Value;

use crate::ExtensionFuture;
use crate::ToolCallSource;

/// Host-owned cancellation signal for one approval review.
pub trait ApprovalReviewCancellation: Send + Sync {
    /// Returns whether cancellation has already been requested.
    fn is_cancelled(&self) -> bool;

    /// Resolves when cancellation is requested.
    fn cancelled(&self) -> ExtensionFuture<'_, ()>;
}

/// Immutable identity that binds a review to one action attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalReviewBinding {
    pub thread_id: String,
    pub turn_id: String,
    pub action_id: String,
    pub attempt_id: String,
    pub source: ToolCallSource,
    pub evidence_revision: u64,
}

/// Typed action details supplied to an approval reviewer.
#[derive(Clone, Debug, PartialEq)]
pub enum ApprovalReviewAction {
    Command {
        source: GuardianCommandSource,
        command: String,
        argv: Vec<String>,
        cwd: AbsolutePathBuf,
        sandbox_permissions: SandboxPermissions,
        additional_permissions: Option<AdditionalPermissionProfile>,
        justification: Option<String>,
        tty: Option<bool>,
    },
    Execve {
        source: GuardianCommandSource,
        program: String,
        argv: Vec<String>,
        cwd: AbsolutePathBuf,
        additional_permissions: Option<AdditionalPermissionProfile>,
    },
    ApplyPatch {
        cwd: AbsolutePathBuf,
        files: Vec<AbsolutePathBuf>,
        patch: String,
    },
    RequestPermissions {
        reason: Option<String>,
        permissions: RequestPermissionProfile,
    },
}

impl ApprovalReviewAction {
    /// Returns the bounded public action shape used by review lifecycle events.
    pub fn assessment_action(&self) -> GuardianAssessmentAction {
        match self {
            Self::Command {
                source,
                command,
                cwd,
                ..
            } => GuardianAssessmentAction::Command {
                source: *source,
                command: command.clone(),
                cwd: cwd.clone(),
            },
            Self::Execve {
                source,
                program,
                argv,
                cwd,
                ..
            } => GuardianAssessmentAction::Execve {
                source: *source,
                program: program.clone(),
                argv: argv.clone(),
                cwd: cwd.clone(),
            },
            Self::ApplyPatch { cwd, files, .. } => GuardianAssessmentAction::ApplyPatch {
                cwd: cwd.clone(),
                files: files.clone(),
            },
            Self::RequestPermissions {
                reason,
                permissions,
            } => GuardianAssessmentAction::RequestPermissions {
                reason: reason.clone(),
                permissions: permissions.clone(),
            },
        }
    }

    /// Returns the redaction input used by the reviewer request builder.
    pub fn request_payload(&self) -> Value {
        match self {
            Self::Command {
                source,
                argv,
                cwd,
                sandbox_permissions,
                additional_permissions,
                justification,
                tty,
                ..
            } => serde_json::json!({
                "tool": match source {
                    GuardianCommandSource::Shell => "shell",
                    GuardianCommandSource::UnifiedExec => "exec_command",
                },
                "command": argv,
                "cwd": cwd,
                "sandbox_permissions": sandbox_permissions,
                "additional_permissions": additional_permissions,
                "justification": justification,
                "tty": tty,
            }),
            Self::Execve {
                source,
                program,
                argv,
                cwd,
                additional_permissions,
            } => serde_json::json!({
                "tool": match source {
                    GuardianCommandSource::Shell => "shell",
                    GuardianCommandSource::UnifiedExec => "exec_command",
                },
                "program": program,
                "argv": argv,
                "cwd": cwd,
                "additional_permissions": additional_permissions,
            }),
            Self::ApplyPatch { cwd, files, patch } => serde_json::json!({
                "tool": "apply_patch",
                "cwd": cwd,
                "files": files,
                "patch": patch,
            }),
            Self::RequestPermissions {
                reason,
                permissions,
            } => serde_json::json!({
                "tool": "request_permissions",
                "reason": reason,
                "permissions": permissions,
            }),
        }
    }
}

/// One bounded evidence entry associated with the current action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalReviewEvidence {
    pub kind: String,
    pub provenance: Option<String>,
    pub text: String,
}

/// One sanitized image associated with the current action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalReviewImage {
    pub data_url: String,
}

/// Complete host input for one synchronous approval review.
pub struct ApprovalReviewInput {
    pub binding: ApprovalReviewBinding,
    pub action: ApprovalReviewAction,
    pub history: Vec<ResponseItem>,
    pub evidence: Vec<ApprovalReviewEvidence>,
    pub images: Vec<ApprovalReviewImage>,
    pub deadline: Instant,
    pub cancellation: Arc<dyn ApprovalReviewCancellation>,
}

/// Sanitized model assessment returned to the approval pipeline.
#[derive(Clone, Debug, PartialEq)]
pub struct ApprovalReviewOutcome {
    pub risk_level: GuardianRiskLevel,
    pub user_authorization: GuardianUserAuthorization,
    pub rationale: String,
}

/// Bounded failure categories that require manual review or a fail-closed denial.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalReviewFailure {
    NotInstalled,
    MultipleAuthorities,
    InvalidInput,
    ActionTooLarge,
    RequestTooLarge,
    SamplerUnavailable,
    Deadline,
    InvalidOutput,
}

/// Authoritative result for one correlated action attempt.
#[derive(Clone, Debug, PartialEq)]
pub enum ApprovalReviewResult {
    Allow(ApprovalReviewOutcome),
    Deny(ApprovalReviewOutcome),
    ManualReview(ApprovalReviewFailure),
    Cancelled,
}
