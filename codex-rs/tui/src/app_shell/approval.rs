use crate::app_server_approval_conversions::granted_permission_profile_from_request;
use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::CommandExecutionRequestApprovalResponse;
use codex_app_server_protocol::FileChangeApprovalDecision;
use codex_app_server_protocol::FileChangeRequestApprovalResponse;
use codex_app_server_protocol::GrantedPermissionProfile;
use codex_app_server_protocol::NetworkApprovalProtocol;
use codex_app_server_protocol::NetworkPolicyRuleAction;
use codex_app_server_protocol::PermissionGrantScope;
use codex_app_server_protocol::PermissionsRequestApprovalResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_protocol::request_permissions::RequestPermissionProfile as CoreRequestPermissionProfile;
use serde::Serialize;
use serde_json::Value;
use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApprovalAction {
    Choose(usize),
    Edit,
    Explain,
    Select(ApprovalSelectionDirection),
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApprovalSelectionDirection {
    Previous,
    Next,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingApproval {
    request_id: RequestId,
    title: String,
    details: Vec<String>,
    edit_prompt: String,
    options: Vec<ApprovalOption>,
    selected_option: usize,
    scroll_offset: Cell<usize>,
    scroll_max: Cell<usize>,
}

#[derive(Debug, Clone, PartialEq)]
struct ApprovalOption {
    label: String,
    decision: ApprovalDecision,
}

#[derive(Debug, Clone, PartialEq)]
enum ApprovalDecision {
    Command(CommandExecutionApprovalDecision),
    FileChange(FileChangeApprovalDecision),
    Permissions(PermissionsRequestApprovalResponse),
}

impl PendingApproval {
    pub(super) fn from_request(request: &ServerRequest) -> Result<Option<Self>, String> {
        match request {
            ServerRequest::CommandExecutionRequestApproval { request_id, params } => {
                let title = params
                    .command
                    .as_ref()
                    .map(|command| format!("Run command: {command}"))
                    .or_else(|| {
                        params.network_approval_context.as_ref().map(|context| {
                            format!(
                                "Connect to {}://{}",
                                protocol_label(context.protocol),
                                context.host
                            )
                        })
                    })
                    .unwrap_or_else(|| "Run command".to_string());
                let mut details = context_details(
                    params.reason.as_deref(),
                    params
                        .cwd
                        .as_ref()
                        .map(|cwd| format!("Working directory: {cwd}")),
                    params.environment_id.as_deref(),
                );
                if let Some(context) = params.network_approval_context.as_ref() {
                    details.push(format!(
                        "Network target: {}://{}",
                        protocol_label(context.protocol),
                        context.host
                    ));
                }
                if let Some(actions) = params.command_actions.as_ref() {
                    details.push(json_detail("Command actions", actions));
                }
                if let Some(permissions) = params.additional_permissions.as_ref() {
                    details.push(json_detail(
                        "Requested additional per-command permissions",
                        permissions,
                    ));
                }
                if let Some(amendment) = params.proposed_execpolicy_amendment.as_ref() {
                    details.push(json_detail("Proposed persistent command rule", amendment));
                }
                if let Some(amendments) = params.proposed_network_policy_amendments.as_ref() {
                    details.push(json_detail("Proposed persistent network rules", amendments));
                }
                let options = params
                    .available_decisions
                    .clone()
                    .unwrap_or_else(|| default_command_decisions(params))
                    .into_iter()
                    .map(|decision| ApprovalOption {
                        label: command_decision_label(&decision),
                        decision: ApprovalDecision::Command(decision),
                    })
                    .collect::<Vec<_>>();
                if options.is_empty() {
                    return Err("command approval request has no available decisions".to_string());
                }
                let command = params.command.clone().unwrap_or_else(|| title.clone());
                Ok(Some(Self {
                    request_id: request_id.clone(),
                    title,
                    details,
                    edit_prompt: format!("Revise and retry this command:\n{command}"),
                    options,
                    selected_option: 0,
                    scroll_offset: Cell::new(0),
                    scroll_max: Cell::new(0),
                }))
            }
            ServerRequest::FileChangeRequestApproval { request_id, params } => Ok(Some(Self {
                request_id: request_id.clone(),
                title: format!("Apply file changes: {}", params.item_id),
                details: context_details(
                    params.reason.as_deref(),
                    params
                        .grant_root
                        .as_ref()
                        .map(|root| format!("Grant root: {}", root.display())),
                    /*environment_id*/ None,
                ),
                edit_prompt: format!(
                    "Revise the requested file changes before trying again: {}",
                    params.item_id
                ),
                options: vec![
                    ApprovalOption {
                        label: "Apply once".to_string(),
                        decision: ApprovalDecision::FileChange(FileChangeApprovalDecision::Accept),
                    },
                    ApprovalOption {
                        label: "Apply for this session".to_string(),
                        decision: ApprovalDecision::FileChange(
                            FileChangeApprovalDecision::AcceptForSession,
                        ),
                    },
                    ApprovalOption {
                        label: "Deny".to_string(),
                        decision: ApprovalDecision::FileChange(FileChangeApprovalDecision::Decline),
                    },
                    ApprovalOption {
                        label: "Deny and interrupt".to_string(),
                        decision: ApprovalDecision::FileChange(FileChangeApprovalDecision::Cancel),
                    },
                ],
                selected_option: 0,
                scroll_offset: Cell::new(0),
                scroll_max: Cell::new(0),
            })),
            ServerRequest::PermissionsRequestApproval { request_id, params } => {
                let requested = CoreRequestPermissionProfile::try_from(params.permissions.clone())
                    .map_err(|err| {
                        format!("failed to localize requested filesystem paths: {err}")
                    })?;
                let mut details = context_details(
                    params.reason.as_deref(),
                    Some(format!(
                        "Working directory: {}",
                        params.cwd.as_path().display()
                    )),
                    params.environment_id.as_deref(),
                );
                details.push(json_detail("Requested permissions", &params.permissions));
                let granted = granted_permission_profile_from_request(requested);
                Ok(Some(Self {
                    request_id: request_id.clone(),
                    title: "Grant permissions".to_string(),
                    details,
                    edit_prompt: "Revise the requested permissions before trying again".to_string(),
                    options: vec![
                        ApprovalOption {
                            label: "Grant for this turn".to_string(),
                            decision: ApprovalDecision::Permissions(
                                PermissionsRequestApprovalResponse {
                                    permissions: granted.clone(),
                                    scope: PermissionGrantScope::Turn,
                                    strict_auto_review: None,
                                },
                            ),
                        },
                        ApprovalOption {
                            label: "Grant for this session".to_string(),
                            decision: ApprovalDecision::Permissions(
                                PermissionsRequestApprovalResponse {
                                    permissions: granted.clone(),
                                    scope: PermissionGrantScope::Session,
                                    strict_auto_review: None,
                                },
                            ),
                        },
                        ApprovalOption {
                            label: "Grant for this turn; review each command".to_string(),
                            decision: ApprovalDecision::Permissions(
                                PermissionsRequestApprovalResponse {
                                    permissions: granted,
                                    scope: PermissionGrantScope::Turn,
                                    strict_auto_review: Some(true),
                                },
                            ),
                        },
                        ApprovalOption {
                            label: "Deny".to_string(),
                            decision: ApprovalDecision::Permissions(
                                PermissionsRequestApprovalResponse {
                                    permissions: GrantedPermissionProfile::default(),
                                    scope: PermissionGrantScope::Turn,
                                    strict_auto_review: None,
                                },
                            ),
                        },
                    ],
                    selected_option: 0,
                    scroll_offset: Cell::new(0),
                    scroll_max: Cell::new(0),
                }))
            }
            ServerRequest::ExecCommandApproval { .. }
            | ServerRequest::ApplyPatchApproval { .. }
            | ServerRequest::ToolRequestUserInput { .. }
            | ServerRequest::DynamicToolCall { .. }
            | ServerRequest::ChatgptAuthTokensRefresh { .. }
            | ServerRequest::CurrentTimeRead { .. }
            | ServerRequest::AttestationGenerate { .. }
            | ServerRequest::McpServerElicitationRequest { .. } => Ok(None),
        }
    }

    pub(super) fn request_id(&self) -> RequestId {
        self.request_id.clone()
    }

    pub(super) fn title(&self) -> &str {
        &self.title
    }

    pub(super) fn details(&self) -> &[String] {
        &self.details
    }

    pub(super) fn explanation(&self) -> String {
        std::iter::once(self.title.as_str())
            .chain(self.details.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" - ")
    }

    pub(super) fn edit_prompt(&self) -> &str {
        &self.edit_prompt
    }

    pub(super) fn options(&self) -> impl Iterator<Item = (usize, &str)> {
        self.options
            .iter()
            .enumerate()
            .map(|(index, option)| (index, option.label.as_str()))
    }

    pub(super) fn option_count(&self) -> usize {
        self.options.len()
    }

    pub(super) fn selected_option(&self) -> usize {
        self.selected_option
    }

    pub(super) fn move_selection(&mut self, direction: ApprovalSelectionDirection) {
        self.selected_option = match direction {
            ApprovalSelectionDirection::Previous => self
                .selected_option
                .checked_sub(1)
                .unwrap_or_else(|| self.options.len().saturating_sub(1)),
            ApprovalSelectionDirection::Next => {
                self.selected_option.saturating_add(1) % self.options.len()
            }
        };
    }

    pub(super) fn scroll_offset(&self) -> usize {
        self.scroll_offset.get()
    }

    pub(super) fn set_scroll_max(&self, scroll_max: usize) {
        self.scroll_max.set(scroll_max);
        self.scroll_offset
            .set(self.scroll_offset.get().min(scroll_max));
    }

    pub(super) fn scroll_up(&self, amount: usize) {
        self.scroll_offset
            .set(self.scroll_offset.get().saturating_sub(amount));
    }

    pub(super) fn scroll_down(&self, amount: usize) {
        self.scroll_offset.set(
            self.scroll_offset
                .get()
                .saturating_add(amount)
                .min(self.scroll_max.get()),
        );
    }

    pub(super) fn denial_index(&self) -> Option<usize> {
        self.options
            .iter()
            .position(|option| option.decision.is_safe_denial())
    }

    pub(super) fn is_denial(&self, option_index: usize) -> bool {
        self.options[option_index].decision.is_denial()
    }

    pub(super) fn result(&self, option_index: usize) -> serde_json::Result<Value> {
        match &self.options[option_index].decision {
            ApprovalDecision::Command(decision) => {
                serde_json::to_value(CommandExecutionRequestApprovalResponse {
                    decision: decision.clone(),
                })
            }
            ApprovalDecision::FileChange(decision) => {
                serde_json::to_value(FileChangeRequestApprovalResponse {
                    decision: decision.clone(),
                })
            }
            ApprovalDecision::Permissions(response) => serde_json::to_value(response),
        }
    }
}

impl ApprovalDecision {
    fn is_safe_denial(&self) -> bool {
        match self {
            Self::Command(
                CommandExecutionApprovalDecision::Decline
                | CommandExecutionApprovalDecision::Cancel,
            )
            | Self::FileChange(
                FileChangeApprovalDecision::Decline | FileChangeApprovalDecision::Cancel,
            ) => true,
            Self::Permissions(response) => {
                response.permissions == GrantedPermissionProfile::default()
            }
            Self::Command(
                CommandExecutionApprovalDecision::Accept
                | CommandExecutionApprovalDecision::AcceptForSession
                | CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment { .. }
                | CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment { .. },
            )
            | Self::FileChange(
                FileChangeApprovalDecision::Accept | FileChangeApprovalDecision::AcceptForSession,
            ) => false,
        }
    }

    fn is_denial(&self) -> bool {
        match self {
            Self::Command(
                CommandExecutionApprovalDecision::Decline
                | CommandExecutionApprovalDecision::Cancel,
            )
            | Self::FileChange(
                FileChangeApprovalDecision::Decline | FileChangeApprovalDecision::Cancel,
            ) => true,
            Self::Command(CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
                network_policy_amendment,
            }) => network_policy_amendment.action == NetworkPolicyRuleAction::Deny,
            Self::Permissions(response) => {
                response.permissions == GrantedPermissionProfile::default()
            }
            Self::Command(
                CommandExecutionApprovalDecision::Accept
                | CommandExecutionApprovalDecision::AcceptForSession
                | CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment { .. },
            )
            | Self::FileChange(
                FileChangeApprovalDecision::Accept | FileChangeApprovalDecision::AcceptForSession,
            ) => false,
        }
    }
}

fn context_details(
    reason: Option<&str>,
    context: Option<String>,
    environment_id: Option<&str>,
) -> Vec<String> {
    let mut details = Vec::new();
    if let Some(reason) = reason.filter(|reason| !reason.is_empty()) {
        details.push(format!("Reason: {reason}"));
    }
    details.extend(context);
    if let Some(environment_id) = environment_id {
        details.push(format!("Environment: {environment_id}"));
    }
    if details.is_empty() {
        details.push("Approve or deny this backend request.".to_string());
    }
    details
}

fn json_detail(label: &str, value: &impl Serialize) -> String {
    let value = serde_json::to_value(value)
        .map(inline_json)
        .unwrap_or_else(|_| "<unavailable>".to_string());
    format!("{label}: {value}")
}

fn inline_json(value: Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(&value).unwrap_or_default(),
        Value::Array(values) => format!(
            "[{}]",
            values
                .into_iter()
                .map(inline_json)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(values) => format!(
            "{{ {} }}",
            values
                .into_iter()
                .map(|(key, value)| format!("{key}: {}", inline_json(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn protocol_label(protocol: NetworkApprovalProtocol) -> &'static str {
    match protocol {
        NetworkApprovalProtocol::Http => "http",
        NetworkApprovalProtocol::Https => "https",
        NetworkApprovalProtocol::Socks5Tcp => "socks5-tcp",
        NetworkApprovalProtocol::Socks5Udp => "socks5-udp",
    }
}

fn default_command_decisions(
    params: &CommandExecutionRequestApprovalParams,
) -> Vec<CommandExecutionApprovalDecision> {
    if params.network_approval_context.is_some() {
        let mut decisions = vec![
            CommandExecutionApprovalDecision::Accept,
            CommandExecutionApprovalDecision::AcceptForSession,
        ];
        if let Some(amendment) = params
            .proposed_network_policy_amendments
            .iter()
            .flatten()
            .find(|amendment| amendment.action == NetworkPolicyRuleAction::Allow)
        {
            decisions.push(
                CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
                    network_policy_amendment: amendment.clone(),
                },
            );
        }
        decisions.push(CommandExecutionApprovalDecision::Cancel);
        return decisions;
    }
    if params.additional_permissions.is_some() {
        return vec![
            CommandExecutionApprovalDecision::Accept,
            CommandExecutionApprovalDecision::Cancel,
        ];
    }
    let mut decisions = vec![CommandExecutionApprovalDecision::Accept];
    if let Some(amendment) = params.proposed_execpolicy_amendment.clone() {
        decisions.push(
            CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment {
                execpolicy_amendment: amendment,
            },
        );
    }
    decisions.push(CommandExecutionApprovalDecision::Cancel);
    decisions
}

fn command_decision_label(decision: &CommandExecutionApprovalDecision) -> String {
    match decision {
        CommandExecutionApprovalDecision::Accept => "Run once".to_string(),
        CommandExecutionApprovalDecision::AcceptForSession => "Run for this session".to_string(),
        CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment {
            execpolicy_amendment,
        } => format!(
            "Run and always allow command prefix: {}",
            execpolicy_amendment.command.join(" ")
        ),
        CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
            network_policy_amendment,
        } => format!(
            "{} host permanently: {}",
            match network_policy_amendment.action {
                NetworkPolicyRuleAction::Allow => "Allow",
                NetworkPolicyRuleAction::Deny => "Deny",
            },
            network_policy_amendment.host
        ),
        CommandExecutionApprovalDecision::Decline => "Deny".to_string(),
        CommandExecutionApprovalDecision::Cancel => "Deny and interrupt".to_string(),
    }
}
