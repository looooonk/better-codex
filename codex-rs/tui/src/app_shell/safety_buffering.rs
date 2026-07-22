//! Safety-buffering state, modal behavior, and retry handling for the app shell.

use super::ShellState;
use super::TranscriptKind;
use super::TranscriptLine;
use super::backend::AppShellBackend;
use super::backend::AppShellTurnStart;
use super::format_user_inputs;
use super::is_unmodified_action_key;
use crate::app_server_session::ForkGoalContinuation;
use crate::legacy_core::config::Config;
use codex_app_server_protocol::ModelSafetyBufferingUpdatedNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_protocol::openai_models::ReasoningEffort;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::style::Stylize;
use ratatui::text::Line;

const SAFETY_BUFFERING_LEARN_MORE_URL: &str = "https://help.openai.com/en/articles/20001326";
const SAFETY_BUFFERING_HEADER: &str =
    "Our systems are thinking a bit more about this request before responding.";
const SAFETY_BUFFERING_MESSAGE_WITH_RETRY: &str = "Hang tight or retry with a faster model for a quicker response, though it may be less capable of handling complex requests.";
const SAFETY_BUFFERING_FOOTER: &str = "No action is required. Codex will keep waiting, and this menu will close when the response is ready.";

const LEGACY_SAFETY_ACCESS_BLOCK_PREFIX: &str =
    "Invalid prompt: we've limited access to this content for safety reasons.";
const BIO_POLICY_SAFETY_ACCESS_BLOCK_PREFIX: &str =
    "This content was flagged for possible biological risk.";
const SAFETY_ACCESS_NOTICE: &str = "This content can't be shown\n\nWe take extra caution with requests involving biological research and applications that could pose safety risks. Eligible researchers can apply for Trusted Access.\n\nTrusted Access: https://www.openai.com/form/trusted-access-for-biology-research/\nLearn more: https://help.openai.com/en/articles/20001326";

#[derive(Debug, Clone)]
struct SubmittedTurn {
    turn_id: String,
    params: AppShellTurnStart,
}

#[derive(Debug, Clone)]
struct ActiveSafetyBuffering {
    turn_id: String,
    faster_model: Option<String>,
    can_retry: bool,
    selected: usize,
    visible: bool,
}

#[derive(Debug, Default)]
pub(super) struct SafetyBufferingState {
    submitted_turn: Option<SubmittedTurn>,
    active: Option<ActiveSafetyBuffering>,
    output_started_turn_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SafetyBufferingAction {
    Retry,
    Dismiss,
    LearnMore,
}

impl SafetyBufferingState {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    fn record_submitted_turn(&mut self, turn_id: String, params: AppShellTurnStart) {
        self.submitted_turn = Some(SubmittedTurn { turn_id, params });
        self.active = None;
        self.output_started_turn_id = None;
    }

    fn reset_for_turn_start(&mut self, turn_id: &str) {
        self.active = None;
        self.output_started_turn_id = None;
        if self
            .submitted_turn
            .as_ref()
            .is_some_and(|submitted| submitted.turn_id != turn_id)
        {
            self.submitted_turn = None;
        }
    }

    fn clear_for_turn_completion(&mut self, turn_id: &str) {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.turn_id == turn_id)
            || self
                .submitted_turn
                .as_ref()
                .is_some_and(|submitted| submitted.turn_id == turn_id)
            || self.output_started_turn_id.as_deref() == Some(turn_id)
        {
            self.clear();
        }
    }

    fn clear_for_streaming(&mut self, turn_id: &str) -> bool {
        let matches_active = self
            .active
            .as_ref()
            .is_some_and(|active| active.turn_id == turn_id);
        if !matches_active {
            return false;
        }
        self.active = None;
        self.submitted_turn = None;
        self.output_started_turn_id = Some(turn_id.to_string());
        true
    }

