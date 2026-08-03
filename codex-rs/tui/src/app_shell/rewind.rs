use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use crate::app_server_session::AppServerStartedThread;
use crate::app_server_session::ForkGoalContinuation;
use crate::legacy_core::config::Config;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::UserInput;
use codex_protocol::ThreadId;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RewindAnchor {
    pub(super) before_turn_id: String,
}

impl RewindAnchor {
    pub(super) fn for_opening_item(turn_id: &str, item: &ThreadItem) -> Option<Self> {
        let ThreadItem::UserMessage { content, .. } = item else {
            return None;
        };
        match content.as_slice() {
            [
                UserInput::Text {
                    text,
                    text_elements,
                },
            ] if !text.trim().is_empty() && text_elements.is_empty() => Some(Self {
                before_turn_id: turn_id.to_string(),
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RewindPoint {
    source_thread_id: ThreadId,
    before_turn_id: String,
    transcript_index: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) enum RewindState {
    #[default]
    Idle,
    Editing(RewindPoint),
    Forking(RewindPoint),
}

impl RewindState {
    pub(super) fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }

    pub(super) fn is_editing(&self) -> bool {
        matches!(self, Self::Editing(_))
    }

    pub(super) fn is_forking(&self) -> bool {
        matches!(self, Self::Forking(_))
    }
}

impl ShellState {
    pub(super) fn selected_transcript_is_rewindable(&self) -> bool {
        self.transcript_selection
            .and_then(|selected| self.transcript.get(selected))
            .is_some_and(|line| line.rewind_anchor.is_some())
    }

    pub(super) fn begin_rewind_edit(&mut self) {
        let Some(selected) = self.transcript_selection else {
            return;
        };
        let Some((prompt, anchor)) = self.transcript.get(selected).and_then(|line| {
            line.rewind_anchor
                .as_ref()
                .map(|anchor| (line.text.clone(), anchor.clone()))
        }) else {
            self.push_status("select an opening text-only user prompt to branch from");
            return;
        };
        let blocker = if self.session_unavailable_reason.is_some() {
            Some("choose an available session before branching")
        } else if self.active_turn_id.is_some() {
            Some("finish or interrupt the active turn before branching")
        } else if self.has_pending_shell_command() {
            Some("finish or cancel the shell command before branching")
        } else if self.has_pending_backend_actions() {
            Some("wait for background work to finish before branching")
        } else if self.pending_approval.is_some()
            || self.pending_elicitation.is_some()
            || self.pending_user_input.is_some()
            || !self.queued_interactive_requests.is_empty()
        {
            Some("resolve the pending request before branching")
        } else if self.composer.has_queued_messages() {
            Some("finish queued messages before branching")
        } else if !self.composer.is_empty() {
            Some("send or clear the message draft before branching")
        } else {
            None
        };
        if let Some(message) = blocker {
            self.push_status(message);
            return;
        }

        self.composer.set_text(prompt);
        self.slash_command_popup.reset();
        self.transcript_selection = None;
        self.session_list.focused = false;
        self.settings.focused = false;
        self.agents_focused = false;
        self.rewind = RewindState::Editing(RewindPoint {
            source_thread_id: self.thread_id,
            before_turn_id: anchor.before_turn_id,
            transcript_index: selected,
        });
        self.status = "editing conversation branch".to_string();
    }

    pub(super) fn handle_rewind_key<S>(
        &mut self,
        key: KeyEvent,
        config: &Config,
        app_server: &S,
    ) -> bool
    where
        S: AppShellBackend,
    {
        if self.rewind.is_forking() {
            self.push_status("wait for the conversation branch to finish");
            return false;
        }
        if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE {
            let RewindState::Editing(point) = std::mem::take(&mut self.rewind) else {
                return false;
            };
            self.composer.clear();
            self.transcript_selection =
                (point.transcript_index < self.transcript.len()).then_some(point.transcript_index);
            self.status = "branch edit canceled".to_string();
            return false;
        }
        if let Some(action) = crate::text_input::text_input_action_from_key(key) {
            self.composer.apply_text_input_action(action);
            return false;
        }

        match key.code {
            KeyCode::Enter if super::is_composer_newline_key(key) => {
                let result = self.composer.insert_newline();
                self.report_composer_insert(result);
            }
            KeyCode::Enter => self.submit_rewind_edit(config, app_server),
            KeyCode::Up => {
                self.composer.move_up();
            }
            KeyCode::Down => {
                self.composer.move_down();
            }
            KeyCode::PageUp => self.scroll_transcript_up(super::TRANSCRIPT_PAGE_SCROLL_STEP),
            KeyCode::PageDown => self.scroll_transcript_down(super::TRANSCRIPT_PAGE_SCROLL_STEP),
            KeyCode::Char(ch)
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
            {
                let result = self.composer.insert_char(ch);
                self.report_composer_insert(result);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let result = self.composer.insert_str("    ");
                self.report_composer_insert(result);
            }
            KeyCode::Esc
            | KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Char(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => {}
        }
        false
    }

    fn submit_rewind_edit<S>(&mut self, config: &Config, app_server: &S)
    where
        S: AppShellBackend,
    {
        if self.reject_unavailable_session_action() {
            return;
        }
        if self.active_turn_id.is_some() {
            self.push_status("finish or interrupt the active turn before branching");
            return;
        }
        let prompt = self.composer.submission_text();
        if prompt.trim().is_empty() {
            self.push_status("enter a prompt for the new conversation branch");
            return;
        }
        if self.reject_oversized_composer() {
            return;
        }
        let RewindState::Editing(point) = &self.rewind else {
            return;
        };
        let point = point.clone();
        let session_config = match self.current_session_config(config) {
            Ok(config) => config,
            Err(err) => {
                self.report_action_error("failed to prepare conversation branch", err);
                return;
            }
        };
        let request = app_server.fork_thread_before_turn_in_background(
            session_config,
            point.source_thread_id,
            point.before_turn_id.clone(),
            ForkGoalContinuation::DeferUntilNextTurn,
        );
        let action_point = point.clone();
        if self.start_backend_action(
            ActionGroup::ConversationBranch,
            "branching conversation",
            async move {
                BackendActionResult::ConversationFork {
                    point: action_point,
                    prompt,
                    result: request.await,
                }
            },
        ) {
            self.rewind = RewindState::Forking(point);
        }
    }

    pub(super) async fn complete_rewind_fork<S>(
        &mut self,
        app_server: &S,
        point: RewindPoint,
        prompt: String,
        result: Result<AppServerStartedThread>,
    ) where
        S: AppShellBackend,
    {
        match result {
            Ok(started) => {
                self.complete_session_switch(started, app_server).await;
                self.push_status(format!(
                    "branched from {}; source session unchanged; working tree not reverted",
                    point.source_thread_id
                ));
                self.submit_prompt(app_server, prompt);
            }
            Err(err) => {
                self.rewind = RewindState::Editing(point);
                self.report_action_error("failed to branch conversation", err);
            }
        }
    }

    pub(super) fn recover_rewind_after_background_failure(&mut self) {
        let RewindState::Forking(point) = std::mem::take(&mut self.rewind) else {
            return;
        };
        self.rewind = RewindState::Editing(point);
    }

    pub(super) fn rewind_input_titles(&self, position: &str) -> Option<Vec<String>> {
        match self.rewind {
            RewindState::Idle => None,
            RewindState::Editing(_) => Some(vec![
                format!(
                    "BRANCH EDIT  SOURCE UNCHANGED  LATER TURNS OMITTED  FILES NOT REVERTED  {position}"
                ),
                format!("BRANCH EDIT  NO FILE REVERT  {position}"),
            ]),
            RewindState::Forking(_) => Some(vec![
                format!("BRANCHING  SOURCE UNCHANGED  FILES NOT REVERTED  {position}"),
                format!("BRANCHING  {position}"),
            ]),
        }
    }
}
