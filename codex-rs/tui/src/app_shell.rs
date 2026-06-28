//! First-stage standalone TUI shell for the Better Codex fork.
//!
//! This intentionally avoids the inherited chat widget and owns a small app-like
//! fullscreen surface that talks to Codex through the app-server harness.

use crate::app::AppExitInfo;
use crate::app::ExitReason;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::AppServerStartedThread;
use crate::app_server_session::TurnPermissionsOverride;
use crate::app_server_session::app_server_rate_limit_snapshots;
use crate::clipboard_copy::ClipboardLease;
use crate::legacy_core::config::Config;
use crate::resume_picker::SessionSelection;
use crate::session_state::ThreadSessionState;
use crate::token_usage::TokenUsage;
use crate::tui;
use crate::tui::TuiEvent;
use crate::workspace_command::AppServerWorkspaceCommandRunner;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnPlanStep;
use codex_app_server_protocol::UserInput;
use codex_protocol::ThreadId;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::select;
use tokio_stream::StreamExt;

mod approval;
mod command_palette;
mod composer;
mod dashboard;
mod dashboard_rate_limits;
mod dashboard_workspace;
mod design;
mod elicitation;
mod events;
mod navigation;
mod render;
mod user_input;
mod workspace;
use approval::ApprovalAction;
use approval::ApprovalChoice;
use approval::PendingApproval;
use command_palette::CommandPaletteAction;
use command_palette::CommandPaletteContext;
use command_palette::CommandPaletteEntry;
use command_palette::CommandPaletteState;
use command_palette::command_palette_entries;
use composer::ComposerState;
use elicitation::ElicitationChoice;
use elicitation::PendingElicitation;
use navigation::AppShellRouteState;
use navigation::DashboardRoute;
use render::draw_shell;
use user_input::PendingUserInput;
use user_input::UserInputAdvance;
use workspace::WorkspaceGitStatus;

const MAX_TRANSCRIPT_LINES: usize = 400;
const TRANSCRIPT_PAGE_SCROLL_STEP: usize = 8;
const TRANSCRIPT_SELECTION_STEP: usize = 1;

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

    let workspace_command_runner = Arc::new(AppServerWorkspaceCommandRunner::new(
        app_server.request_handle(),
    ));

    let started = start_selected_session(&mut app_server, &config, session_selection).await?;
    let route_state = AppShellRouteState::load(config.codex_home.as_path());
    let mut shell = ShellState::new(
        started.session,
        bootstrap.default_model,
        config.codex_home.to_path_buf(),
        route_state.route,
    );
    shell.ingest_turn_history(started.turns);
    shell
        .refresh_workspace_status(workspace_command_runner.as_ref())
        .await;
    shell.refresh_rate_limits(&mut app_server).await;
    shell.refresh_goal_state(&mut app_server).await;

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
                        shell
                            .handle_app_server_event(
                                &mut app_server,
                                workspace_command_runner.as_ref(),
                                event,
                            )
                            .await?;
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptLine {
    kind: TranscriptKind,
    text: String,
}