    fn update(
        &mut self,
        active_turn_id: Option<&str>,
        notification: ModelSafetyBufferingUpdatedNotification,
    ) -> bool {
        if active_turn_id != Some(notification.turn_id.as_str()) {
            return false;
        }
        if self.output_started_turn_id.as_deref() == Some(notification.turn_id.as_str()) {
            return false;
        }
        if !notification.show_buffering_ui {
            let matched = self
                .active
                .as_ref()
                .is_some_and(|active| active.turn_id == notification.turn_id);
            if matched {
                self.active = None;
            }
            return matched;
        }

        let can_retry = notification.faster_model.is_some()
            && self
                .submitted_turn
                .as_ref()
                .is_some_and(|submitted| submitted.turn_id == notification.turn_id);
        let previous = self
            .active
            .as_ref()
            .filter(|active| active.turn_id == notification.turn_id);
        let retry_availability_changed =
            previous.is_some_and(|active| active.can_retry != can_retry);
        let selected = previous.map_or(0, |active| active.selected);
        let visible = previous.is_none_or(|active| active.visible) || retry_availability_changed;
        self.active = Some(ActiveSafetyBuffering {
            turn_id: notification.turn_id,
            faster_model: notification.faster_model.filter(|_| can_retry),
            can_retry,
            selected: if retry_availability_changed {
                0
            } else {
                selected
            },
            visible,
        });
        true
    }

    fn actions(active: &ActiveSafetyBuffering) -> Vec<SafetyBufferingAction> {
        let mut actions = Vec::with_capacity(3);
        if active.can_retry {
            actions.push(SafetyBufferingAction::Retry);
        }
        actions.extend([
            SafetyBufferingAction::Dismiss,
            SafetyBufferingAction::LearnMore,
        ]);
        actions
    }

    fn modal_lines(&self) -> Option<Vec<Line<'static>>> {
        let active = self.active.as_ref().filter(|active| active.visible)?;
        let actions = Self::actions(active);
        let mut lines = vec![Line::from(SAFETY_BUFFERING_HEADER.bold())];
        if active.can_retry {
            lines.extend([
                Line::default(),
                Line::from(SAFETY_BUFFERING_MESSAGE_WITH_RETRY.dim()),
            ]);
        }
        lines.push(Line::default());
        for (index, action) in actions.into_iter().enumerate() {
            let label = match action {
                SafetyBufferingAction::Retry => "Retry with a faster model",
                SafetyBufferingAction::Dismiss => "Dismiss and keep waiting",
                SafetyBufferingAction::LearnMore => "Learn more",
            };
            let marker = if index == active.selected {
                ">".cyan().bold()
            } else {
                " ".into()
            };
            lines.push(vec![marker, " ".into(), label.into()].into());
        }
        let shortcuts = if active.can_retry {
            "↑↓ / j k select  Enter confirm  r retry  d / Esc dismiss"
        } else {
            "↑↓ / j k select  Enter confirm  d / Esc dismiss"
        };
        lines.extend([
            Line::default(),
            Line::from(SAFETY_BUFFERING_FOOTER.dim()),
            Line::from(shortcuts.dim()),
        ]);
        Some(lines)
    }

    fn move_selection(&mut self, offset: isize) {
        let Some(active) = self.active.as_mut().filter(|active| active.visible) else {
            return;
        };
        let len = Self::actions(active).len();
        active.selected = active.selected.saturating_add_signed(offset).min(len - 1);
    }

    fn selected_action(&self) -> Option<SafetyBufferingAction> {
        let active = self.active.as_ref().filter(|active| active.visible)?;
        Self::actions(active).get(active.selected).copied()
    }

    fn click_key_at(&mut self, line: usize) -> Option<KeyCode> {
        let active = self.active.as_mut().filter(|active| active.visible)?;
        let actions = Self::actions(active);
        let action_start = if active.can_retry { 4 } else { 2 };
        let selected = line.checked_sub(action_start)?;
        if selected >= actions.len() {
            return None;
        }
        active.selected = selected;
        Some(KeyCode::Enter)
    }

    fn retry_action(&self) -> Option<SafetyBufferingAction> {
        self.active
            .as_ref()
            .filter(|active| active.visible && active.can_retry)
            .map(|_| SafetyBufferingAction::Retry)
    }

    pub(super) fn dismiss(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.visible = false;
        }
    }

    fn retry_payload(&self) -> Option<(SubmittedTurn, String)> {
        let active = self.active.as_ref()?;
        let submitted = self
            .submitted_turn
            .as_ref()
            .filter(|submitted| submitted.turn_id == active.turn_id)?;
        Some((submitted.clone(), active.faster_model.clone()?))
    }
}

