use super::PendingApproval;
use super::PendingElicitation;
use super::PendingUserInput;
use super::ShellState;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;

const MAX_QUEUED_INTERACTIVE_REQUESTS: usize = 64;
const MAX_APPROVAL_TRANSCRIPT_TITLE_GRAPHEMES: usize = 160;
const MAX_APPROVAL_TRANSCRIPT_TITLE_LINES: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum PendingInteractiveRequest {
    Approval(PendingApproval),
    Elicitation(PendingElicitation),
    UserInput(PendingUserInput),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InteractiveRequestRemoval {
    Active,
    Queued,
    Missing,
}

impl PendingInteractiveRequest {
    pub(super) fn from_request(request: &ServerRequest) -> Result<Option<Self>, String> {
        if let Some(pending) = PendingApproval::from_request(request)? {
            Ok(Some(Self::Approval(pending)))
        } else if let Some(pending) = PendingElicitation::from_request(request) {
            Ok(Some(Self::Elicitation(pending)))
        } else {
            Ok(PendingUserInput::from_request(request).map(Self::UserInput))
        }
    }

    pub(super) fn request_id(&self) -> RequestId {
        match self {
            Self::Approval(pending) => pending.request_id(),
            Self::Elicitation(pending) => pending.request_id(),
            Self::UserInput(pending) => pending.request_id().clone(),
        }
    }

    pub(super) fn transcript_title(&self) -> String {
        match self {
            Self::Approval(pending) => {
                let mut source_lines = pending.title().lines();
                let mut title_lines = source_lines
                    .by_ref()
                    .take(MAX_APPROVAL_TRANSCRIPT_TITLE_LINES)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if source_lines.next().is_some()
                    && let Some(last) = title_lines.last_mut()
                {
                    last.push_str("...");
                }
                crate::text_formatting::truncate_text(
                    &title_lines.join("\n"),
                    MAX_APPROVAL_TRANSCRIPT_TITLE_GRAPHEMES,
                )
            }
            Self::Elicitation(pending) => pending.title().to_string(),
            Self::UserInput(pending) => pending.title().to_string(),
        }
    }

    fn requested_status(&self) -> String {
        match self {
            Self::Approval(_) => format!("approval requested: {}", self.transcript_title()),
            Self::Elicitation(pending) => format!("elicitation requested: {}", pending.title()),
            Self::UserInput(pending) => format!("input requested: {}", pending.title()),
        }
    }
}

impl ShellState {
    pub(super) fn receive_interactive_request(
        &mut self,
        pending: PendingInteractiveRequest,
    ) -> Result<(), PendingInteractiveRequest> {
        if !self.has_pending_interactive_request() {
            self.activate_interactive_request(pending);
        } else if self.queued_interactive_requests.len() < MAX_QUEUED_INTERACTIVE_REQUESTS {
            self.push_status(format!(
                "interactive request queued: {}",
                pending.transcript_title()
            ));
            self.queued_interactive_requests.push_back(pending);
        } else {
            return Err(pending);
        }
        Ok(())
    }

    pub(super) fn has_interactive_request(&self, request_id: &RequestId) -> bool {
        self.pending_approval
            .as_ref()
            .is_some_and(|pending| pending.request_id() == request_id.clone())
            || self
                .pending_elicitation
                .as_ref()
                .is_some_and(|pending| pending.request_id() == request_id.clone())
            || self
                .pending_user_input
                .as_ref()
                .is_some_and(|pending| pending.request_id() == request_id)
            || self
                .queued_interactive_requests
                .iter()
                .any(|pending| pending.request_id() == request_id.clone())
    }

    pub(super) fn remove_interactive_request(
        &mut self,
        request_id: &RequestId,
    ) -> InteractiveRequestRemoval {
        if self
            .pending_approval
            .as_ref()
            .is_some_and(|pending| pending.request_id() == request_id.clone())
        {
            self.pending_approval = None;
            return InteractiveRequestRemoval::Active;
        }
        if let Some(pending) = self
            .pending_elicitation
            .as_ref()
            .filter(|pending| pending.request_id() == request_id.clone())
        {
            let uses_composer = pending.uses_composer();
            self.pending_elicitation = None;
            if uses_composer {
                self.composer.clear();
            }
            return InteractiveRequestRemoval::Active;
        }
        if self
            .pending_user_input
            .as_ref()
            .is_some_and(|pending| pending.request_id() == request_id)
        {
            self.pending_user_input = None;
            self.composer.clear();
            return InteractiveRequestRemoval::Active;
        }
        let Some(index) = self
            .queued_interactive_requests
            .iter()
            .position(|pending| pending.request_id() == request_id.clone())
        else {
            return InteractiveRequestRemoval::Missing;
        };
        self.queued_interactive_requests.remove(index);
        InteractiveRequestRemoval::Queued
    }

    pub(super) fn activate_next_interactive_request(&mut self) {
        if let Some(pending) = self.queued_interactive_requests.pop_front() {
            self.activate_interactive_request(pending);
        }
    }

    pub(super) fn clear_interactive_requests(&mut self) {
        self.pending_approval = None;
        self.pending_elicitation = None;
        self.pending_user_input = None;
        self.queued_interactive_requests.clear();
    }

    fn has_pending_interactive_request(&self) -> bool {
        self.pending_approval.is_some()
            || self.pending_elicitation.is_some()
            || self.pending_user_input.is_some()
    }

    fn activate_interactive_request(&mut self, pending: PendingInteractiveRequest) {
        let status = pending.requested_status();
        self.close_overlays_for_interactive_request();
        match pending {
            PendingInteractiveRequest::Approval(pending) => {
                self.pending_approval = Some(pending);
            }
            PendingInteractiveRequest::Elicitation(pending) => {
                if pending.editing() {
                    self.composer
                        .set_text(pending.default_answer().unwrap_or_default());
                }
                self.pending_elicitation = Some(pending);
            }
            PendingInteractiveRequest::UserInput(pending) => {
                self.pending_user_input = Some(pending);
            }
        }
        self.push_status(status);
    }

    fn close_overlays_for_interactive_request(&mut self) {
        self.composer.finish_queued_message_edit();
        self.close_agent_log();
        self.close_tool_output();
        self.close_diff_view();
        self.selector = None;
        self.command_palette = None;
        self.pending_external_agent_import = None;
        self.pending_mcp_management = None;
        self.pending_plugin_management = None;
        self.pending_session_delete = None;
        self.safety_buffering.dismiss();
    }
}