impl TranscriptLine {
    fn new(kind: TranscriptKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptKind {
    System,
    User,
    Assistant,
    Plan,
    Tool,
    Diff,
    Output,
    Status,
    Audit,
    Error,
}

impl TranscriptKind {
    fn label(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "you",
            Self::Assistant => "codex",
            Self::Plan => "plan",
            Self::Tool => "tool",
            Self::Diff => "diff",
            Self::Output => "output",
            Self::Status => "status",
            Self::Audit => "audit",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolActivity {
    id: String,
    title: String,
    status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DiffSummary {
    files: usize,
    additions: usize,
    removals: usize,
}

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
    transcript_scroll: usize,
    transcript_scroll_max: Cell<usize>,
    transcript_selection: Option<usize>,
    command_palette: Option<CommandPaletteState>,
    codex_home: std::path::PathBuf,
    dashboard_route: DashboardRoute,
    composer: ComposerState,
    clipboard_lease: Option<ClipboardLease>,
    active_turn_id: Option<String>,
    pending_approval: Option<PendingApproval>,
    pending_elicitation: Option<PendingElicitation>,
    pending_user_input: Option<PendingUserInput>,
    streaming_assistant: String,
    streaming_plan: String,
    plan_explanation: Option<String>,
    plan_steps: Vec<TurnPlanStep>,
    active_goal: Option<ThreadGoal>,
    tool_activity: VecDeque<ToolActivity>,
    subagent_activity: VecDeque<ToolActivity>,
    latest_diff: Option<DiffSummary>,
    workspace_git_status: Option<WorkspaceGitStatus>,
    workspace_status_refresh_due: bool,
    rate_limits: Vec<RateLimitSnapshot>,
    rate_limit_reset_credits: Option<i64>,
    status: String,
    token_usage: TokenUsage,
    model_context_window: Option<i64>,
}

impl ShellState {
    fn new(
        session: ThreadSessionState,
        fallback_model: String,
        codex_home: std::path::PathBuf,
        dashboard_route: DashboardRoute,
    ) -> Self {
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
            transcript_scroll: 0,
            transcript_scroll_max: Cell::new(0),
            transcript_selection: None,
            command_palette: None,
            codex_home,
            dashboard_route,
            composer: ComposerState::default(),
            clipboard_lease: None,
            active_turn_id: None,
            pending_approval: None,
            pending_elicitation: None,
            pending_user_input: None,
            streaming_assistant: String::new(),
            streaming_plan: String::new(),
            plan_explanation: None,
            plan_steps: Vec::new(),
            active_goal: None,
            tool_activity: VecDeque::new(),
            subagent_activity: VecDeque::new(),
            latest_diff: None,
            workspace_git_status: None,
            workspace_status_refresh_due: false,
            rate_limits: Vec::new(),
            rate_limit_reset_credits: None,
            status: "ready".to_string(),
            token_usage: TokenUsage::default(),
            model_context_window: None,
        };
        shell.push_system("Better Codex app shell");
        shell
    }

    fn ingest_turn_history(&mut self, turns: Vec<Turn>) {
        if turns.is_empty() {
            return;
        }

        self.push_system(format!("loaded {} previous turns", turns.len()));
        for turn in turns {
            for item in turn.items {
                self.ingest_completed_item(item);
            }
            if let Some(error) = turn.error {
                self.push_error(error.message);
            }
        }
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
            if self.active_turn_id.is_some() {
                self.interrupt_active_turn(app_server).await?;
                return Ok(false);
            }
            return Ok(true);
        }
        if self.command_palette.is_some() {
            self.handle_command_palette_key(key, app_server).await?;
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('o')) {
            self.copy_selected_transcript_with(crate::clipboard_copy::copy_to_clipboard);
            return Ok(false);
        }
        if let Some(route) = dashboard_route_from_key(key) {
            self.set_dashboard_route(route);
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Left => {
                    self.set_dashboard_route(self.dashboard_route.previous());
                    return Ok(false);
                }
                KeyCode::Right => {
                    self.set_dashboard_route(self.dashboard_route.next());
                    return Ok(false);
                }
                _ => {}
            }
        }
        if self.transcript_selection.is_some()
            && let Some(handled) = self.handle_transcript_selection_key(key)
        {
            return Ok(handled);
        }
        if key.modifiers.contains(KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Up | KeyCode::Down)
        {
            self.select_latest_transcript_item();
            if matches!(key.code, KeyCode::Up) {
                self.move_transcript_selection_up(TRANSCRIPT_SELECTION_STEP);
            }
            return Ok(false);
        }
        if self.pending_approval.is_some()
            && let Some(action) = approval_action_from_key(key)
        {
            self.handle_pending_approval_action(app_server, action)
                .await?;
            return Ok(false);
        }
        if self.pending_elicitation.is_some()
            && let Some(choice) = elicitation_choice_from_key(key)
        {
            self.resolve_pending_elicitation(app_server, choice).await?;
            return Ok(false);
        }
        if self.pending_user_input.is_some() {
            return self.handle_user_input_key(key, app_server).await;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('p')) {
            self.open_command_palette();
            return Ok(false);
        }
        match key.code {
            KeyCode::Esc => Ok(true),
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.composer.insert_newline();
                } else {
                    let prompt = self.composer.submission_text();
                    if !prompt.is_empty() {
                        if self.active_turn_id.is_some() {
                            self.steer_active_turn(app_server, prompt).await?;
                        } else {
                            self.submit_prompt(app_server, prompt).await?;
                        }
                    }
                }
                Ok(false)
            }
            KeyCode::Backspace => {
                self.composer.backspace();
                Ok(false)
            }
            KeyCode::Up => {
                self.composer.move_up_or_recall_history();
                Ok(false)
            }
            KeyCode::Down => {
                self.composer.move_down_or_recall_history();
                Ok(false)
            }
            KeyCode::PageUp => {
                self.scroll_transcript_up(TRANSCRIPT_PAGE_SCROLL_STEP);
                Ok(false)
            }
            KeyCode::PageDown => {
                self.scroll_transcript_down(TRANSCRIPT_PAGE_SCROLL_STEP);
                Ok(false)
            }
            KeyCode::Home => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_transcript_to_top();
                } else {
                    self.composer.move_to_line_start();
                }
                Ok(false)
            }
            KeyCode::End => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_transcript_to_bottom();
                } else {
                    self.composer.move_to_line_end();
                }
                Ok(false)
            }
            KeyCode::Char(ch) => {
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    self.composer.insert_char(ch);
                }
                Ok(false)
            }
            KeyCode::Tab => {
                self.composer.insert_str("    ");
                Ok(false)
            }
            KeyCode::BackTab => {
                self.composer.insert_str("    ");
                Ok(false)
            }
            KeyCode::Left => {
                self.composer.move_left();
                Ok(false)
            }
            KeyCode::Right => {
                self.composer.move_right();
                Ok(false)
            }
            KeyCode::Delete => {
                self.composer.delete();
                Ok(false)
            }
            KeyCode::Insert
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

    async fn refresh_workspace_status(
        &mut self,
        runner: &dyn crate::workspace_command::WorkspaceCommandExecutor,
    ) {
        self.workspace_git_status =
            workspace::load_git_status(runner, std::path::Path::new(&self.cwd)).await;
        self.workspace_status_refresh_due = false;
    }

    async fn refresh_rate_limits(&mut self, app_server: &mut AppServerSession) {
        let Ok(response) = app_server.account_rate_limits().await else {
            return;
        };
        self.rate_limit_reset_credits = response
            .rate_limit_reset_credits
            .as_ref()
            .map(|credits| credits.available_count);
        self.rate_limits = app_server_rate_limit_snapshots(response);
    }

    async fn refresh_goal_state(&mut self, app_server: &mut AppServerSession) {
        let Ok(response) = app_server.thread_goal_get(self.thread_id).await else {
            return;
        };
        self.active_goal = response.goal;
    }

    fn apply_rate_limit_update(&mut self, snapshot: RateLimitSnapshot) {
        let Some(limit_id) = snapshot.limit_id.as_deref() else {
            if self.rate_limits.is_empty() {
                self.rate_limits.push(snapshot);
            } else {
                self.rate_limits[0] =
                    merge_rate_limit_snapshot(self.rate_limits[0].clone(), snapshot);
            }
            return;
        };
        if let Some(existing) = self
            .rate_limits
            .iter_mut()
            .find(|existing| existing.limit_id.as_deref() == Some(limit_id))
        {
            *existing = merge_rate_limit_snapshot(existing.clone(), snapshot);
        } else {
            self.rate_limits.push(snapshot);
        }
    }

    fn insert_text(&mut self, text: &str) {
        self.clear_transcript_selection();
        self.close_command_palette();
        self.composer.insert_str(text);
    }

    async fn handle_command_palette_key(
        &mut self,
        key: KeyEvent,
        app_server: &mut AppServerSession,
    ) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.close_command_palette();
            }
            KeyCode::Enter => {
                self.execute_selected_command_palette_action(app_server)
                    .await?;
            }
            KeyCode::Up => {
                let entries = self.command_palette_entries();
                if let Some(palette) = &mut self.command_palette {
                    palette.move_up(&entries);
                }
            }
            KeyCode::Down => {
                let entries = self.command_palette_entries();
                if let Some(palette) = &mut self.command_palette {
                    palette.move_down(&entries);
                }
            }
            KeyCode::Home => {
                self.command_palette = Some(CommandPaletteState::default());
            }
            KeyCode::End => {
                let entries = self.command_palette_entries();
                if let Some(palette) = &mut self.command_palette {
                    palette.select_last(&entries);
                }
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.close_command_palette();
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
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
            | KeyCode::Modifier(_)
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::PageUp
            | KeyCode::PageDown => {}
        }
        Ok(())
    }

    async fn execute_selected_command_palette_action(
        &mut self,
        app_server: &mut AppServerSession,
    ) -> Result<()> {
        let Some(palette) = &self.command_palette else {
            return Ok(());
        };
        let entries = self.command_palette_entries();
        let Some(entry) = entries.get(palette.selected()) else {
            self.close_command_palette();
            return Ok(());
        };
        if !entry.enabled {
            self.push_status(format!("{}: {}", entry.title, entry.detail));
            return Ok(());
        }
        let Some(action) = palette.selected_action(&entries) else {
            return Ok(());
        };
        self.close_command_palette();
        match action {
            CommandPaletteAction::CopyTranscript => {
                self.copy_selected_transcript_with(crate::clipboard_copy::copy_to_clipboard);
            }
            CommandPaletteAction::ClearTranscript => {
                self.clear_visible_transcript();
            }
            CommandPaletteAction::SelectLatestTranscript => {
                self.select_latest_transcript_item();
            }
            CommandPaletteAction::ScrollTranscriptTop => {
                self.scroll_transcript_to_top();
            }
            CommandPaletteAction::ScrollTranscriptBottom => {
                self.scroll_transcript_to_bottom();
            }
            CommandPaletteAction::InterruptTurn => {
                self.interrupt_active_turn(app_server).await?;
            }
            CommandPaletteAction::SwitchModel
            | CommandPaletteAction::ChangePermissions
            | CommandPaletteAction::ResumeThread
            | CommandPaletteAction::ForkThread
            | CommandPaletteAction::CompactContext => {}
        }
        Ok(())
    }

    fn open_command_palette(&mut self) {
        self.command_palette = Some(CommandPaletteState::default());
        self.clear_transcript_selection();
    }

    fn close_command_palette(&mut self) {
        self.command_palette = None;
    }

    fn set_dashboard_route(&mut self, route: DashboardRoute) {
        if self.dashboard_route == route {
            return;
        }

        self.dashboard_route = route;
        let route_state = AppShellRouteState { route };
        if let Err(err) = route_state.save(&self.codex_home) {
            tracing::warn!("failed to persist app shell route state: {err}");
        }
    }

    fn command_palette_entries(&self) -> Vec<CommandPaletteEntry> {
        command_palette_entries(CommandPaletteContext {
            active_turn: self.active_turn_id.is_some(),
            can_copy_transcript: self.transcript_copy_text().is_some(),
            has_transcript: !self.transcript.is_empty(),
        })
    }

    fn handle_transcript_selection_key(&mut self, key: KeyEvent) -> Option<bool> {
        match key.code {
            KeyCode::Esc => {
                self.clear_transcript_selection();
                Some(false)
            }
            KeyCode::Enter => {
                self.copy_selected_transcript_with(crate::clipboard_copy::copy_to_clipboard);
                Some(false)
            }
            KeyCode::Up => {
                self.move_transcript_selection_up(TRANSCRIPT_SELECTION_STEP);
                Some(false)
            }
            KeyCode::Down => {
                self.move_transcript_selection_down(TRANSCRIPT_SELECTION_STEP);
                Some(false)
            }
            KeyCode::PageUp => {
                self.scroll_transcript_up(TRANSCRIPT_PAGE_SCROLL_STEP);
                Some(false)
            }
            KeyCode::PageDown => {
                self.scroll_transcript_down(TRANSCRIPT_PAGE_SCROLL_STEP);
                Some(false)
            }
            KeyCode::Home => {
                self.select_first_transcript_item();
                Some(false)
            }
            KeyCode::End => {
                self.select_latest_transcript_item();
                Some(false)
            }
            KeyCode::Char('c') => {
                self.copy_selected_transcript_with(crate::clipboard_copy::copy_to_clipboard);
                Some(false)
            }
            KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
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
            | KeyCode::Modifier(_)
            | KeyCode::Tab
            | KeyCode::BackTab => Some(false),
        }
    }

    fn scroll_transcript_up(&mut self, rows: usize) {
        let scroll = self.transcript_scroll.saturating_add(rows);
        self.transcript_scroll = self.clamp_transcript_scroll(scroll);
    }

    fn scroll_transcript_down(&mut self, rows: usize) {
        self.transcript_scroll = self.transcript_scroll.saturating_sub(rows);
    }

    fn scroll_transcript_to_top(&mut self) {
        self.transcript_scroll = self.transcript_scroll_max.get();
    }

    fn scroll_transcript_to_bottom(&mut self) {
        self.transcript_scroll = 0;
    }

    fn clamp_transcript_scroll(&self, scroll: usize) -> usize {
        let max_scroll = self.transcript_scroll_max.get();
        if max_scroll == 0 {
            scroll
        } else {
            scroll.min(max_scroll)
        }
    }

    fn select_latest_transcript_item(&mut self) {
        self.transcript_selection = self.transcript.len().checked_sub(1);
        self.scroll_transcript_to_bottom();
    }

    fn select_first_transcript_item(&mut self) {
        self.transcript_selection = (!self.transcript.is_empty()).then_some(0);
        self.scroll_transcript_to_top();
    }

    fn clear_transcript_selection(&mut self) {
        self.transcript_selection = None;
    }

    fn clear_visible_transcript(&mut self) {
        self.transcript.clear();
        self.streaming_assistant.clear();
        self.streaming_plan.clear();
        self.transcript_scroll = 0;
        self.transcript_selection = None;
        self.push_system("visible transcript cleared");
    }

    fn move_transcript_selection_up(&mut self, rows: usize) {
        let selected = self
            .transcript_selection
            .unwrap_or_else(|| self.transcript.len().saturating_sub(1));
        self.transcript_selection = Some(selected.saturating_sub(rows));
        self.scroll_transcript_up(rows);
    }

    fn move_transcript_selection_down(&mut self, rows: usize) {
        let Some(selected) = self.transcript_selection else {
            self.select_latest_transcript_item();
            return;
        };
        let Some(max_index) = self.transcript.len().checked_sub(1) else {
            self.clear_transcript_selection();
            return;
        };
        self.transcript_selection = Some(selected.saturating_add(rows).min(max_index));
        self.scroll_transcript_down(rows);
    }

    fn copy_selected_transcript_with(
        &mut self,
        copy_fn: impl FnOnce(&str) -> Result<Option<ClipboardLease>, String>,
    ) {
        let Some((kind, text)) = self.transcript_copy_text() else {
            self.push_error("No assistant transcript item to copy");
            return;
        };
        let kind = kind.label();
        let text = text.to_string();
        match copy_fn(&text) {
            Ok(lease) => {
                self.clipboard_lease = lease;
                self.push_status(format!("copied {kind} transcript item"));
            }
            Err(error) => {
                self.push_error(format!("Copy failed: {error}"));
            }
        }
    }

    fn selected_transcript_copy_text(&self) -> Option<(TranscriptKind, &str)> {
        let selected = self.transcript_selection?;
        self.transcript
            .get(selected)
            .map(|line| (line.kind, line.text.as_str()))
    }

    fn transcript_copy_text(&self) -> Option<(TranscriptKind, &str)> {
        self.selected_transcript_copy_text().or_else(|| {
            self.transcript
                .iter()
                .rev()
                .find(|line| line.kind == TranscriptKind::Assistant)
                .map(|line| (line.kind, line.text.as_str()))
        })
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

        self.scroll_transcript_to_bottom();
        self.push_user(prompt.clone());
        self.status = "thinking".to_string();
        self.streaming_assistant.clear();
        self.streaming_plan.clear();
        let response = app_server
            .turn_start(
                self.thread_id,
                vec![UserInput::Text {
                    text: prompt.clone(),
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
        self.composer.remember_submission(&prompt);
        self.composer.clear();
        self.active_turn_id = Some(response.turn.id);
        Ok(())
    }

    async fn interrupt_active_turn(&mut self, app_server: &mut AppServerSession) -> Result<()> {
        let Some(turn_id) = self.active_turn_id.clone() else {
            self.push_status("no active turn to interrupt");
            return Ok(());
        };
        app_server
            .turn_interrupt(self.thread_id, turn_id.clone())
            .await
            .wrap_err("failed to interrupt active turn")?;
        self.status = "interrupted".to_string();
        self.push_decision_audit("turn", "interrupted", &turn_id);
        Ok(())
    }

    async fn steer_active_turn(
        &mut self,
        app_server: &mut AppServerSession,
        prompt: String,
    ) -> Result<()> {
        let Some(turn_id) = self.active_turn_id.clone() else {
            self.submit_prompt(app_server, prompt).await?;
            return Ok(());
        };
        app_server
            .turn_steer(
                self.thread_id,
                turn_id,
                vec![UserInput::Text {
                    text: prompt.clone(),
                    text_elements: Vec::new(),
                }],
            )
            .await
            .wrap_err("failed to steer active turn")?;
        self.scroll_transcript_to_bottom();
        self.push_user(prompt.clone());
        let audit_title = compact_multiline(prompt.clone()).unwrap_or_else(|| prompt.clone());
        self.push_decision_audit("turn", "steered", &audit_title);
        self.composer.remember_submission(&prompt);
        self.composer.clear();
        self.status = "thinking".to_string();
        Ok(())
    }

    async fn resolve_pending_approval(
        &mut self,
        app_server: &mut AppServerSession,
        choice: ApprovalChoice,
    ) -> Result<()> {
        let Some(pending) = self.pending_approval.as_ref() else {
            return Ok(());
        };
        let request_id = pending.request_id();
        let title = pending.title().to_string();
        let result = pending.result(choice)?;
        app_server
            .resolve_server_request(request_id, result)
            .await
            .wrap_err("failed to resolve app-server approval request")?;
        self.pending_approval = None;
        let decision = match choice {
            ApprovalChoice::Approve => "approved",
            ApprovalChoice::Deny => "denied",
        };
        self.push_decision_audit("approval", decision, &title);
        Ok(())
    }

    async fn handle_pending_approval_action(
        &mut self,
        app_server: &mut AppServerSession,
        action: ApprovalAction,
    ) -> Result<()> {
        match action {
            ApprovalAction::Choose(choice) => {
                self.resolve_pending_approval(app_server, choice).await
            }
            ApprovalAction::Edit => self.edit_pending_approval(app_server).await,
            ApprovalAction::Explain => {
                self.explain_pending_approval();
                Ok(())
            }
        }
    }

    async fn edit_pending_approval(&mut self, app_server: &mut AppServerSession) -> Result<()> {
        let Some(pending) = self.pending_approval.as_ref() else {
            return Ok(());
        };
        let title = pending.title().to_string();
        let edit_prompt = pending.edit_prompt().to_string();
        self.resolve_pending_approval(app_server, ApprovalChoice::Deny)
            .await?;
        self.seed_composer_with_edit_prompt(edit_prompt);
        self.push_decision_audit("approval", "edit", &title);
        Ok(())
    }

    fn explain_pending_approval(&mut self) {
        let Some(pending) = self.pending_approval.as_ref() else {
            return;
        };
        self.push_decision_audit("approval", "explained", &pending.explanation());
    }

    fn seed_composer_with_edit_prompt(&mut self, edit_prompt: String) {
        let composer_text = self.composer.text().trim();
        if composer_text.is_empty() {
            self.composer.set_text(edit_prompt);
        } else {
            self.composer
                .set_text(format!("{composer_text}\n\n{edit_prompt}"));
        }
    }

    async fn handle_user_input_key(
        &mut self,
        key: KeyEvent,
        app_server: &mut AppServerSession,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => Ok(true),
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.composer.insert_newline();
                } else {
                    self.resolve_pending_user_input(app_server).await?;
                }
                Ok(false)
            }
            KeyCode::Backspace => {
                self.composer.backspace();
                Ok(false)
            }
            KeyCode::Up => {
                self.composer.move_up_or_recall_history();
                Ok(false)
            }
            KeyCode::Down => {
                self.composer.move_down_or_recall_history();
                Ok(false)
            }
            KeyCode::Home => {
                self.composer.move_to_line_start();
                Ok(false)
            }
            KeyCode::End => {
                self.composer.move_to_line_end();
                Ok(false)
            }
            KeyCode::Char(ch) => {
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    self.composer.insert_char(ch);
                }
                Ok(false)
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.composer.insert_str("    ");
                Ok(false)
            }
            KeyCode::Left => {
                self.composer.move_left();
                Ok(false)
            }
            KeyCode::Right => {
                self.composer.move_right();
                Ok(false)
            }
            KeyCode::Delete => {
                self.composer.delete();
                Ok(false)
            }
            KeyCode::PageUp => {
                self.scroll_transcript_up(TRANSCRIPT_PAGE_SCROLL_STEP);
                Ok(false)
            }
            KeyCode::PageDown => {
                self.scroll_transcript_down(TRANSCRIPT_PAGE_SCROLL_STEP);
                Ok(false)
            }
            KeyCode::Insert
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

    async fn resolve_pending_user_input(
        &mut self,
        app_server: &mut AppServerSession,
    ) -> Result<()> {
        let answer = self.composer.submission_text();
        let Some(pending) = self.pending_user_input.as_ref() else {
            return Ok(());
        };
        let mut next_pending = pending.clone();
        let title = pending.title().to_string();
        match next_pending.answer_current(answer) {
            Ok(UserInputAdvance::Next) => {
                self.pending_user_input = Some(next_pending);
                self.composer.clear();
                self.push_decision_audit("tool input", "answered", &title);
            }
            Ok(UserInputAdvance::Complete { request_id, result }) => {
                app_server
                    .resolve_server_request(request_id, result)
                    .await
                    .wrap_err("failed to resolve app-server tool input request")?;
                self.pending_user_input = None;
                self.composer.clear();
                self.push_decision_audit("tool input", "submitted", &title);
            }
            Err(message) => {
                self.push_error(message);
            }
        }
        Ok(())
    }

    async fn resolve_pending_elicitation(
        &mut self,
        app_server: &mut AppServerSession,
        choice: ElicitationChoice,
    ) -> Result<()> {
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
        app_server
            .resolve_server_request(request_id, result)
            .await
            .wrap_err("failed to resolve app-server MCP elicitation request")?;
        self.pending_elicitation = None;
        let decision = match choice {
            ElicitationChoice::Accept => "accepted",
            ElicitationChoice::Decline => "declined",
            ElicitationChoice::Cancel => "cancelled",
        };
        self.push_decision_audit("elicitation", decision, &title);
        Ok(())
    }

    fn finish_streaming_assistant(&mut self) {
        if self.streaming_assistant.trim().is_empty() {
            return;
        }
        let message = std::mem::take(&mut self.streaming_assistant);
        self.push_assistant(message);
    }

    fn finish_streaming_plan(&mut self) {
        if self.streaming_plan.trim().is_empty() {
            return;
        }
        let plan = std::mem::take(&mut self.streaming_plan);
        self.push_plan(plan);
    }

    fn ingest_completed_item(&mut self, item: ThreadItem) {
        match item {
            ThreadItem::UserMessage { content, .. } => {
                let text = format_user_inputs(&content);
                if !text.is_empty() {
                    self.push_user(text);
                }
            }
            ThreadItem::HookPrompt { fragments, .. } => {
                let text = fragments
                    .into_iter()
                    .map(|fragment| fragment.text)
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    self.push_status(format!("hook prompt: {text}"));
                }
            }
            ThreadItem::AgentMessage { text, .. } => {
                if !text.is_empty() {
                    if self.streaming_assistant == text {
                        self.streaming_assistant.clear();
                    }
                    self.push_assistant(text);
                }
            }
            ThreadItem::Plan { text, .. } => {
                if !text.is_empty() {
                    if self.streaming_plan == text {
                        self.streaming_plan.clear();
                    }
                    self.push_plan(text);
                }
            }
            ThreadItem::Reasoning {
                summary, content, ..
            } => {
                let text = summary
                    .into_iter()
                    .chain(content)
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    self.push_status(format!("reasoning: {text}"));
                }
            }
            ThreadItem::CommandExecution {
                id,
                command,
                status,
                aggregated_output,
                exit_code,
                duration_ms,
                ..
            } => {
                let title = command_summary(&command, exit_code, duration_ms);
                self.upsert_tool_activity(id, title.clone(), format!("{status:?}").to_lowercase());
                self.push_tool(title);
                if let Some(output) = aggregated_output.and_then(compact_multiline) {
                    self.push_output(output);
                }
            }
            ThreadItem::FileChange {
                id,
                changes,
                status,
            } => {
                let summary = file_change_summary(&changes);
                self.latest_diff = Some(diff_summary_from_changes(&changes));
                self.upsert_tool_activity(id, summary, format!("{status:?}").to_lowercase());
                self.push_diff(file_change_detail(&changes));
            }
            ThreadItem::McpToolCall {
                id,
                server,
                tool,
                status,
                error,
                duration_ms,
                ..
            } => {
                let mut title = format!("mcp {server}/{tool}");
                if let Some(duration_ms) = duration_ms {
                    title.push_str(&format!(" ({duration_ms}ms)"));
                }
                self.upsert_tool_activity(id, title.clone(), format!("{status:?}").to_lowercase());
                self.push_tool(title);
                if let Some(error) = error {
                    self.push_error(format!("mcp error: {}", error.message));
                }
            }
            ThreadItem::DynamicToolCall {
                id,
                namespace,
                tool,
                status,
                success,
                duration_ms,
                ..
            } => {
                let prefix = namespace
                    .map(|namespace| format!("{namespace}/{tool}"))
                    .unwrap_or(tool);
                let result = success
                    .map(|success| if success { "ok" } else { "failed" })
                    .unwrap_or("pending");
                let mut title = format!("tool {prefix}: {result}");
                if let Some(duration_ms) = duration_ms {
                    title.push_str(&format!(" ({duration_ms}ms)"));
                }
                self.upsert_tool_activity(id, title.clone(), format!("{status:?}").to_lowercase());
                self.push_tool(title);
            }
            ThreadItem::CollabAgentToolCall {
                id,
                tool,
                status,
                receiver_thread_ids,
                ..
            } => {
                let title = format!("agent {tool:?}: {} targets", receiver_thread_ids.len());
                self.upsert_subagent_activity(
                    id,
                    title.clone(),
                    format!("{status:?}").to_lowercase(),
                );
                self.push_tool(title);
            }
            ThreadItem::SubAgentActivity {
                id,
                kind,
                agent_path,
                ..
            } => {
                let title = format!("subagent {kind:?}: {agent_path}");
                self.upsert_subagent_activity(id, title.clone(), "active".to_string());
                self.push_tool(title);
            }
            ThreadItem::WebSearch { id, query, action } => {
                let title = format!("web search: {query}");
                self.upsert_tool_activity(id, title.clone(), format!("{action:?}"));
                self.push_tool(title);
            }
            ThreadItem::ImageView { id, path } => {
                let title = format!("view image: {path}");
                self.upsert_tool_activity(id, title.clone(), "completed".to_string());
                self.push_tool(title);
            }
            ThreadItem::Sleep { id, duration_ms } => {
                let title = format!("sleep {duration_ms}ms");
                self.upsert_tool_activity(id, title.clone(), "completed".to_string());
                self.push_tool(title);
            }
            ThreadItem::ImageGeneration {
                id,
                status,
                saved_path,
                ..
            } => {
                let title = saved_path
                    .map(|path| format!("image generation: {}", path.as_path().display()))
                    .unwrap_or_else(|| "image generation".to_string());
                self.upsert_tool_activity(id, title.clone(), status);
                self.push_tool(title);
            }
            ThreadItem::EnteredReviewMode { review, .. } => {
                self.push_status(format!("entered review mode: {review}"));
            }
            ThreadItem::ExitedReviewMode { review, .. } => {
                self.push_status(format!("exited review mode: {review}"));
            }
            ThreadItem::ContextCompaction { .. } => {
                self.push_status("context compacted");
            }
        }
    }

    fn upsert_tool_activity(&mut self, id: String, title: String, status: String) {
        upsert_activity(&mut self.tool_activity, id, title, status);
    }

    fn upsert_subagent_activity(&mut self, id: String, title: String, status: String) {
        upsert_activity(&mut self.subagent_activity, id, title, status);
    }

    fn record_item_activity(&mut self, item: &ThreadItem, status: String) {
        let id = item.id().to_string();
        let title = events::item_activity_title(item);
        match item {
            ThreadItem::CollabAgentToolCall { .. } | ThreadItem::SubAgentActivity { .. } => {
                self.upsert_subagent_activity(id, title, status);
            }
            _ => self.upsert_tool_activity(id, title, status),
        }
    }

    fn push_system(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::System, text));
    }

    fn push_user(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::User, text));
    }

    fn push_assistant(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::Assistant, text));
    }

    fn push_plan(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::Plan, text));
    }

    fn push_tool(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::Tool, text));
    }

    fn push_diff(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::Diff, text));
    }

    fn push_output(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::Output, text));
    }

    fn push_status(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::Status, text));
    }

    fn push_decision_audit(&mut self, category: &str, decision: &str, title: &str) {
        self.push_line(TranscriptLine::new(
            TranscriptKind::Audit,
            format!("{category} {decision}: {title}"),
        ));
    }

    fn push_error(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::Error, text));
    }

    fn push_line(&mut self, line: TranscriptLine) {
        if self.transcript.back() == Some(&line) {
            return;
        }
        self.transcript.push_back(line);
        while self.transcript.len() > MAX_TRANSCRIPT_LINES {
            self.transcript.pop_front();
            if let Some(selected) = self.transcript_selection {
                self.transcript_selection = Some(selected.saturating_sub(1));
            }
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
            transcript_scroll: 0,
            transcript_scroll_max: Cell::new(0),
            transcript_selection: None,
            command_palette: None,
            codex_home: std::path::PathBuf::from("/tmp/codex-home"),
            dashboard_route: DashboardRoute::Sessions,
            composer: {
                let mut composer = ComposerState::default();
                composer.set_text("Summarize the new shell architecture");
                composer
            },
            clipboard_lease: None,
            active_turn_id: None,
            pending_approval: None,
            pending_elicitation: None,
            pending_user_input: None,
            streaming_assistant: "The new shell owns the fullscreen surface.".to_string(),
            streaming_plan: String::new(),
            plan_explanation: Some("Build the standalone shell in slices.".to_string()),
            plan_steps: vec![
                TurnPlanStep {
                    step: "Shell frame".to_string(),
                    status: codex_app_server_protocol::TurnPlanStepStatus::Completed,
                },
                TurnPlanStep {
                    step: "Transcript model".to_string(),
                    status: codex_app_server_protocol::TurnPlanStepStatus::InProgress,
                },
                TurnPlanStep {
                    step: "Approvals".to_string(),
                    status: codex_app_server_protocol::TurnPlanStepStatus::Pending,
                },
            ],
            active_goal: None,
            tool_activity: VecDeque::from([
                ToolActivity {
                    id: "tool-1".to_string(),
                    title: "exec just test -p codex-tui".to_string(),
                    status: "in progress".to_string(),
                },
                ToolActivity {
                    id: "tool-2".to_string(),
                    title: "file changes in app_shell".to_string(),
                    status: "completed".to_string(),
                },
            ]),
            subagent_activity: VecDeque::new(),
            latest_diff: Some(DiffSummary {
                files: 3,
                additions: 128,
                removals: 24,
            }),
            workspace_git_status: None,
            workspace_status_refresh_due: false,
            rate_limits: Vec::new(),
            rate_limit_reset_credits: None,
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
        shell.push_plan("1. Build shell\n2. Wire transcript\n3. Render dashboard");
        shell.push_tool("exec just test -p codex-tui");
        shell.push_diff("diff 3 files +128 -24");
        shell
    }
}

