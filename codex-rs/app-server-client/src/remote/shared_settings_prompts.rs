//! Restrict approvals from servers that persist settings in the standard Codex home.
//! Keep request context so unrelated MCP forms and user answers pass through unchanged.
use crate::AppServerEvent;
use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_protocol::mcp_approval_meta as approval;
use serde_json::Value;
use std::collections::HashMap;
use std::io;

// The legacy tool/requestUserInput approval protocol encodes decisions as answer labels.
const MCP_APPROVAL_QUESTION_PREFIX: &str = "mcp_tool_call_approval_";
const REMEMBER_ANSWER: &str = "Allow and don't ask me again";

enum Prompt {
    Command,
    LegacyCommand,
    McpApproval,
    Suggestion,
    ApprovalQuestions,
}

#[derive(Default)]
pub(super) struct SharedSettingsPrompts {
    pending: HashMap<RequestId, Prompt>,
}

impl SharedSettingsPrompts {
    pub(super) fn observe(&mut self, event: &mut AppServerEvent) {
        match event {
            AppServerEvent::ServerRequest(request) => {
                if let Some(prompt) = restrict_options(request) {
                    self.pending.insert(request.id().clone(), prompt);
                }
            }
            AppServerEvent::ServerNotification(notification) => {
                if let ServerNotification::ServerRequestResolved(resolved) = notification.as_ref() {
                    self.pending.remove(&resolved.request_id);
                }
            }
            AppServerEvent::Disconnected { .. } | AppServerEvent::Lagged { .. } => {}
        }
    }

    pub(super) fn resolve(&mut self, id: &RequestId, result: &Value) -> io::Result<()> {
        if self
            .pending
            .get(id)
            .is_some_and(|prompt| prompt.would_persist(result))
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "This choice would change the standard Codex settings. Choose a one-time or session response.",
            ));
        }
        self.dismiss(id);
        Ok(())
    }

    pub(super) fn dismiss(&mut self, id: &RequestId) {
        self.pending.remove(id);
    }
}

fn restrict_options(request: &mut ServerRequest) -> Option<Prompt> {
    match request {
        ServerRequest::CommandExecutionRequestApproval { params, .. } => {
            params.proposed_execpolicy_amendment = None;
            params.proposed_network_policy_amendments = None;
            if let Some(decisions) = &mut params.available_decisions {
                decisions.retain(|decision| {
                    !matches!(
                        decision,
                        CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment { .. }
                            | CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment { .. }
                    )
                });
            }
            Some(Prompt::Command)
        }
        ServerRequest::ExecCommandApproval { .. } | ServerRequest::ApplyPatchApproval { .. } => {
            Some(Prompt::LegacyCommand)
        }
        ServerRequest::ToolRequestUserInput { params, .. } => {
            let mut restricted = false;
            for question in &mut params.questions {
                if question.id.starts_with(MCP_APPROVAL_QUESTION_PREFIX) {
                    if let Some(options) = &mut question.options {
                        options.retain(|option| option.label != REMEMBER_ANSWER);
                    }
                    restricted = true;
                }
            }
            restricted.then_some(Prompt::ApprovalQuestions)
        }
        ServerRequest::McpServerElicitationRequest { params, .. } => {
            let (McpServerElicitationRequest::Form { meta, .. }
            | McpServerElicitationRequest::OpenAiForm { meta, .. }
            | McpServerElicitationRequest::OpenAiElicitationForm { meta, .. }) =
                &mut params.request
            else {
                return None;
            };
            let meta = meta.as_mut()?.as_object_mut()?;
            let kind = match meta.get(approval::APPROVAL_KIND_KEY)?.as_str()? {
                approval::APPROVAL_KIND_MCP_TOOL_CALL => Prompt::McpApproval,
                approval::APPROVAL_KIND_TOOL_SUGGESTION => Prompt::Suggestion,
                _ => return None,
            };
            let persist = meta.get_mut(approval::PERSIST_KEY);
            match persist {
                Some(Value::Array(scopes)) => {
                    scopes.retain(|value| value.as_str() != Some(approval::PERSIST_ALWAYS));
                    if scopes.is_empty() {
                        meta.remove(approval::PERSIST_KEY);
                    }
                }
                Some(scope) if scope.as_str() == Some(approval::PERSIST_ALWAYS) => {
                    meta.remove(approval::PERSIST_KEY);
                }
                _ => {}
            }
            Some(kind)
        }
        _ => None,
    }
}

impl Prompt {
    fn would_persist(&self, result: &Value) -> bool {
        let persistent_meta = result
            .get("_meta")
            .and_then(|meta| meta.get(approval::PERSIST_KEY))
            .and_then(Value::as_str)
            == Some(approval::PERSIST_ALWAYS);
        match self {
            Self::Command => result.get("decision").is_some_and(|decision| {
                decision.get("acceptWithExecpolicyAmendment").is_some()
                    || decision.get("applyNetworkPolicyAmendment").is_some()
            }),
            Self::LegacyCommand => result.get("decision").is_some_and(|decision| {
                decision.get("approved_execpolicy_amendment").is_some()
                    || decision.get("network_policy_amendment").is_some()
                    || decision.as_str() == Some("approved_mcp_policy_amendment")
            }),
            Self::Suggestion => {
                result.get("action").and_then(Value::as_str) == Some("decline") && persistent_meta
            }
            Self::McpApproval => {
                result.get("action").and_then(Value::as_str) == Some("accept")
                    && (persistent_meta || persistent_answer(result.get("content")))
            }
            Self::ApprovalQuestions => persistent_answer(result.get("answers")),
        }
    }
}

fn persistent_answer(answers: Option<&Value>) -> bool {
    answers.and_then(Value::as_object).is_some_and(|answers| {
        answers.iter().any(|(key, value)| {
            if !key.starts_with(MCP_APPROVAL_QUESTION_PREFIX) {
                return false;
            }
            let value = value.get("answers").unwrap_or(value);
            value.as_str() == Some(REMEMBER_ANSWER)
                || value.as_array().is_some_and(|answers| {
                    answers
                        .iter()
                        .any(|answer| answer.as_str() == Some(REMEMBER_ANSWER))
                })
        })
    })
}

#[cfg(test)]
#[path = "shared_settings_prompts_tests.rs"]
mod tests;