impl ShellState {
    pub(super) fn record_safety_buffering_turn(
        &mut self,
        turn_id: String,
        params: AppShellTurnStart,
    ) {
        self.safety_buffering.record_submitted_turn(turn_id, params);
    }

    pub(super) fn reset_safety_buffering_for_turn_start(&mut self, turn_id: &str) {
        self.safety_buffering.reset_for_turn_start(turn_id);
    }

    pub(super) fn clear_safety_buffering_for_turn_completion(&mut self, turn_id: &str) {
        self.safety_buffering.clear_for_turn_completion(turn_id);
    }

    pub(super) fn clear_safety_buffering_for_streaming(&mut self, turn_id: &str) {
        if self.safety_buffering.clear_for_streaming(turn_id) {
            self.status = "thinking".to_string();
        }
    }

    pub(super) fn on_model_safety_buffering_updated(
        &mut self,
        notification: ModelSafetyBufferingUpdatedNotification,
    ) {
        if notification.thread_id != self.thread_id.to_string() {
            return;
        }
        let show_buffering_ui = notification.show_buffering_ui;
        if self
            .safety_buffering
            .update(self.active_turn_id.as_deref(), notification)
        {
            self.command_palette = None;
            self.selector = None;
            if self.pending_approval.is_some()
                || self.pending_elicitation.is_some()
                || self.pending_user_input.is_some()
            {
                self.safety_buffering.dismiss();
            }
            self.status = if show_buffering_ui {
                "waiting".to_string()
            } else {
                "thinking".to_string()
            };
        }
    }

    pub(super) fn safety_buffering_modal_lines(&self) -> Option<Vec<Line<'static>>> {
        self.safety_buffering.modal_lines()
    }

    pub(super) fn safety_buffering_click_key(&mut self, line: usize) -> Option<KeyCode> {
        self.safety_buffering.click_key_at(line)
    }

    pub(super) async fn handle_safety_buffering_key<S>(
        &mut self,
        key: KeyEvent,
        config: &Config,
        app_server: &mut S,
    ) where
        S: AppShellBackend,
    {
        if !is_unmodified_action_key(key) {
            return;
        }
        let action = match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.safety_buffering.move_selection(/*offset*/ -1);
                return;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.safety_buffering.move_selection(/*offset*/ 1);
                return;
            }
            KeyCode::Esc | KeyCode::Char('d') => Some(SafetyBufferingAction::Dismiss),
            KeyCode::Char('r') => self.safety_buffering.retry_action(),
            KeyCode::Enter => self.safety_buffering.selected_action(),
            _ => None,
        };
        match action {
            Some(SafetyBufferingAction::Retry) => {
                self.retry_safety_buffered_turn(config, app_server).await;
            }
            Some(SafetyBufferingAction::Dismiss) => self.safety_buffering.dismiss(),
            Some(SafetyBufferingAction::LearnMore) => {
                if let Err(err) = webbrowser::open(SAFETY_BUFFERING_LEARN_MORE_URL) {
                    self.push_error(format!("Failed to open safety information: {err}"));
                }
            }
            None => {}
        }
    }

    async fn retry_safety_buffered_turn<S>(&mut self, config: &Config, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        let Some((mut submitted, faster_model)) = self.safety_buffering.retry_payload() else {
            self.push_error("Failed to retry with a faster model: original turn is unavailable");
            return;
        };
        if self.active_turn_id.as_deref() != Some(submitted.turn_id.as_str()) {
            return;
        }
        if let Err(err) = app_server
            .turn_interrupt(self.thread_id, submitted.turn_id.clone())
            .await
        {
            self.push_error(format!("Failed to retry with a faster model: {err}"));
            return;
        }
        let source_thread = match app_server
            .thread_read_full_in_background(self.thread_id)
            .await
        {
            Ok(thread) => thread,
            Err(err) => {
                self.push_error(format!("Failed to retry with a faster model: {err}"));
                return;
            }
        };
        let additional_inputs = match safety_retry_additional_inputs(
            &source_thread.turns,
            submitted.turn_id.as_str(),
        ) {
            Ok(inputs) => inputs,
            Err(err) => {
                self.push_error(format!("Failed to retry with a faster model: {err}"));
                return;
            }
        };
        submitted.params.items.extend(additional_inputs);

        let retry_config = match self.current_session_config(config) {
            Ok(config) => config,
            Err(err) => {
                self.push_error(format!("Failed to retry with a faster model: {err}"));
                return;
            }
        };
        let started = match app_server
            .fork_thread_before_turn(
                retry_config,
                self.thread_id,
                submitted.turn_id.clone(),
                ForkGoalContinuation::DeferUntilNextTurn,
            )
            .await
        {
            Ok(started) => started,
            Err(err) => {
                self.push_error(format!("Failed to retry with a faster model: {err}"));
                return;
            }
        };
        let composer = self.composer.clone();
        self.complete_session_switch(started, app_server).await;
        self.composer = composer;

        let mut params = submitted.params;
        params.thread_id = self.thread_id;
        params.model = faster_model.clone();
        params.effort = Some(ReasoningEffort::Low);
        params.collaboration_mode = params.collaboration_mode.map(|mode| {
            mode.with_updates(
                Some(faster_model),
                Some(Some(ReasoningEffort::Low)),
                /*developer_instructions*/ None,
            )
        });

        let retry_display = format_user_inputs(&params.items);
        self.push_user(retry_display);
        self.status = "thinking".to_string();

        match app_server.turn_start(params.clone()).await {
            Ok(response) => {
                self.record_active_turn_started(response.turn.id.clone());
                self.record_safety_buffering_turn(response.turn.id, params);
            }
            Err(err) => {
                self.status = "error".to_string();
                self.push_error(format!("Failed to retry with a faster model: {err}"));
            }
        }
    }

    pub(super) fn handle_safety_access_error(&mut self, message: &str) -> bool {
        if !is_safety_access_error(message) {
            return false;
        }
        self.safety_buffering.clear();
        self.clear_active_turn();
        self.clear_interactive_requests();
        self.status = "blocked".to_string();
        self.push_line(TranscriptLine::new(
            TranscriptKind::Status,
            SAFETY_ACCESS_NOTICE,
        ));
        true
    }
}