#[doc(hidden)]
pub mod bench_support {
    use super::render::ShellView;
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const BENCH_AREA: Rect = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 120, /*height*/ 40,
    );

    pub fn render_large_transcript() -> String {
        let mut shell = bench_fixture();
        shell.transcript.clear();
        shell.streaming_assistant.clear();

        for index in 0..2_000 {
            shell.push_user(format!(
                "large transcript user turn {index}: inspect the shell layout and dashboard state"
            ));
            shell.push_assistant(format!(
                "large transcript assistant turn {index}: rendered transcript item with enough text to wrap on a desktop viewport"
            ));
            if index % 10 == 0 {
                shell.push_tool(format!("exec benchmark-step-{index} completed"));
            }
        }

        render_to_string(&shell)
    }

    pub fn render_long_streaming_turn() -> String {
        let mut shell = bench_fixture();
        shell.transcript.clear();
        shell.streaming_assistant = (0..1_000)
            .map(|index| {
                format!(
                    "streaming chunk {index} keeps markdown wrapping and dashboard layout stable"
                )
            })
            .collect::<Vec<_>>()
            .join(" ");

        render_to_string(&shell)
    }

    fn bench_fixture() -> ShellState {
        let mut shell = ShellState {
            thread_id: ThreadId::new(),
            thread_name: Some("bench".to_string()),
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
            transcript_scroll: 0,
            transcript_scroll_max: Cell::new(0),
            transcript_selection: None,
            command_palette: None,
            codex_home: std::path::PathBuf::from("/tmp/codex-home"),
            dashboard_route: DashboardRoute::Sessions,
            composer: {
                let mut composer = ComposerState::default();
                composer.set_text("Benchmark the app shell render path");
                composer
            },
            clipboard_lease: None,
            active_turn_id: Some("turn-bench-1234567890".to_string()),
            pending_approval: None,
            pending_elicitation: None,
            pending_user_input: None,
            streaming_assistant: String::new(),
            streaming_plan: String::new(),
            plan_explanation: Some("Keep render performance bounded.".to_string()),
            plan_steps: vec![
                TurnPlanStep {
                    step: "Large transcript".to_string(),
                    status: codex_app_server_protocol::TurnPlanStepStatus::InProgress,
                },
                TurnPlanStep {
                    step: "Long streaming turn".to_string(),
                    status: codex_app_server_protocol::TurnPlanStepStatus::Pending,
                },
            ],
            active_goal: None,
            tool_activity: VecDeque::from([ToolActivity {
                id: "tool-bench".to_string(),
                title: "render benchmark".to_string(),
                status: "running".to_string(),
            }]),
            subagent_activity: VecDeque::new(),
            latest_diff: Some(DiffSummary {
                files: 4,
                additions: 320,
                removals: 12,
            }),
            workspace_git_status: None,
            workspace_status_refresh_due: false,
            rate_limits: Vec::new(),
            rate_limit_reset_credits: None,
            status: "benchmarking".to_string(),
            token_usage: TokenUsage {
                input_tokens: 120_000,
                cached_input_tokens: 30_000,
                output_tokens: 35_000,
                reasoning_output_tokens: 8_000,
                total_tokens: 155_000,
            },
            model_context_window: Some(200_000),
        };
        shell.push_system("Better Codex app shell benchmark");
        shell
    }

    fn render_to_string(shell: &ShellState) -> String {
        let mut buf = Buffer::empty(BENCH_AREA);
        ShellView { shell }.render(BENCH_AREA, &mut buf);
        buffer_contents(&buf, BENCH_AREA)
    }

    fn buffer_contents(buf: &Buffer, area: Rect) -> String {
        let mut rows = Vec::new();
        for y in area.y..area.bottom() {
            let mut row = String::new();
            for x in area.x..area.right() {
                if let Some(cell) = buf.cell((x, y)) {
                    row.push_str(cell.symbol());
                }
            }
            rows.push(row.trim_end().to_string());
        }
        rows.join("\n")
    }
}

