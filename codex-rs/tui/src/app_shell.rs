//! First-stage standalone TUI shell for the Better Codex fork.
//!
//! This intentionally avoids the inherited chat widget and owns a small app-like
//! fullscreen surface that talks to Codex through the app-server harness.

use crate::app::AppExitInfo;
use crate::app::ExitReason;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::AppServerStartedThread;
use crate::app_server_session::TurnPermissionsOverride;
use crate::legacy_core::config::Config;
use crate::resume_picker::SessionSelection;
use crate::session_state::ThreadSessionState;
use crate::token_usage::TokenUsage;
use crate::tui;
use crate::tui::TuiEvent;
use codex_app_server_protocol::UserInput;
use codex_protocol::ThreadId;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use std::collections::VecDeque;
use tokio::select;
use tokio_stream::StreamExt;

mod events;
mod render;
use render::draw_shell;

const MAX_TRANSCRIPT_LINES: usize = 400;

pub(crate) async fn run(
    tui: &mut tui::Tui,
    mut app_server: AppServerSession,
    config: Config,
    initial_prompt: Option<String>,
    session_selection: SessionSelection,
    startup_bootstrap: Option<crate::app_server_session::AppServerBootstrap>,
) -> Result<AppExitInfo> {
    tui.enter_alt_screen()
        .wrap_err("failed to enter fullscreen app shell")?;
    tui.frame_requester().schedule_frame();

    let bootstrap = match startup_bootstrap {
        Some(bootstrap) => bootstrap,
        None => app_server.bootstrap(&config).await?,
    };

    let started = start_selected_session(&mut app_server, &config, session_selection).await?;
    let mut shell = ShellState::new(started.session, bootstrap.default_model);
    for turn in started.turns {
        shell.push_system(format!("loaded previous turn {}", turn.id));
    }

    if let Some(prompt) = initial_prompt.filter(|prompt| !prompt.trim().is_empty()) {
        shell.submit_prompt(&mut app_server, prompt).await?;
        tui.frame_requester().schedule_frame();
    }

    let mut tui_events = tui.event_stream();
    let exit_reason = loop {
        select! {
            event = tui_events.next() => {
                let Some(event) = event else {
                    break ExitReason::UserRequested;
                };
                match event {
                    TuiEvent::Key(key) => {
                        if shell.handle_key(key, &mut app_server).await? {
                            break ExitReason::UserRequested;
                        }
                        tui.frame_requester().schedule_frame();
                    }
                    TuiEvent::Paste(text) => {
                        shell.insert_text(&text);
                        tui.frame_requester().schedule_frame();
                    }
                    TuiEvent::Resize | TuiEvent::Draw => {
                        draw_shell(tui, &shell)?;
                    }
                }
            }
            event = app_server.next_event() => {
                match event {
                    Some(event) => {
                        shell.handle_app_server_event(&mut app_server, event).await?;
                        tui.frame_requester().schedule_frame();
                    }
                    None => {
                        shell.push_system("app-server disconnected");
                        break ExitReason::Fatal("app-server disconnected".to_string());
                    }
                }
            }
        }
    };

    let _ = app_server.thread_unsubscribe(shell.thread_id).await;
    app_server
        .shutdown()
        .await
        .inspect_err(|err| {
            tracing::warn!("app-server shutdown failed: {err}");
        })
        .ok();

    Ok(AppExitInfo {
        token_usage: shell.token_usage.clone(),
        thread_id: Some(shell.thread_id),
        resume_hint: shell.resume_hint(),
        update_action: None,
        exit_reason,
    })
}

async fn start_selected_session(
    app_server: &mut AppServerSession,
    config: &Config,
    session_selection: SessionSelection,
) -> Result<AppServerStartedThread> {
    match session_selection {
        SessionSelection::StartFresh | SessionSelection::Exit => {
            app_server
                .start_thread_with_session_start_source(
                    config,
                    Some(codex_app_server_protocol::ThreadStartSource::Startup),
                )
                .await
        }
        SessionSelection::Resume(target) => {
            app_server
                .resume_thread(config.clone(), target.thread_id)
                .await
        }
        SessionSelection::Fork(target) => {
            app_server
                .fork_thread(config.clone(), target.thread_id)
                .await
        }
    }
}

#[derive(Debug, Clone)]
enum TranscriptLine {
    System(String),
    User(String),
    Assistant(String),
    Status(String),
    Error(String),
}

#[derive(Debug)]
struct ShellState {
    thread_id: ThreadId,
    thread_name: Option<String>,
    model: String,
    cwd: String,
    approval_policy: codex_app_server_protocol::AskForApproval,
    approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer,
    permission_profile: codex_protocol::models::PermissionProfile,
    runtime_workspace_roots: Vec<codex_utils_absolute_path::AbsolutePathBuf>,
    reasoning_effort: Option<codex_protocol::openai_models::ReasoningEffort>,
    service_tier: Option<String>,
    collaboration_mode: Option<Box<codex_protocol::config_types::CollaborationMode>>,
    personality: Option<codex_protocol::config_types::Personality>,
    transcript: VecDeque<TranscriptLine>,
    input: String,
    active_turn_id: Option<String>,
    streaming_assistant: String,
    status: String,
    token_usage: TokenUsage,
    model_context_window: Option<i64>,
}

