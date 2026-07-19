#[path = "elicitation_form.rs"]
mod form;

use super::ShellState;
use super::backend::AppShellBackend;
use super::interactive_requests::InteractiveRequestRemoval;
use codex_app_server_protocol::McpServerElicitationAction;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use form::ElicitationFieldView;
use form::ElicitationForm;
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

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
    form: Option<ElicitationForm>,
}

impl PendingElicitation {
    pub(super) fn from_request(request: &ServerRequest) -> Option<Self> {
        let ServerRequest::McpServerElicitationRequest { request_id, params } = request else {
            return None;
        };
        let (summary, detail, form) = match &params.request {
            McpServerElicitationRequest::Url { message, url, .. } => {
                ("URL request", format!("{message} - {url}"), None)
            }
            McpServerElicitationRequest::Form {
                message,
                requested_schema,
                ..
            } => (
                "form request",
                message.clone(),
                Some(ElicitationForm::from_schema(
                    &serde_json::to_value(requested_schema).ok()?,
                )),
            ),
            McpServerElicitationRequest::OpenAiForm {
                message,
                requested_schema,
                ..
            } => (
                "OpenAI form request",
                message.clone(),
                Some(ElicitationForm::from_schema(requested_schema)),
            ),
        };
        Some(Self {
            request_id: request_id.clone(),
            title: format!("MCP {}: {summary}", params.server_name),
            detail,
            form,
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

    pub(super) fn editing(&self) -> bool {
        self.form.as_ref().is_some_and(|form| !form.complete())
    }

    pub(super) fn uses_composer(&self) -> bool {
        self.form.is_some()
    }

    pub(super) fn field_view(&self) -> Option<ElicitationFieldView<'_>> {
        self.form.as_ref()?.field_view()
    }

    pub(super) fn default_answer(&self) -> Option<String> {
        self.form.as_ref()?.default_input()
    }

    pub(super) fn primary_action_label(&self) -> &'static str {
        self.form
            .as_ref()
            .map_or("Accept", ElicitationForm::action_label)
    }

    pub(super) fn result(&self, choice: ElicitationChoice) -> Result<Value, String> {
        if choice == ElicitationChoice::Accept && self.editing() {
            return Err("complete the current MCP form field before submitting".to_string());
        }
        let content = (choice == ElicitationChoice::Accept)
            .then(|| self.form.as_ref().map(ElicitationForm::content))
            .flatten();
        serde_json::to_value(McpServerElicitationRequestResponse {
            action: match choice {
                ElicitationChoice::Accept => McpServerElicitationAction::Accept,
                ElicitationChoice::Decline => McpServerElicitationAction::Decline,
                ElicitationChoice::Cancel => McpServerElicitationAction::Cancel,
            },
            content,
            meta: None,
        })
        .map_err(|err| format!("failed to serialize MCP elicitation response: {err}"))
    }

    pub(super) fn choice_at(&self, line: usize, column: usize) -> Option<ElicitationChoice> {
        if line != if self.editing() { 4 } else { 2 } {
            return None;
        }
        let primary = format!("{} ↵", self.primary_action_label());
        let (decline, cancel) = if self.editing() {
            ("Decline ^D", "Cancel Esc")
        } else {
            ("Decline d", "Cancel c")
        };
        let text = format!("   {primary}   {decline}   {cancel} ");
        [
            (primary.as_str(), ElicitationChoice::Accept),
            (decline, ElicitationChoice::Decline),
            (cancel, ElicitationChoice::Cancel),
        ]
        .into_iter()
        .find_map(|(label, choice)| {
            let start = UnicodeWidthStr::width(&text[..text.find(label)?]);
            (start..start + UnicodeWidthStr::width(label))
                .contains(&column)
                .then_some(choice)
        })
    }
}

impl ShellState {
    pub(super) async fn resolve_pending_elicitation<S>(
        &mut self,
        app_server: &mut S,
        choice: ElicitationChoice,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        if choice == ElicitationChoice::Accept
            && self
                .pending_elicitation
                .as_ref()
                .is_some_and(PendingElicitation::editing)
        {
            let Some(pending) = self.pending_elicitation.as_ref() else {
                return Ok(());
            };
            let mut next = pending.clone();
            let title = pending.title().to_string();
            let answer = self.composer.submission_text();
            let Some(form) = &mut next.form else {
                return Ok(());
            };
            if let Err(message) = form.answer(&answer) {
                self.push_error(message);
                return Ok(());
            }
            if !form.complete() {
                self.composer
                    .set_text(next.default_answer().unwrap_or_default());
                self.pending_elicitation = Some(next);
                self.push_decision_audit("elicitation", "field answered", &title);
                return Ok(());
            }
            let request_id = next.request_id();
            let result = next.result(choice).map_err(color_eyre::eyre::Report::msg)?;
            return self
                .finish_pending_elicitation(app_server, choice, request_id, result, title)
                .await;
        }
        let Some(pending) = self.pending_elicitation.as_ref() else {
            return Ok(());
        };
        let request_id = pending.request_id();
        let title = pending.title().to_string();
        let result = match pending.result(choice) {
            Ok(result) => result,
            Err(message) => {
                self.push_error(message);
                return Ok(());
            }
        };
        self.finish_pending_elicitation(app_server, choice, request_id, result, title)
            .await
    }

    async fn finish_pending_elicitation<S>(
        &mut self,
        app_server: &mut S,
        choice: ElicitationChoice,
        request_id: RequestId,
        result: Value,
        title: String,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        app_server
            .resolve_server_request(request_id.clone(), result)
            .await
            .wrap_err("failed to resolve app-server MCP elicitation request")?;
        let removal = self.remove_interactive_request(&request_id);
        let decision = match choice {
            ElicitationChoice::Accept => "accepted",
            ElicitationChoice::Decline => "declined",
            ElicitationChoice::Cancel => "cancelled",
        };
        self.push_decision_audit("elicitation", decision, &title);
        if removal == InteractiveRequestRemoval::Active {
            self.activate_next_interactive_request();
        }
        Ok(())
    }
}

pub(super) fn elicitation_choice_from_key(key: KeyEvent) -> Option<ElicitationChoice> {
    if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
        return None;
    }
    match key.code {
        KeyCode::Enter | KeyCode::Char('a' | 'A' | 'y' | 'Y') => Some(ElicitationChoice::Accept),
        KeyCode::Char('d' | 'D' | 'n' | 'N') => Some(ElicitationChoice::Decline),
        KeyCode::Esc | KeyCode::Char('c' | 'C') => Some(ElicitationChoice::Cancel),
        _ => None,
    }
}
