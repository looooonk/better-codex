use codex_app_server_protocol::McpServerElicitationAction;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ElicitationChoice {
    Accept,
    Decline,
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingElicitation {
    request_id: RequestId,
    title: String,
    detail: String,
    can_accept: bool,
}

impl PendingElicitation {
    pub(super) fn from_request(request: &ServerRequest) -> Option<Self> {
        let ServerRequest::McpServerElicitationRequest { request_id, params } = request else {
            return None;
        };

        let (summary, detail, can_accept) = match &params.request {
            McpServerElicitationRequest::Url { message, url, .. } => (
                "URL request".to_string(),
                format!("{message} - {url}"),
                true,
            ),
            McpServerElicitationRequest::Form {
                message,
                requested_schema,
                ..
            } => {
                let can_accept = requested_schema.properties.is_empty();
                let detail = if can_accept {
                    message.clone()
                } else {
                    format!("{message} - rich form fields require decline or cancel")
                };
                ("form request".to_string(), detail, can_accept)
            }
            McpServerElicitationRequest::OpenAiForm { message, .. } => (
                "OpenAI form request".to_string(),
                format!("{message} - rich form fields require decline or cancel"),
                false,
            ),
        };

        Some(Self {
            request_id: request_id.clone(),
            title: format!("MCP {}: {}", params.server_name, summary),
            detail,
            can_accept,
        })
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

    pub(super) fn can_accept(&self) -> bool {
        self.can_accept
    }

    pub(super) fn result(&self, choice: ElicitationChoice) -> Result<Value, String> {
        if choice == ElicitationChoice::Accept && !self.can_accept {
            return Err(
                "this MCP elicitation needs form fields the app shell cannot submit yet"
                    .to_string(),
            );
        }
        serde_json::to_value(McpServerElicitationRequestResponse {
            action: match choice {
                ElicitationChoice::Accept => McpServerElicitationAction::Accept,
                ElicitationChoice::Decline => McpServerElicitationAction::Decline,
                ElicitationChoice::Cancel => McpServerElicitationAction::Cancel,
            },
            content: None,
            meta: None,
        })
        .map_err(|err| format!("failed to serialize MCP elicitation response: {err}"))
    }
}