fn upsert_activity(
    activities: &mut VecDeque<ToolActivity>,
    id: String,
    title: String,
    status: String,
) {
    if let Some(existing) = activities.iter_mut().find(|activity| activity.id == id) {
        existing.title = title;
        existing.status = status;
        return;
    }

    activities.push_back(ToolActivity { id, title, status });
    while activities.len() > 8 {
        activities.pop_front();
    }
}

fn format_user_inputs(content: &[UserInput]) -> String {
    content
        .iter()
        .map(|input| match input {
            UserInput::Text { text, .. } => text.clone(),
            UserInput::Image { url, .. } => format!("[image {url}]"),
            UserInput::LocalImage { path, .. } => format!("[image {}]", path.display()),
            UserInput::Skill { name, path } => format!("[skill {name} {}]", path.display()),
            UserInput::Mention { name, path } => format!("[mention {name} {path}]"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn command_summary(command: &str, exit_code: Option<i32>, duration_ms: Option<i64>) -> String {
    let mut summary = format!("exec {command}");
    if let Some(exit_code) = exit_code {
        summary.push_str(&format!(" exit {exit_code}"));
    }
    if let Some(duration_ms) = duration_ms {
        summary.push_str(&format!(" {duration_ms}ms"));
    }
    summary
}

fn file_change_summary(changes: &[FileUpdateChange]) -> String {
    let summary = diff_summary_from_changes(changes);
    format!(
        "diff {} files +{} -{}",
        summary.files, summary.additions, summary.removals
    )
}

fn file_change_detail(changes: &[FileUpdateChange]) -> String {
    let mut lines = vec![file_change_summary(changes)];
    for change in changes.iter().take(8) {
        let line = match &change.kind {
            PatchChangeKind::Add => format!("  A {}", change.path),
            PatchChangeKind::Delete => format!("  D {}", change.path),
            PatchChangeKind::Update { move_path: None } => format!("  M {}", change.path),
            PatchChangeKind::Update {
                move_path: Some(move_path),
            } => format!("  R {} -> {}", change.path, move_path.display()),
        };
        lines.push(line);
    }
    let hidden = changes.len().saturating_sub(8);
    if hidden > 0 {
        lines.push(format!("  ... {hidden} more"));
    }
    lines.join("\n")
}

fn diff_summary_from_changes(changes: &[FileUpdateChange]) -> DiffSummary {
    let mut summary = DiffSummary {
        files: changes.len(),
        ..DiffSummary::default()
    };
    for change in changes {
        let (additions, removals) = count_diff_lines(&change.diff);
        summary.additions += additions;
        summary.removals += removals;
        if matches!(&change.kind, PatchChangeKind::Update { move_path: Some(_) }) {
            summary.files += 1;
        }
    }
    summary
}

fn diff_summary_from_unified_diff(diff: &str) -> DiffSummary {
    let files = diff
        .lines()
        .filter(|line| line.starts_with("diff --git "))
        .count();
    let (additions, removals) = count_diff_lines(diff);
    DiffSummary {
        files,
        additions,
        removals,
    }
}

fn count_diff_lines(diff: &str) -> (usize, usize) {
    let mut additions = 0;
    let mut removals = 0;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            additions += 1;
        } else if line.starts_with('-') {
            removals += 1;
        }
    }
    (additions, removals)
}

fn merge_rate_limit_snapshot(
    mut base: RateLimitSnapshot,
    update: RateLimitSnapshot,
) -> RateLimitSnapshot {
    if update.limit_id.is_some() {
        base.limit_id = update.limit_id;
    }
    if update.limit_name.is_some() {
        base.limit_name = update.limit_name;
    }
    if update.primary.is_some() {
        base.primary = update.primary;
    }
    if update.secondary.is_some() {
        base.secondary = update.secondary;
    }
    if update.credits.is_some() {
        base.credits = update.credits;
    }
    if update.individual_limit.is_some() {
        base.individual_limit = update.individual_limit;
    }
    if update.plan_type.is_some() {
        base.plan_type = update.plan_type;
    }
    if update.rate_limit_reached_type.is_some() {
        base.rate_limit_reached_type = update.rate_limit_reached_type;
    }
    base
}

fn compact_multiline(text: String) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    const MAX_CHARS: usize = 500;
    if text.chars().count() <= MAX_CHARS {
        return Some(text.to_string());
    }
    let mut compact = text.chars().take(MAX_CHARS).collect::<String>();
    compact.push_str("...");
    Some(compact)
}

fn dashboard_route_from_key(key: KeyEvent) -> Option<DashboardRoute> {
    if !key.modifiers.contains(KeyModifiers::CONTROL) {
        return None;
    }

    match key.code {
        KeyCode::Char('1') => Some(DashboardRoute::Sessions),
        KeyCode::Char('2') => Some(DashboardRoute::Workspace),
        KeyCode::Char('3') => Some(DashboardRoute::Settings),
        KeyCode::Char('4') => Some(DashboardRoute::Help),
        _ => None,
    }
}

fn approval_action_from_key(key: KeyEvent) -> Option<ApprovalAction> {
    if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
        return None;
    }
    match key.code {
        KeyCode::Char('a') | KeyCode::Char('A') => {
            Some(ApprovalAction::Choose(ApprovalChoice::Approve))
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            Some(ApprovalAction::Choose(ApprovalChoice::Deny))
        }
        KeyCode::Char('e') | KeyCode::Char('E') => Some(ApprovalAction::Edit),
        KeyCode::Char('?') => Some(ApprovalAction::Explain),
        _ => None,
    }
}

fn elicitation_choice_from_key(key: KeyEvent) -> Option<ElicitationChoice> {
    if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
        return None;
    }
    match key.code {
        KeyCode::Char('a') | KeyCode::Char('A') => Some(ElicitationChoice::Accept),
        KeyCode::Char('d') | KeyCode::Char('D') => Some(ElicitationChoice::Decline),
        KeyCode::Char('c') | KeyCode::Char('C') => Some(ElicitationChoice::Cancel),
        _ => None,
    }
}

#[cfg(test)]
#[path = "app_shell_tests.rs"]
mod tests;
