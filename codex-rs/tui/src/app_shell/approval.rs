use crate::app_server_approval_conversions::granted_permission_profile_from_request;
use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::CommandExecutionRequestApprovalResponse;
use codex_app_server_protocol::FileChangeApprovalDecision;
use codex_app_server_protocol::FileChangeRequestApprovalResponse;
use codex_app_server_protocol::GrantedPermissionProfile;
use codex_app_server_protocol::PermissionGrantScope;
use codex_app_server_protocol::PermissionsRequestApprovalResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_protocol::request_permissions::RequestPermissionProfile as CoreRequestPermissionProfile;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApprovalChoice {
    Approve,
    Deny,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingApproval {
    request_id: RequestId,
    title: String,
    detail: String,
    kind: PendingApprovalKind,
}

impl PendingApproval {
    pub(super) fn from_request(request: &ServerRequest) -> Result<Option<Self>, String> {
        match request {
            ServerRequest::CommandExecutionRequestApproval { request_id, params } => {
                let command = params
                    .command
                    .clone()
                    .unwrap_or_else(|| "<unknown command>".to_string());
                let detail = approval_detail(
                    params.reason.as_deref(),
                    params.cwd.as_ref().map(ToString::to_string),
                );
                Ok(Some(Self {
                    request_id: request_id.clone(),
                    title: format!("Run command: {command}"),
                    detail,
                    kind: PendingApprovalKind::Command,
                }))
            }
            ServerRequest::FileChangeRequestApproval { request_id, params } => {
                let detail = approval_detail(
                    params.reason.as_deref(),
                    params
                        .grant_root
                        .as_ref()
                        .map(|root| format!("grant root {}", root.display())),
                );
                Ok(Some(Self {
                    request_id: request_id.clone(),
                    title: format!("Apply file changes: {}", params.item_id),
                    detail,
                    kind: PendingApprovalKind::FileChange,
                }))
            }
            ServerRequest::PermissionsRequestApproval { request_id, params } => {
                let requested_permissions = CoreRequestPermissionProfile::try_from(
                    params.permissions.clone(),
                )
                .map_err(|err| format!("failed to localize requested filesystem paths: {err}"))?;
                let detail = approval_detail(
                    params.reason.as_deref(),
                    Some(format!("cwd {}", params.cwd.as_path().display())),
                );
                Ok(Some(Self {
                    request_id: request_id.clone(),
                    title: format!(
                        "Grant permissions: {}",
                        permission_summary(&params.permissions)
                    ),
                    detail,
                    kind: PendingApprovalKind::Permissions {
                        approved: granted_permission_profile_from_request(requested_permissions),
                    },
                }))
            }
            ServerRequest::ExecCommandApproval { .. }
            | ServerRequest::ApplyPatchApproval { .. }
            | ServerRequest::ToolRequestUserInput { .. }
            | ServerRequest::DynamicToolCall { .. }
            | ServerRequest::McpServerElicitationRequest { .. }
            | ServerRequest::ChatgptAuthTokensRefresh { .. }
            | ServerRequest::CurrentTimeRead { .. }
            | ServerRequest::AttestationGenerate { .. } => Ok(None),
        }
    }

    pub(super) fn request_id(&self) -> RequestId {
        self.request_id.clone()
    }

    pub(super) fn title(&self) -> &str {
        &self.title
    }

    pub(super) fn detail(&self) -> &str {
        &self.detail
    }

    pub(super) fn result(&self, choice: ApprovalChoice) -> serde_json::Result<Value> {
        match self.kind.clone() {
            PendingApprovalKind::Command => {
                serde_json::to_value(CommandExecutionRequestApprovalResponse {
                    decision: match choice {
                        ApprovalChoice::Approve => CommandExecutionApprovalDecision::Accept,
                        ApprovalChoice::Deny => CommandExecutionApprovalDecision::Decline,
                    },
                })
            }
            PendingApprovalKind::FileChange => {
                serde_json::to_value(FileChangeRequestApprovalResponse {
                    decision: match choice {
                        ApprovalChoice::Approve => FileChangeApprovalDecision::Accept,
                        ApprovalChoice::Deny => FileChangeApprovalDecision::Decline,
                    },
                })
            }
            PendingApprovalKind::Permissions { approved } => {
                serde_json::to_value(PermissionsRequestApprovalResponse {
                    permissions: match choice {
                        ApprovalChoice::Approve => approved,
                        ApprovalChoice::Deny => GrantedPermissionProfile::default(),
                    },
                    scope: PermissionGrantScope::Turn,
                    strict_auto_review: None,
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum PendingApprovalKind {
    Command,
    FileChange,
    Permissions { approved: GrantedPermissionProfile },
}

fn approval_detail(reason: Option<&str>, context: Option<String>) -> String {
    match (reason, context) {
        (Some(reason), Some(context)) if !reason.is_empty() => format!("{reason} - {context}"),
        (Some(reason), _) if !reason.is_empty() => reason.to_string(),
        (_, Some(context)) => context,
        _ => "Approve or deny this backend request.".to_string(),
    }
}

fn permission_summary(permissions: &codex_app_server_protocol::RequestPermissionProfile) -> String {
    let mut parts = Vec::new();
    if permissions.network.is_some() {
        parts.push("network");
    }
    if permissions.file_system.is_some() {
        parts.push("filesystem");
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(", ")
    }
}
