//! Central approval policy-stage execution and reviewer routing.

use std::sync::Arc;

use crate::sandboxing::SandboxPermissions;
use crate::tools::hook_names::HookToolName;
use crate::tools::runtimes::apply_patch::ApplyPatchApprovalKey;
use crate::tools::runtimes::shell::ApprovalKey;
use crate::tools::runtimes::unified_exec::UnifiedExecApprovalKey;
use crate::tools::sandboxing::PermissionRequestPayload;
use codex_protocol::approvals::ExecPolicyAmendment;
use codex_protocol::approvals::GuardianCommandSource;
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_protocol::request_permissions::RequestPermissionProfile;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ApprovalAction {
    Shell {
        id: String,
        environment_id: String,
        command: Vec<String>,
        hook_command: String,
        cwd: PathUri,
        sandbox_permissions: SandboxPermissions,
        additional_permissions: Option<AdditionalPermissionProfile>,
        justification: Option<String>,
        proposed_execpolicy_amendment: Option<ExecPolicyAmendment>,
        cache_keys: Vec<ApprovalCacheKey>,
    },
    ExecCommand {
        id: String,
        environment_id: String,
        command: Vec<String>,
        hook_command: String,
        cwd: PathUri,
        sandbox_permissions: SandboxPermissions,
        additional_permissions: Option<AdditionalPermissionProfile>,
        justification: Option<String>,
        tty: bool,
        proposed_execpolicy_amendment: Option<ExecPolicyAmendment>,
        cache_keys: Vec<ApprovalCacheKey>,
    },
    #[cfg(unix)]
    Execve {
        id: String,
        approval_id: String,
        environment_id: String,
        source: GuardianCommandSource,
        program: String,
        argv: Vec<String>,
        cwd: AbsolutePathBuf,
        additional_permissions: Option<AdditionalPermissionProfile>,
    },
    ApplyPatch {
        id: String,
        environment_id: String,
        cwd: PathUri,
        files: Vec<PathUri>,
        patch: String,
        changes: Arc<HashMap<PathBuf, FileChange>>,
        permissions_preapproved: bool,
        cache_keys: Vec<ApprovalCacheKey>,
    },
    RequestPermissions {
        id: String,
        turn_id: String,
        environment: TurnEnvironmentSelection,
        cwd: AbsolutePathBuf,
        reason: Option<String>,
        permissions: RequestPermissionProfile,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ApprovalCacheKey {
    Shell(ApprovalKey),
    ExecCommand(UnifiedExecApprovalKey),
    ApplyPatch(ApplyPatchApprovalKey),
}

impl ApprovalAction {
    pub(crate) fn approval_review_action(
        &self,
    ) -> std::io::Result<codex_extension_api::ApprovalReviewAction> {
        Ok(match self {
            Self::Shell {
                environment_id,
                command,
                hook_command,
                cwd,
                sandbox_permissions,
                additional_permissions,
                justification,
                ..
            } => codex_extension_api::ApprovalReviewAction::Command {
                source: GuardianCommandSource::Shell,
                command: hook_command.clone(),
                argv: command.clone(),
                cwd: guardian_cwd(environment_id, cwd.clone())?,
                sandbox_permissions: *sandbox_permissions,
                additional_permissions: additional_permissions.clone(),
                justification: justification.clone(),
                tty: None,
            },
            Self::ExecCommand {
                environment_id,
                command,
                hook_command,
                cwd,
                sandbox_permissions,
                additional_permissions,
                justification,
                tty,
                ..
            } => codex_extension_api::ApprovalReviewAction::Command {
                source: GuardianCommandSource::UnifiedExec,
                command: hook_command.clone(),
                argv: command.clone(),
                cwd: guardian_cwd(environment_id, cwd.clone())?,
                sandbox_permissions: *sandbox_permissions,
                additional_permissions: additional_permissions.clone(),
                justification: justification.clone(),
                tty: Some(*tty),
            },
            #[cfg(unix)]
            Self::Execve {
                source,
                program,
                argv,
                cwd,
                additional_permissions,
                ..
            } => codex_extension_api::ApprovalReviewAction::Execve {
                source: *source,
                program: program.clone(),
                argv: argv.clone(),
                cwd: cwd.clone(),
                additional_permissions: additional_permissions.clone(),
            },
            Self::ApplyPatch {
                environment_id,
                cwd,
                files,
                patch,
                ..
            } => codex_extension_api::ApprovalReviewAction::ApplyPatch {
                cwd: guardian_cwd(environment_id, cwd.clone())?,
                files: files
                    .iter()
                    .cloned()
                    .map(|path| path.to_abs_path())
                    .collect::<std::io::Result<Vec<_>>>()?,
                patch: patch.clone(),
            },
            Self::RequestPermissions {
                reason,
                permissions,
                ..
            } => codex_extension_api::ApprovalReviewAction::RequestPermissions {
                reason: reason.clone(),
                permissions: permissions.clone(),
            },
        })
    }

    pub(crate) fn permission_request_payload(&self) -> PermissionRequestPayload {
        match self {
            Self::Shell {
                hook_command,
                justification,
                ..
            }
            | Self::ExecCommand {
                hook_command,
                justification,
                ..
            } => PermissionRequestPayload::bash(hook_command.clone(), justification.clone()),
            #[cfg(unix)]
            Self::Execve { program, argv, .. } => PermissionRequestPayload::bash(
                codex_shell_command::parse_command::shlex_join(
                    &std::iter::once(program.clone())
                        .chain(argv.iter().cloned())
                        .collect::<Vec<_>>(),
                ),
                /*description*/ None,
            ),
            Self::ApplyPatch { patch, .. } => PermissionRequestPayload {
                tool_name: HookToolName::apply_patch(),
                tool_input: serde_json::json!({ "command": patch }),
            },
            Self::RequestPermissions {
                reason,
                permissions,
                ..
            } => PermissionRequestPayload {
                tool_name: HookToolName::new("request_permissions"),
                tool_input: serde_json::json!({
                    "reason": reason,
                    "permissions": permissions,
                }),
            },
        }
    }

    pub(crate) fn cache_keys(&self) -> Vec<ApprovalCacheKey> {
        match self {
            Self::Shell { cache_keys, .. }
            | Self::ExecCommand { cache_keys, .. }
            | Self::ApplyPatch { cache_keys, .. } => cache_keys.clone(),
            #[cfg(unix)]
            Self::Execve { .. } => Vec::new(),
            Self::RequestPermissions { .. } => Vec::new(),
        }
    }

    pub(super) fn into_guardian_request(
        self,
    ) -> std::io::Result<crate::guardian::GuardianApprovalRequest> {
        Ok(match self {
            Self::Shell {
                id,
                environment_id,
                command,
                cwd,
                sandbox_permissions,
                additional_permissions,
                justification,
                ..
            } => crate::guardian::GuardianApprovalRequest::Shell {
                id,
                command,
                cwd: guardian_cwd(&environment_id, cwd)?,
                sandbox_permissions,
                additional_permissions,
                justification,
            },
            Self::ExecCommand {
                id,
                environment_id,
                command,
                cwd,
                sandbox_permissions,
                additional_permissions,
                justification,
                tty,
                ..
            } => crate::guardian::GuardianApprovalRequest::ExecCommand {
                id,
                command,
                cwd: guardian_cwd(&environment_id, cwd)?,
                sandbox_permissions,
                additional_permissions,
                justification,
                tty,
            },
            #[cfg(unix)]
            Self::Execve {
                id,
                source,
                program,
                argv,
                cwd,
                additional_permissions,
                ..
            } => crate::guardian::GuardianApprovalRequest::Execve {
                id,
                source,
                program,
                argv,
                cwd,
                additional_permissions,
            },
            Self::ApplyPatch {
                id,
                environment_id,
                cwd,
                files,
                patch,
                ..
            } => crate::guardian::GuardianApprovalRequest::ApplyPatch {
                id,
                cwd: guardian_cwd(&environment_id, cwd)?,
                files: files
                    .into_iter()
                    .map(|path| path.to_abs_path())
                    .collect::<std::io::Result<Vec<_>>>()?,
                patch,
            },
            Self::RequestPermissions {
                id,
                turn_id,
                reason,
                permissions,
                ..
            } => crate::guardian::GuardianApprovalRequest::RequestPermissions {
                id,
                turn_id,
                reason,
                permissions,
            },
        })
    }
}

pub(super) fn guardian_cwd(environment_id: &str, cwd: PathUri) -> std::io::Result<AbsolutePathBuf> {
    match cwd.to_abs_path() {
        Ok(cwd) => Ok(cwd),
        Err(err) if environment_id != codex_exec_server::LOCAL_ENVIRONMENT_ID => Err(err),
        Err(_) => {
            let cwd_display = cwd.to_string();
            let path = cwd.to_url().to_file_path().map_err(|()| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("local cwd URI `{cwd_display}` is not a host-native path"),
                )
            })?;
            AbsolutePathBuf::from_absolute_path_checked(path).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("local cwd URI `{cwd_display}` is not absolute: {err}"),
                )
            })
        }
    }
}

#[cfg(all(test, unix))]
#[path = "approvals_tests.rs"]
mod tests;