impl ShellState {
    fn new(session: ThreadSessionState, fallback_model: String) -> Self {
        let model = if session.model.is_empty() {
            fallback_model
        } else {
            session.model.clone()
        };
        let mut shell = Self {
            thread_id: session.thread_id,
            thread_name: session.thread_name,
            model,
            cwd: session.cwd.to_string_lossy().to_string(),
            approval_policy: session.approval_policy,
            approvals_reviewer: session.approvals_reviewer,
            permission_profile: session.permission_profile,
            runtime_workspace_roots: session.runtime_workspace_roots,
            reasoning_effort: session.reasoning_effort,
            service_tier: session.service_tier,
            collaboration_mode: session.collaboration_mode,
            personality: session.personality,
            transcript: VecDeque::new(),
            input: String::new(),
            active_turn_id: None,
            streaming_assistant: String::new(),
            status: "ready".to_string(),
            token_usage: TokenUsage::default(),
            model_context_window: None,
        };
        shell.push_system("Better Codex app shell");
        shell
    }

    async fn handle_key(
        &mut self,
        key: KeyEvent,
        app_server: &mut AppServerSession,
    ) -> Result<bool> {
        if key.kind != KeyEventKind::Press {
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            return Ok(true);
        }
        match key.code {
            KeyCode::Esc => Ok(true),
            KeyCode::Enter => {
                let prompt = self.input.trim().to_string();
                self.input.clear();
                if !prompt.is_empty() {
                    self.submit_prompt(app_server, prompt).await?;
                }
                Ok(false)
            }
            KeyCode::Backspace => {
                self.input.pop();
                Ok(false)
            }
            KeyCode::Char(ch) => {
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    self.input.push(ch);
                }
                Ok(false)
            }
            KeyCode::Tab => {
                self.input.push_str("    ");
                Ok(false)
            }
            KeyCode::BackTab => Ok(false),
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => Ok(false),
        }
    }

    fn insert_text(&mut self, text: &str) {
        self.input.push_str(text);
    }

    async fn submit_prompt(
        &mut self,
        app_server: &mut AppServerSession,
        prompt: String,
    ) -> Result<()> {
        if self.active_turn_id.is_some() {
            self.push_system("wait for the current turn to finish before sending another message");
            return Ok(());
        }

        self.push_user(prompt.clone());
        self.status = "thinking".to_string();
        self.streaming_assistant.clear();
        let response = app_server
            .turn_start(
                self.thread_id,
                vec![UserInput::Text {
                    text: prompt,
                    text_elements: Vec::new(),
                }],
                self.cwd.clone().into(),
                self.approval_policy,
                self.approvals_reviewer,
                TurnPermissionsOverride::Preserve,
                &self.runtime_workspace_roots,
                self.model.clone(),
                self.reasoning_effort.clone(),
                None,
                Some(self.service_tier.clone()),
                self.collaboration_mode.as_deref().cloned(),
                self.personality,
                None,
            )
            .await?;
        self.active_turn_id = Some(response.turn.id);
        Ok(())
    }

    fn finish_streaming_assistant(&mut self) {
        if self.streaming_assistant.trim().is_empty() {
            return;
        }
        let message = std::mem::take(&mut self.streaming_assistant);
        self.push_assistant(message);
    }

    fn push_system(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::System(text.into()));
    }

    fn push_user(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::User(text.into()));
    }

    fn push_assistant(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::Assistant(text.into()));
    }

    fn push_status(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::Status(text.into()));
    }

    fn push_error(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::Error(text.into()));
    }

    fn push_line(&mut self, line: TranscriptLine) {
        self.transcript.push_back(line);
        while self.transcript.len() > MAX_TRANSCRIPT_LINES {
            self.transcript.pop_front();
        }
    }

    fn resume_hint(&self) -> Option<String> {
        let thread = self
            .thread_name
            .clone()
            .unwrap_or_else(|| self.thread_id.to_string());
        Some(format!("codex resume {thread}"))
    }

    #[cfg(test)]
    fn snapshot_fixture() -> Self {
        let mut shell = Self {
            thread_id: ThreadId::from_string("01900000-0000-7000-8000-000000000001")
                .expect("valid snapshot thread id"),
            thread_name: Some("stage-one".to_string()),
            model: "gpt-5-codex".to_string(),
            cwd: "/workspace/better-codex".to_string(),
            approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer::User,
            permission_profile: codex_protocol::models::PermissionProfile::default(),
            runtime_workspace_roots: Vec::new(),
            reasoning_effort: None,
            service_tier: None,
            collaboration_mode: None,
            personality: None,
            transcript: VecDeque::new(),
            input: "Summarize the new shell architecture".to_string(),
            active_turn_id: None,
            streaming_assistant: "The new shell owns the fullscreen surface.".to_string(),
            status: "thinking".to_string(),
            token_usage: TokenUsage {
                input_tokens: 1200,
                cached_input_tokens: 300,
                output_tokens: 240,
                reasoning_output_tokens: 80,
                total_tokens: 1440,
            },
            model_context_window: Some(200000),
        };
        shell.push_system("Better Codex app shell");
        shell.push_user("Create a divergent standalone TUI.");
        shell.push_assistant("Started a fullscreen app shell backed by app-server turns.");
        shell
    }
}

#[cfg(test)]
#[path = "app_shell_tests.rs"]
mod tests;