fn safety_retry_additional_inputs(turns: &[Turn], turn_id: &str) -> Result<Vec<UserInput>, String> {
    let Some(turn_index) = turns.iter().position(|turn| turn.id == turn_id) else {
        return Err(format!(
            "interrupted turn {turn_id} is missing from the source thread"
        ));
    };
    if turn_index + 1 != turns.len() {
        return Err(format!(
            "interrupted turn {turn_id} is no longer the latest turn"
        ));
    }
    let turn = &turns[turn_index];
    if turn.status == TurnStatus::InProgress {
        return Err(format!("interrupted turn {turn_id} is still in progress"));
    }
    if let Some(previous_turn) = turns[..turn_index].last()
        && previous_turn.status == TurnStatus::InProgress
    {
        return Err(format!(
            "previous turn {} is still in progress",
            previous_turn.id
        ));
    }

    let mut user_messages = turn.items.iter().filter_map(|item| match item {
        ThreadItem::UserMessage { content, .. } => Some(content),
        _ => None,
    });
    user_messages.next();
    Ok(user_messages
        .flat_map(|content| {
            std::iter::once(UserInput::Text {
                text: "\n".to_string(),
                text_elements: Vec::new(),
            })
            .chain(content.iter().cloned())
        })
        .collect())
}

fn is_safety_access_error(message: &str) -> bool {
    if message.starts_with(LEGACY_SAFETY_ACCESS_BLOCK_PREFIX)
        || message.starts_with(BIO_POLICY_SAFETY_ACCESS_BLOCK_PREFIX)
    {
        return true;
    }
    serde_json::from_str::<serde_json::Value>(message).is_ok_and(|response| {
        response["error"]["code"].as_str() == Some("bio_policy")
            || response["error"]["message"]
                .as_str()
                .is_some_and(|message| {
                    message.starts_with(LEGACY_SAFETY_ACCESS_BLOCK_PREFIX)
                        || message.starts_with(BIO_POLICY_SAFETY_ACCESS_BLOCK_PREFIX)
                })
    })
}

#[cfg(test)]
#[path = "safety_buffering_tests.rs"]
mod tests;
