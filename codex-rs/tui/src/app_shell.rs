//! First-stage standalone TUI shell for the Better Codex fork.
//!
//! This intentionally avoids the inherited chat widget and owns a small app-like
//! fullscreen surface that talks to Codex through the app-server harness.

use crate::app_exit::AppExitInfo;
use crate::app_exit::ExitReason;
use crate::app_server_session::AgentHistoryTask;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::AppServerStartedThread;
use crate::app_server_session::TurnPermissionsOverride;
use crate::clipboard_copy::ClipboardLease;
use crate::goal_display::GOAL_USAGE;
use crate::goal_display::goal_status_label;
use crate::goal_display::goal_usage_summary;
use crate::key_hint;
use crate::legacy_core::config::Config;
use crate::resume_picker::SessionSelection;
use crate::session_state::ThreadSessionState;
use crate::token_usage::TokenUsage;
use crate::tui;
use crate::tui::TuiEvent;
use crate::workspace_command::AppServerWorkspaceCommandRunner;
use crate::workspace_command::WorkspaceCommandRunner;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::ListMcpServerStatusParams;
use codex_app_server_protocol::ListMcpServerStatusResponse;
use codex_app_server_protocol::McpServerStatusDetail;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::PluginListParams;
use codex_app_server_protocol::PluginListResponse;
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnPlanStep;
use codex_app_server_protocol::UserInput;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ModelPreset;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::select;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;

mod agent_activity;
mod agent_activity_controller;
mod agent_activity_render;
mod agent_history;
mod agent_log;
mod agent_log_format;
mod agent_log_view;
mod approval;
mod backend;
mod command_palette;
mod command_palette_view;
mod composer;
mod composer_render;
mod dashboard;
mod dashboard_help;
mod dashboard_rate_limits;
mod dashboard_workspace;
mod design;
mod elicitation;
mod events;
mod external_agent_import;
mod header;
mod input_request_view;
mod integrations;
mod mcp_management;
mod modal_view;
mod navigation;
mod plugin_management;
mod pointer;
mod render;
mod safety_buffering;
mod selector;
mod selector_controller;
mod session_hydration;
mod sessions;
mod settings;
mod shell_command;
mod startup;
mod startup_availability_nux;
mod startup_layout;
mod startup_login;
mod startup_model_migration;
mod transcript_render;
mod transcript_view;
mod user_input;
mod workspace;
use agent_activity::AgentActivityState;
use agent_log::AgentLogState;
use approval::ApprovalAction;
use approval::ApprovalChoice;
use approval::PendingApproval;
use backend::AppShellBackend;
use backend::AppShellTurnStart;
use backend::shutdown_app_shell_backend;
use command_palette::CommandPaletteAction;
use command_palette::CommandPaletteContext;
use command_palette::CommandPaletteEntry;
use command_palette::CommandPaletteState;
use command_palette::command_palette_entries;
use composer::ComposerState;
use elicitation::ElicitationChoice;
use elicitation::PendingElicitation;
use external_agent_import::ExternalAgentImportState;
use integrations::McpInventorySummary;
use integrations::PluginInventorySummary;
use mcp_management::McpManagementState;
use navigation::AppShellRouteState;
use navigation::DashboardRoute;
use plugin_management::PluginManagementState;
use render::draw_shell;
use safety_buffering::SafetyBufferingState;
use selector::SelectorState;
use selector::SelectorValue;
use session_hydration::SessionHydrationState;
use sessions::SessionListState;
use sessions::SessionSearchOutcome;
use settings::SettingsAction;
use settings::SettingsState;
use shell_command::PendingShellCommand;
use shell_command::ShellCommand;
pub(crate) use startup::StartupOnboardingOutcome;
pub(crate) use startup::run_startup_onboarding;
pub(crate) use startup_login::LoginOnboardingOutcome;
pub(crate) use startup_login::run_login_onboarding;
use transcript_render::TranscriptRenderCache;
use user_input::PendingUserInput;
use user_input::UserInputAdvance;
use workspace::WorkspaceGitStatus;

const MAX_TRANSCRIPT_LINES: usize = 400;
const TRANSCRIPT_OUTPUT_HIGH_WATER_CHARS: usize = 8_000;
const TRANSCRIPT_OUTPUT_HIGH_WATER_LINES: usize = 160;
const TRANSCRIPT_OUTPUT_LOW_WATER_CHARS: usize = 6_000;
const TRANSCRIPT_OUTPUT_LOW_WATER_LINES: usize = 120;
const TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX: &str = "... earlier output omitted ...\n";
const TRANSCRIPT_PAGE_SCROLL_STEP: usize = 8;
const TRANSCRIPT_SELECTION_STEP: usize = 1;
const APP_SERVER_FRAME_INTERVAL: Duration = Duration::from_millis(33);
const AGENT_HISTORY_POLL_INTERVAL: Duration = Duration::from_millis(50);

fn next_transcript_render_revision() -> u64 {
    static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);
    NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
}

pub(crate) async fn run(
    tui: &mut tui::Tui,
    mut app_server: AppServerSession,
    mut config: Config,
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
    if startup_model_migration::run_model_migration_onboarding(
        tui,
        &mut app_server,
        &mut config,
        bootstrap.default_model.as_str(),
        &bootstrap.available_models,
    )
    .await?
    .is_exit()
    {
        app_server
            .shutdown()
            .await
            .inspect_err(|err| {
                tracing::warn!("app-server shutdown failed: {err}");
            })
            .ok();
        return Ok(AppExitInfo {
            token_usage: TokenUsage::default(),
            thread_id: None,
            resume_hint: None,
            update_action: None,
            exit_reason: ExitReason::UserRequested,
        });
    }

    let fallback_model = config
        .model
        .clone()
        .unwrap_or_else(|| bootstrap.default_model.clone());
    let availability_nux = startup_availability_nux::prepare(
        app_server.request_handle(),
        &mut config,
        &bootstrap.available_models,
    )
    .await;
    let workspace_command_runner = Arc::new(AppServerWorkspaceCommandRunner::new(
        app_server.request_handle(),
    ));

    let started = start_selected_session(&mut app_server, &config, session_selection).await?;
    let AppServerStartedThread {
        session,
        turns,
        agent_threads,
        agent_history_task,
    } = started;
    let route_state = AppShellRouteState::load(config.codex_home.as_path());
    let mut shell = ShellState::new(
        session,
        fallback_model,
        bootstrap.available_models,
        config.codex_home.to_path_buf(),
        route_state.route,
        config.tui_theme.clone(),
        config.animations,
        config.show_tooltips,
        config.multi_agent_v2.max_concurrent_threads_per_session,
    );
    shell.workspace_command_runner = Some(workspace_command_runner.clone());
    shell.ingest_turn_history(turns);
    shell.install_agent_history(agent_threads, agent_history_task);
    if let Some(message) = availability_nux {
        shell.push_system(message);
    }
    // Paint the restored conversation and start accepting input before secondary dashboard data
    // completes. These lookups can cross a remote app-server boundary, so their results are
    // revision-guarded and applied from the event loop as they become available.
    let has_initial_prompt = initial_prompt
        .as_deref()
        .is_some_and(|prompt| !prompt.trim().is_empty());
    draw_shell(tui, &shell)?;
    shell.start_initial_dashboard_hydration(&app_server);
    if !has_initial_prompt {
        shell.start_initial_goal_hydration(&app_server);
    }

    let run_result: Result<ExitReason> = async {
        if let Some(prompt) = initial_prompt.filter(|prompt| !prompt.trim().is_empty()) {
            shell.submit_prompt(&mut app_server, prompt).await?;
            // Goal reads and turn starts serialize by thread id. Start this lookup only after the
            // initial turn is accepted so a slow goal read cannot delay the user's first prompt.
            shell.start_initial_goal_hydration(&app_server);
            tui.frame_requester().schedule_frame();
        }

        let mut tui_events = tui.event_stream();
        let mut agent_history_poll = tokio::time::interval(AGENT_HISTORY_POLL_INTERVAL);
        agent_history_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let exit_reason = loop {
            select! {
                event = tui_events.next() => {
                    let Some(event) = event else {
                        break ExitReason::UserRequested;
                    };
                    match event {
                        TuiEvent::Key(key) => {
                            if shell.handle_key(key, &config, &mut app_server).await? {
                                break ExitReason::UserRequested;
                            }
                            tui.frame_requester().schedule_frame();
                        }
                        TuiEvent::MouseClick(position) => {
                            let size = tui.terminal.size()?;
                            shell
                                .handle_mouse_click(
                                    ratatui::layout::Rect::new(
                                        /*x*/ 0,
                                        /*y*/ 0,
                                        size.width,
                                        size.height,
                                    ),
                                    position,
                                    &config,
                                    &mut app_server,
                                )
                                .await?;
                            tui.frame_requester().schedule_frame();
                        }
                        TuiEvent::MouseMove(position) => {
                            if shell.set_pointer_position(position) {
                                tui.frame_requester().schedule_frame();
                            }
                        }
                        TuiEvent::MouseScroll {
                            position,
                            direction,
                        } => {
                            let size = tui.terminal.size()?;
                            shell.handle_mouse_scroll(
                                ratatui::layout::Rect::new(
                                    /*x*/ 0,
                                    /*y*/ 0,
                                    size.width,
                                    size.height,
                                ),
                                position,
                                direction,
                            );
                            tui.frame_requester().schedule_frame();
                        }
                        TuiEvent::Paste(text) => {
                            shell.insert_text(&text);
                            tui.frame_requester().schedule_frame();
                        }
                        TuiEvent::Resize => {
                            shell.clear_pointer_position();
                            draw_shell(tui, &shell)?;
                        }
                        TuiEvent::Draw => {
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
                                    event,
                                )
                                .await?;
                            tui.frame_requester()
                                .schedule_frame_in(APP_SERVER_FRAME_INTERVAL);
                        }
                        None => {
                            shell.push_system("app-server disconnected");
                            break ExitReason::Fatal("app-server disconnected".to_string());
                        }
                    }
                }
                _ = agent_history_poll.tick(), if shell.has_pending_agent_history()
                    || shell.has_pending_agent_log()
                    || shell.has_pending_session_hydration()
                    || shell.has_pending_shell_command() =>
                {
                    let mut changed = shell.poll_shell_command().await;
                    changed |= shell.poll_session_hydration(&app_server).await;
                    if shell.has_pending_agent_history() {
                        changed |= shell.poll_agent_history(&app_server).await;
                    }
                    if shell.has_pending_agent_log() {
                        changed |= shell.poll_agent_log().await;
                    }
                    if changed {
                        tui.frame_requester().schedule_frame();
                    }
                }
            }
        };
        Ok(exit_reason)
    }
    .await;

    shell.cancel_shell_command();
    shell.close_agent_log();
    shell.cancel_agent_history().await;
    shell.cancel_session_hydration();
    shell.finish_subscription_cleanup().await;
    app_server
        .unsubscribe_threads(shell.tracked_thread_ids())
        .await;
    shutdown_app_shell_backend(app_server)
        .await
        .inspect_err(|err| {
            tracing::warn!("app-server shutdown failed: {err}");
        })
        .ok();
    let exit_reason = run_result?;

    Ok(AppExitInfo {
        token_usage: shell.token_usage.clone(),
        thread_id: Some(shell.thread_id),
        resume_hint: shell.resume_hint(),
        update_action: None,
        exit_reason,
    })
}

async fn start_selected_session<S>(
    app_server: &mut S,
    config: &Config,
    session_selection: SessionSelection,
) -> Result<crate::app_server_session::AppServerStartedThread>
where
    S: AppShellBackend,
{
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

#[derive(Debug)]
struct TranscriptLine {
    kind: TranscriptKind,
    text: String,
    tool_status: Option<ToolBlockStatus>,
    item_id: Option<String>,
    render_revision: u64,
}

impl TranscriptLine {
    fn new(kind: TranscriptKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            tool_status: None,
            item_id: None,
            render_revision: next_transcript_render_revision(),
        }
    }

    fn tool_status(mut self, status: ToolBlockStatus) -> Self {
        self.tool_status = Some(status);
        self.mark_render_changed();
        self
    }

    fn item_id(mut self, item_id: impl Into<String>) -> Self {
        self.item_id = Some(item_id.into());
        self
    }

    fn mark_render_changed(&mut self) {
        self.render_revision = next_transcript_render_revision();
    }
}

impl Clone for TranscriptLine {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            text: self.text.clone(),
            tool_status: self.tool_status,
            item_id: self.item_id.clone(),
            render_revision: next_transcript_render_revision(),
        }
    }
}

impl PartialEq for TranscriptLine {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.text == other.text
            && self.tool_status == other.tool_status
            && self.item_id == other.item_id
    }
}

impl Eq for TranscriptLine {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptKind {
    System,
    User,
    Assistant,
    Plan,
    Tool,
    Diff,
    Output,
    Separator,
    Status,
    Audit,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolBlockStatus {
    Running,
    Success,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalSlashCommand {
    Clear,
    Exit,
    Goal(GoalSlashCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalSlashCommandOutcome {
    Continue,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GoalSlashCommand {
    Show,
    Set(String),
    Clear,
    Pause,
    Resume,
    Edit,
}

impl LocalSlashCommand {
    fn parse(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let command = parts.next()?;
        let args = parts.next().unwrap_or("").trim();
        match command {
            "/clear" if args.is_empty() => Some(Self::Clear),
            "/exit" if args.is_empty() => Some(Self::Exit),
            "/goal" => Some(Self::Goal(GoalSlashCommand::parse(args))),
            _ => None,
        }
    }
}

impl GoalSlashCommand {
    fn parse(args: &str) -> Self {
        match args {
            "" => Self::Show,
            "clear" => Self::Clear,
            "pause" => Self::Pause,
            "resume" => Self::Resume,
            "edit" => Self::Edit,
            objective => Self::Set(objective.to_string()),
        }
    }
}

impl TranscriptKind {
    fn label(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "you",
            Self::Assistant => "codex",
            Self::Plan => "plan",
            Self::Tool => "tool",
            Self::Diff => "edited",
            Self::Output => "output",
            Self::Separator => "",
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
    available_models: Vec<ModelPreset>,
    cwd: String,
    approval_policy: codex_app_server_protocol::AskForApproval,
    approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer,
    permission_profile: codex_protocol::models::PermissionProfile,
    runtime_workspace_roots: Vec<codex_utils_absolute_path::AbsolutePathBuf>,
    reasoning_effort: Option<codex_protocol::openai_models::ReasoningEffort>,
    service_tier: Option<String>,
    collaboration_mode: Option<Box<codex_protocol::config_types::CollaborationMode>>,
    max_concurrent_threads_per_session: usize,
    personality: Option<codex_protocol::config_types::Personality>,
    transcript: VecDeque<TranscriptLine>,
    transcript_scroll: usize,
    transcript_scroll_max: Cell<usize>,
    transcript_selection: Option<usize>,
    transcript_render_cache: RefCell<TranscriptRenderCache>,
    session_list: SessionListState,
    settings: SettingsState,
    mcp_inventory: McpInventorySummary,
    mcp_catalog: Option<ListMcpServerStatusResponse>,
    plugin_inventory: PluginInventorySummary,
    plugin_catalog: Option<PluginListResponse>,
    tui_theme: Option<String>,
    animations: bool,
    show_tooltips: bool,
    command_palette: Option<CommandPaletteState>,
    selector: Option<SelectorState<SelectorValue>>,
    codex_home: std::path::PathBuf,
    dashboard_route: DashboardRoute,
    dashboard_visible: bool,
    pointer_position: Option<ratatui::layout::Position>,
    agents_focused: bool,
    composer: ComposerState,
    workspace_command_runner: Option<WorkspaceCommandRunner>,
    pending_shell_command: Option<PendingShellCommand>,
    session_hydration: SessionHydrationState,
    exit_confirmation_pending: bool,
    clipboard_lease: Option<ClipboardLease>,
    active_turn_id: Option<String>,
    pending_approval: Option<PendingApproval>,
    pending_elicitation: Option<PendingElicitation>,
    pending_external_agent_import: Option<ExternalAgentImportState>,
    pending_mcp_management: Option<McpManagementState>,
    pending_plugin_management: Option<PluginManagementState>,
    pending_user_input: Option<PendingUserInput>,
    safety_buffering: SafetyBufferingState,
    streaming_assistant: String,
    streaming_assistant_revision: u64,
    streaming_plan: String,
    streaming_plan_revision: u64,
    plan_explanation: Option<String>,
    plan_steps: Vec<TurnPlanStep>,
    active_goal: Option<ThreadGoal>,
    tool_activity: VecDeque<ToolActivity>,
    agent_activity: AgentActivityState,
    agent_log: Option<AgentLogState>,
    agent_history_task: Option<AgentHistoryTask>,
    active_agent_thread_ids: HashSet<String>,
    deferred_unsubscribe_thread_ids: Vec<ThreadId>,
    subscription_cleanup_task: Option<JoinHandle<()>>,
    subagent_activity: VecDeque<ToolActivity>,
    latest_diff: Option<DiffSummary>,
    workspace_git_status: Option<WorkspaceGitStatus>,
    workspace_status_refresh_due: bool,
    rate_limits: Vec<RateLimitSnapshot>,
    rate_limit_reset_credits: Option<i64>,
    status: String,
    token_usage: TokenUsage,
    context_token_usage: TokenUsage,
    model_context_window: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletedItemOrigin {
    Historical,
    Live,
}

impl ShellState {
    fn new(
        session: ThreadSessionState,
        fallback_model: String,
        available_models: Vec<ModelPreset>,
        codex_home: std::path::PathBuf,
        dashboard_route: DashboardRoute,
        tui_theme: Option<String>,
        animations: bool,
        show_tooltips: bool,
        max_concurrent_threads_per_session: usize,
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
            available_models,
            cwd: session.cwd.to_string_lossy().to_string(),
            approval_policy: session.approval_policy,
            approvals_reviewer: session.approvals_reviewer,
            permission_profile: session.permission_profile,
            runtime_workspace_roots: session.runtime_workspace_roots,
            reasoning_effort: session.reasoning_effort,
            service_tier: session.service_tier,
            collaboration_mode: session.collaboration_mode,
            max_concurrent_threads_per_session,
            personality: session.personality,
            transcript: VecDeque::new(),
            transcript_scroll: 0,
            transcript_scroll_max: Cell::new(0),
            transcript_selection: None,
            transcript_render_cache: RefCell::new(TranscriptRenderCache::default()),
            session_list: SessionListState::default(),
            settings: SettingsState::default(),
            mcp_inventory: McpInventorySummary::default(),
            mcp_catalog: None,
            plugin_inventory: PluginInventorySummary::default(),
            plugin_catalog: None,
            tui_theme,
            animations,
            show_tooltips,
            command_palette: None,
            selector: None,
            codex_home,
            dashboard_route,
            dashboard_visible: true,
            pointer_position: None,
            agents_focused: false,
            composer: ComposerState::default(),
            workspace_command_runner: None,
            pending_shell_command: None,
            session_hydration: SessionHydrationState::default(),
            exit_confirmation_pending: false,
            clipboard_lease: None,
            active_turn_id: None,
            pending_approval: None,
            pending_elicitation: None,
            pending_external_agent_import: None,
            pending_mcp_management: None,
            pending_plugin_management: None,
            pending_user_input: None,
            safety_buffering: SafetyBufferingState::default(),
            streaming_assistant: String::new(),
            streaming_assistant_revision: next_transcript_render_revision(),
            streaming_plan: String::new(),
            streaming_plan_revision: next_transcript_render_revision(),
            plan_explanation: None,
            plan_steps: Vec::new(),
            active_goal: None,
            tool_activity: VecDeque::new(),
            agent_activity: AgentActivityState::default(),
            agent_log: None,
            agent_history_task: None,
            active_agent_thread_ids: HashSet::new(),
            deferred_unsubscribe_thread_ids: Vec::new(),
            subscription_cleanup_task: None,
            subagent_activity: VecDeque::new(),
            latest_diff: None,
            workspace_git_status: None,
            workspace_status_refresh_due: false,
            rate_limits: Vec::new(),
            rate_limit_reset_credits: None,
            status: "ready".to_string(),
            token_usage: TokenUsage::default(),
            context_token_usage: TokenUsage::default(),
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
                self.ingest_completed_item(item, CompletedItemOrigin::Historical);
            }
            if let Some(error) = turn.error {
                self.push_error(error.message);
            }
            self.push_turn_separator();
        }
    }

    async fn handle_key<S>(
        &mut self,
        key: KeyEvent,
        config: &Config,
        app_server: &mut S,
    ) -> Result<bool>
    where
        S: AppShellBackend,
    {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(false);
        }
        let is_plain_text_repeat = if key.kind == KeyEventKind::Repeat
            && let KeyCode::Char(ch) = key.code
            && !ch.is_control()
            && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            if self.selector.is_some()
                || self.command_palette.is_some()
                || self.agent_log.is_some()
                || self.safety_buffering_modal_lines().is_some()
                || self.pending_approval.is_some()
                || self.pending_elicitation.is_some()
                || self.pending_external_agent_import.is_some()
            {
                false
            } else if let Some(mcp_management) = &self.pending_mcp_management {
                mcp_management.editing()
            } else if self.pending_plugin_management.is_some() {
                false
            } else if self.pending_user_input.is_some() {
                true
            } else if self.dashboard_route == DashboardRoute::Sessions && self.session_list.focused
            {
                self.session_list.search_active() || self.session_list.renaming()
            } else if self.dashboard_route == DashboardRoute::Settings && self.settings.focused {
                self.settings.editing()
            } else {
                !self.dashboard_focused() && self.transcript_selection.is_none()
            }
        } else {
            false
        };
        if key.kind == KeyEventKind::Repeat
            && !is_plain_text_repeat
            && !matches!(
                key.code,
                KeyCode::Backspace
                    | KeyCode::Char('\u{007f}')
                    | KeyCode::Delete
                    | KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::PageUp
                    | KeyCode::PageDown
            )
        {
            return Ok(false);
        }
        let is_ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c'));
        if is_ctrl_c {
            if self.active_turn_id.is_some() {
                self.exit_confirmation_pending = false;
                self.interrupt_active_turn(app_server).await?;
                return Ok(false);
            }
            if self.has_pending_shell_command() {
                self.cancel_shell_command();
                self.exit_confirmation_pending = false;
                return Ok(false);
            }
            return Ok(self.confirm_exit());
        }
        if !matches!(key.code, KeyCode::Esc) {
            self.exit_confirmation_pending = false;
        }
        if self.agent_log.is_some() {
            if key.kind == KeyEventKind::Press
                && key.modifiers == KeyModifiers::NONE
                && matches!(key.code, KeyCode::Char('r'))
            {
                self.reload_agent_log(config, app_server);
            } else {
                self.handle_agent_log_key(key);
            }
            return Ok(false);
        }
        if self.selector.is_some() {
            self.handle_selector_key(key, app_server).await?;
            return Ok(false);
        }
        if self.command_palette.is_some() {
            self.handle_command_palette_key(key, app_server).await?;
            return Ok(false);
        }
        if self.safety_buffering_modal_lines().is_some() {
            self.handle_safety_buffering_key(key, app_server).await;
            return Ok(false);
        }
        if self.pending_approval.is_some() {
            if let Some(action) = approval_action_from_key(key) {
                self.handle_pending_approval_action(app_server, action)
                    .await?;
            }
            return Ok(false);
        }
        if self.pending_elicitation.is_some() {
            if let Some(choice) = elicitation_choice_from_key(key) {
                self.resolve_pending_elicitation(app_server, choice).await?;
            }
            return Ok(false);
        }
        if self.pending_external_agent_import.is_some() {
            self.handle_external_agent_import_key(key, app_server)
                .await?;
            return Ok(false);
        }
        if self.pending_mcp_management.is_some() {
            self.handle_mcp_management_key(key, app_server).await?;
            return Ok(false);
        }
        if self.pending_plugin_management.is_some() {
            self.handle_plugin_management_key(key, app_server).await?;
            return Ok(false);
        }
        if self.pending_user_input.is_some() {
            return self.handle_user_input_key(key, app_server).await;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('d')) {
            self.dashboard_visible = !self.dashboard_visible;
            if !self.dashboard_visible {
                self.session_list.focused = false;
                self.settings.focused = false;
                self.agents_focused = false;
            }
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('o')) {
            self.copy_selected_transcript_with(crate::clipboard_copy::copy_to_clipboard);
            return Ok(false);
        }
        if let Some(route) = dashboard_route_from_key(key) {
            let route_already_visible = self.dashboard_visible && self.dashboard_route == route;
            self.dashboard_visible = true;
            self.set_dashboard_route(route);
            self.session_list.focused = route_already_visible && route == DashboardRoute::Sessions;
            self.settings.focused = route_already_visible && route == DashboardRoute::Settings;
            self.agents_focused = route_already_visible && route == DashboardRoute::Agents;
            if route == DashboardRoute::Sessions {
                self.start_session_list_refresh(app_server);
            }
            return Ok(false);
        }
        if self.composer.is_empty()
            && let Some(step) =
                dashboard_route_step_from_key(key, /*allow_word_motion_fallback*/ true)
        {
            let route = match step {
                DashboardRouteStep::Previous => self.dashboard_route.previous(),
                DashboardRouteStep::Next => self.dashboard_route.next(),
            };
            self.set_dashboard_route(route);
            self.session_list.focused = false;
            self.settings.focused = false;
            self.agents_focused = false;
            return Ok(false);
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
        if self.dashboard_route == DashboardRoute::Sessions
            && self.session_list.focused
            && self
                .handle_session_list_key(key, config, app_server)
                .await?
        {
            return Ok(false);
        }
        if self.dashboard_route == DashboardRoute::Settings
            && self.settings.focused
            && self.handle_settings_key(key, app_server).await?
        {
            return Ok(false);
        }
        if self.dashboard_visible
            && self.dashboard_route == DashboardRoute::Agents
            && self.agents_focused
            && matches!(key.code, KeyCode::Enter)
            && matches!(key.modifiers, KeyModifiers::NONE)
        {
            self.open_selected_agent_log(config, app_server);
            return Ok(false);
        }
        if self.handle_agent_activity_key(key) {
            return Ok(false);
        }
        if self.dashboard_focused()
            && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            return Ok(false);
        }
        if let Some(action) = composer_backspace_action_from_key(key) {
            self.apply_composer_backspace_action(action);
            return Ok(false);
        }
        if let Some(word_motion) = composer_word_motion_from_key(key) {
            match word_motion {
                ComposerWordMotion::Left => self.composer.move_word_left(),
                ComposerWordMotion::Right => self.composer.move_word_right(),
            }
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('p')) {
            self.open_command_palette();
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Char('m')) {
            self.open_model_selector();
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Char('e')) {
            self.open_reasoning_selector();
            return Ok(false);
        }
        match key.code {
            KeyCode::Esc => Ok(self.confirm_exit()),
            KeyCode::Enter => {
                if is_composer_newline_key(key) {
                    self.composer.insert_newline();
                } else {
                    let prompt = self.composer.submission_text();
                    if prompt.is_empty() && self.dashboard_visible {
                        match self.dashboard_route {
                            DashboardRoute::Sessions => self.session_list.focused = true,
                            DashboardRoute::Agents => self.agents_focused = true,
                            DashboardRoute::Settings => self.settings.focused = true,
                            DashboardRoute::Workspace | DashboardRoute::Help => {}
                        }
                        if self.dashboard_focused() {
                            return Ok(false);
                        }
                    }
                    if !prompt.is_empty() {
                        if let Some(command) = LocalSlashCommand::parse(&prompt) {
                            let outcome = self
                                .run_local_slash_command(command, prompt, app_server)
                                .await?;
                            return Ok(outcome == LocalSlashCommandOutcome::Exit);
                        } else if let Some(command) = ShellCommand::parse(&prompt) {
                            self.start_shell_command(command, prompt);
                        } else if self.active_turn_id.is_some() {
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
        let status = workspace::load_git_status(runner, std::path::Path::new(&self.cwd)).await;
        self.record_workspace_git_status(status);
    }

    async fn refresh_mcp_inventory<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        let mut cursor = None;
        let mut response = codex_app_server_protocol::ListMcpServerStatusResponse {
            data: Vec::new(),
            next_cursor: None,
        };
        loop {
            match app_server
                .mcp_server_status_list(ListMcpServerStatusParams {
                    cursor: cursor.clone(),
                    limit: Some(100),
                    detail: Some(McpServerStatusDetail::ToolsAndAuthOnly),
                    thread_id: Some(self.thread_id.to_string()),
                })
                .await
            {
                Ok(mut page) => {
                    cursor = page.next_cursor.take();
                    response.data.extend(page.data);
                    if cursor.is_none() {
                        self.mcp_inventory = McpInventorySummary::from_response(&response);
                        self.mcp_catalog = Some(response);
                        self.settings.set_info("mcp inventory refreshed");
                        return;
                    }
                }
                Err(err) => {
                    self.mcp_inventory = McpInventorySummary::from_error(err.to_string());
                    self.mcp_catalog = None;
                    self.settings.set_error("failed to refresh mcp inventory");
                    return;
                }
            }
        }
    }

    async fn refresh_plugin_inventory<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        let cwd = match codex_utils_absolute_path::AbsolutePathBuf::try_from(
            std::path::PathBuf::from(&self.cwd),
        ) {
            Ok(cwd) => cwd,
            Err(err) => {
                self.plugin_inventory = PluginInventorySummary::from_error(err.to_string());
                self.plugin_catalog = None;
                self.settings.set_error("failed to refresh plugins");
                return;
            }
        };
        match app_server
            .plugin_list(PluginListParams {
                cwds: Some(vec![cwd]),
                marketplace_kinds: None,
            })
            .await
        {
            Ok(response) => {
                self.plugin_inventory = PluginInventorySummary::from_response(&response);
                self.plugin_catalog = Some(response);
                self.settings.set_info("plugin inventory refreshed");
            }
            Err(err) => {
                self.plugin_inventory = PluginInventorySummary::from_error(err.to_string());
                self.plugin_catalog = None;
                self.settings.set_error("failed to refresh plugins");
            }
        }
    }

    fn apply_rate_limit_update(&mut self, snapshot: RateLimitSnapshot) {
        self.mark_rate_limits_updated();
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
        if self.selector.is_some()
            || self.command_palette.is_some()
            || self.agent_log.is_some()
            || self.safety_buffering_modal_lines().is_some()
            || self.pending_approval.is_some()
            || self.pending_elicitation.is_some()
            || self.pending_external_agent_import.is_some()
            || self.pending_mcp_management.is_some()
            || self.pending_plugin_management.is_some()
            || self.dashboard_focused()
        {
            return;
        }
        self.clear_transcript_selection();
        self.composer.insert_str(text);
    }

    async fn handle_command_palette_key<S>(
        &mut self,
        key: KeyEvent,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        if key_hint::ctrl(KeyCode::Char('p')).is_press(key) {
            self.close_command_palette();
            return Ok(());
        }
        if !is_unmodified_action_key(key) {
            return Ok(());
        }
        match key.code {
            KeyCode::Esc => {
                self.close_command_palette();
            }
            KeyCode::Enter => {
                self.execute_selected_command_palette_action(app_server)
                    .await?;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let entries = self.command_palette_entries();
                if let Some(palette) = &mut self.command_palette {
                    palette.move_up(&entries);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
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

    async fn handle_session_list_key<S>(
        &mut self,
        key: KeyEvent,
        config: &Config,
        app_server: &mut S,
    ) -> Result<bool>
    where
        S: AppShellBackend,
    {
        if self.session_list.renaming() {
            return self
                .handle_session_rename_key(key, app_server)
                .await
                .map(|()| true);
        }
        if self.session_list.search_active() {
            if self.handle_session_search_key(key) == SessionSearchOutcome::RefreshList {
                self.start_session_list_refresh(app_server);
            }
            return Ok(true);
        }
        if !is_unmodified_action_key(key) {
            return Ok(matches!(
                key.code,
                KeyCode::Esc
                    | KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Enter
                    | KeyCode::Char('k' | 'j' | '/' | 'v' | 'r' | 'f' | 'a' | 'u' | 'd' | 'n')
                    | KeyCode::PageUp
                    | KeyCode::PageDown
            ));
        }
        match key.code {
            KeyCode::Esc => {
                self.session_list.focused = false;
                Ok(true)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.session_list.move_selection_up();
                Ok(true)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.session_list.move_selection_down();
                Ok(true)
            }
            KeyCode::Enter => {
                self.resume_selected_session(config, app_server).await?;
                Ok(true)
            }
            KeyCode::Char('/') => {
                self.session_list.start_search();
                Ok(true)
            }
            KeyCode::Char('v') => {
                self.session_list.toggle_archived();
                self.start_session_list_refresh(app_server);
                Ok(true)
            }
            KeyCode::Char('r') => {
                self.resume_selected_session(config, app_server).await?;
                Ok(true)
            }
            KeyCode::Char('f') => {
                self.fork_selected_session(config, app_server).await?;
                Ok(true)
            }
            KeyCode::Char('a') if !self.session_list.show_archived() => {
                self.archive_selected_session(app_server).await?;
                Ok(true)
            }
            KeyCode::Char('u') if self.session_list.show_archived() => {
                self.unarchive_selected_session(app_server).await?;
                Ok(true)
            }
            KeyCode::Char('d') => {
                self.delete_selected_session(app_server).await?;
                Ok(true)
            }
            KeyCode::Char('n') if !self.session_list.show_archived() => {
                self.session_list.start_rename();
                Ok(true)
            }
            KeyCode::PageUp => {
                for _ in 0..5 {
                    self.session_list.move_selection_up();
                }
                Ok(true)
            }
            KeyCode::PageDown => {
                for _ in 0..5 {
                    self.session_list.move_selection_down();
                }
                Ok(true)
            }
            KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
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
            | KeyCode::BackTab => Ok(false),
        }
    }

    fn handle_session_search_key(&mut self, key: KeyEvent) -> SessionSearchOutcome {
        if (matches!(key.code, KeyCode::Esc | KeyCode::Enter) && !is_unmodified_key_press(key))
            || (key.code == KeyCode::Backspace && !is_unmodified_key_event(key))
            || (matches!(key.code, KeyCode::Up | KeyCode::Down) && !is_unmodified_action_key(key))
        {
            return SessionSearchOutcome::LocalFilterOnly;
        }
        match key.code {
            KeyCode::Esc => {
                self.session_list.clear_search();
                SessionSearchOutcome::RefreshList
            }
            KeyCode::Enter => {
                self.session_list.stop_search();
                SessionSearchOutcome::RefreshList
            }
            KeyCode::Backspace => self.session_list.backspace_search(),
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.session_list.push_search_char(ch);
                SessionSearchOutcome::LocalFilterOnly
            }
            KeyCode::Char(_) => SessionSearchOutcome::LocalFilterOnly,
            KeyCode::Up => {
                self.session_list.move_selection_up();
                SessionSearchOutcome::LocalFilterOnly
            }
            KeyCode::Down => {
                self.session_list.move_selection_down();
                SessionSearchOutcome::LocalFilterOnly
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
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
            | KeyCode::PageDown => SessionSearchOutcome::LocalFilterOnly,
        }
    }

    async fn handle_session_rename_key<S>(
        &mut self,
        key: KeyEvent,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        if (matches!(key.code, KeyCode::Esc | KeyCode::Enter) && !is_unmodified_key_press(key))
            || (key.code == KeyCode::Backspace && !is_unmodified_key_event(key))
        {
            return Ok(());
        }
        match key.code {
            KeyCode::Esc => {
                self.session_list.cancel_rename();
            }
            KeyCode::Enter => {
                let Some(thread_id) = self.session_list.selected_thread_id() else {
                    self.session_list.cancel_rename();
                    return Ok(());
                };
                let Some(name) = self.session_list.take_rename_draft() else {
                    return Ok(());
                };
                if name.is_empty() {
                    self.push_error("session name cannot be empty");
                    return Ok(());
                }
                app_server.thread_set_name(thread_id, name.clone()).await?;
                self.invalidate_session_list_refresh();
                self.session_list.rename_selected(name.clone());
                if thread_id == self.thread_id {
                    self.thread_name = Some(name.clone());
                }
                self.push_status(format!("renamed session {name}"));
            }
            KeyCode::Backspace => {
                self.session_list.backspace_rename();
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.session_list.push_rename_char(ch);
            }
            KeyCode::Char(_) => {}
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
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
            | KeyCode::PageDown
            | KeyCode::Up
            | KeyCode::Down => {}
        }
        Ok(())
    }

    async fn resume_selected_session<S>(
        &mut self,
        config: &Config,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        if self.block_session_switch_if_busy() {
            return Ok(());
        }
        let Some(thread_id) = self.session_list.selected_thread_id() else {
            self.push_status("no session selected");
            return Ok(());
        };
        if thread_id == self.thread_id {
            self.push_status("session is already open");
            return Ok(());
        }
        self.finish_subscription_cleanup().await;
        let started = app_server.resume_thread(config.clone(), thread_id).await?;
        self.cancel_agent_history().await;
        let previous_thread_ids = self.tracked_thread_ids();
        self.replace_started_session(started);
        self.prepare_replaced_session_cleanup(app_server, previous_thread_ids);
        self.start_replaced_session_hydration(app_server);
        self.start_session_list_refresh(app_server);
        Ok(())
    }

    async fn fork_selected_session<S>(&mut self, config: &Config, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        if self.block_session_switch_if_busy() {
            return Ok(());
        }
        let Some(thread_id) = self.session_list.selected_thread_id() else {
            self.push_status("no session selected");
            return Ok(());
        };
        self.finish_subscription_cleanup().await;
        let started = app_server.fork_thread(config.clone(), thread_id).await?;
        self.cancel_agent_history().await;
        let previous_thread_ids = self.tracked_thread_ids();
        self.replace_started_session(started);
        self.prepare_replaced_session_cleanup(app_server, previous_thread_ids);
        self.start_replaced_session_hydration(app_server);
        self.start_session_list_refresh(app_server);
        Ok(())
    }

    fn block_session_switch_if_busy(&mut self) -> bool {
        let message = if self.active_turn_id.is_some() {
            "finish or interrupt the active turn before switching sessions"
        } else if self.pending_shell_command.is_some() {
            "finish or cancel the shell command before switching sessions"
        } else if self.pending_approval.is_some()
            || self.pending_elicitation.is_some()
            || self.pending_user_input.is_some()
        {
            "resolve the pending request before switching sessions"
        } else if !self.composer.is_empty() {
            "send or clear the message draft before switching sessions"
        } else {
            return false;
        };
        self.push_status(message);
        true
    }

    async fn archive_selected_session<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(thread_id) = self.session_list.selected_thread_id() else {
            self.push_status("no session selected");
            return Ok(());
        };
        if self.session_list.selected_is_current(self.thread_id) {
            self.push_error("cannot archive the active session");
            return Ok(());
        }
        app_server.thread_archive(thread_id).await?;
        self.invalidate_session_list_refresh();
        let title = self
            .session_list
            .remove_selected()
            .map(|row| row.thread_id.to_string())
            .unwrap_or_else(|| thread_id.to_string());
        self.push_status(format!("archived session {title}"));
        Ok(())
    }

    async fn unarchive_selected_session<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(thread_id) = self.session_list.selected_thread_id() else {
            self.push_status("no session selected");
            return Ok(());
        };
        app_server.thread_unarchive(thread_id).await?;
        self.invalidate_session_list_refresh();
        self.session_list.remove_selected();
        self.push_status(format!("unarchived session {thread_id}"));
        Ok(())
    }

    async fn delete_selected_session<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(thread_id) = self.session_list.selected_thread_id() else {
            self.push_status("no session selected");
            return Ok(());
        };
        if self.session_list.selected_is_current(self.thread_id) {
            self.push_error("cannot delete the active session");
            return Ok(());
        }
        app_server.thread_delete(thread_id).await?;
        self.invalidate_session_list_refresh();
        self.session_list.remove_selected();
        self.push_status(format!("deleted session {thread_id}"));
        Ok(())
    }

    fn replace_started_session(&mut self, started: AppServerStartedThread) {
        self.invalidate_session_hydration();
        self.close_agent_log();
        let AppServerStartedThread {
            session,
            turns,
            agent_threads,
            agent_history_task,
        } = started;
        self.thread_id = session.thread_id;
        self.thread_name = session.thread_name;
        if !session.model.is_empty() {
            self.model = session.model;
        }
        self.cwd = session.cwd.to_string_lossy().to_string();
        self.approval_policy = session.approval_policy;
        self.approvals_reviewer = session.approvals_reviewer;
        self.permission_profile = session.permission_profile;
        self.runtime_workspace_roots = session.runtime_workspace_roots;
        self.reasoning_effort = session.reasoning_effort;
        self.service_tier = session.service_tier;
        self.collaboration_mode = session.collaboration_mode;
        self.personality = session.personality;
        self.transcript.clear();
        self.transcript_scroll = 0;
        self.transcript_scroll_max.set(0);
        self.transcript_selection = None;
        self.transcript_render_cache.get_mut().clear();
        self.clear_streaming_transcript();
        self.plan_explanation = None;
        self.plan_steps.clear();
        self.record_active_goal(None);
        self.composer.clear();
        self.pending_shell_command = None;
        self.command_palette = None;
        self.exit_confirmation_pending = false;
        self.pending_external_agent_import = None;
        self.pending_mcp_management = None;
        self.pending_plugin_management = None;
        self.mcp_inventory = McpInventorySummary::default();
        self.mcp_catalog = None;
        self.plugin_inventory = PluginInventorySummary::default();
        self.plugin_catalog = None;
        self.tool_activity.clear();
        self.agent_activity = AgentActivityState::default();
        self.active_agent_thread_ids.clear();
        self.deferred_unsubscribe_thread_ids.clear();
        self.subagent_activity.clear();
        self.latest_diff = None;
        self.record_workspace_git_status(None);
        self.token_usage = TokenUsage::default();
        self.context_token_usage = TokenUsage::default();
        self.model_context_window = None;
        self.active_turn_id = None;
        self.pending_approval = None;
        self.pending_elicitation = None;
        self.pending_user_input = None;
        self.selector = None;
        self.safety_buffering.clear();
        self.status = "ready".to_string();
        self.push_system("switched session");
        self.ingest_turn_history(turns);
        self.install_agent_history(agent_threads, agent_history_task);
    }

    async fn execute_selected_command_palette_action<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
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
            CommandPaletteAction::SwitchModel => {
                self.set_dashboard_route(DashboardRoute::Settings);
                self.session_list.focused = false;
                self.settings.focused = true;
                self.settings.focus_action(SettingsAction::Model);
                self.open_model_selector();
            }
            CommandPaletteAction::ChangePermissions => {
                self.set_dashboard_route(DashboardRoute::Settings);
                self.session_list.focused = false;
                self.settings.focused = true;
                self.settings.focus_action(SettingsAction::ApprovalPolicy);
                self.open_approval_selector();
            }
            CommandPaletteAction::ResumeThread => {
                self.set_dashboard_route(DashboardRoute::Sessions);
                self.settings.focused = false;
                self.session_list.focused = true;
                self.start_session_list_refresh(app_server);
                self.push_status("press r to resume selected session");
            }
            CommandPaletteAction::ForkThread => {
                self.set_dashboard_route(DashboardRoute::Sessions);
                self.settings.focused = false;
                self.session_list.focused = true;
                self.start_session_list_refresh(app_server);
                self.push_status("press f to fork selected session");
            }
            CommandPaletteAction::ImportExternalAgentConfig => {
                self.start_external_agent_import_review(app_server).await?;
            }
            CommandPaletteAction::CompactContext => {}
        }
        Ok(())
    }

    fn open_command_palette(&mut self) {
        self.close_agent_log();
        self.selector = None;
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

        self.session_list.focused = false;
        self.settings.focused = false;
        self.agents_focused = false;
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
        self.transcript_scroll = self
            .transcript_scroll
            .min(self.transcript_scroll_max.get())
            .saturating_sub(rows);
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
        self.transcript_render_cache.get_mut().clear();
        self.clear_streaming_transcript();
        self.transcript_scroll = 0;
        self.transcript_selection = None;
        self.push_system("visible transcript cleared");
    }

    fn confirm_exit(&mut self) -> bool {
        if self.exit_confirmation_pending {
            return true;
        }

        self.exit_confirmation_pending = true;
        self.push_status("press Esc or Ctrl+C again to exit");
        false
    }

    async fn run_local_slash_command<S>(
        &mut self,
        command: LocalSlashCommand,
        prompt: String,
        app_server: &mut S,
    ) -> Result<LocalSlashCommandOutcome>
    where
        S: AppShellBackend,
    {
        self.composer.remember_submission(&prompt);
        self.composer.clear();
        let outcome = match command {
            LocalSlashCommand::Clear => {
                self.clear_visible_transcript();
                LocalSlashCommandOutcome::Continue
            }
            LocalSlashCommand::Exit => LocalSlashCommandOutcome::Exit,
            LocalSlashCommand::Goal(command) => {
                self.run_goal_slash_command(command, app_server).await;
                LocalSlashCommandOutcome::Continue
            }
        };
        Ok(outcome)
    }

    async fn run_goal_slash_command<S>(&mut self, command: GoalSlashCommand, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        match command {
            GoalSlashCommand::Show => self.show_goal_status(app_server).await,
            GoalSlashCommand::Set(objective) => {
                self.set_goal_objective(app_server, objective).await
            }
            GoalSlashCommand::Clear => self.clear_goal(app_server).await,
            GoalSlashCommand::Pause => {
                self.update_goal_status(app_server, ThreadGoalStatus::Paused, "paused")
                    .await;
            }
            GoalSlashCommand::Resume => {
                self.update_goal_status(app_server, ThreadGoalStatus::Active, "resumed")
                    .await;
            }
            GoalSlashCommand::Edit => {
                self.push_status("use /goal <objective> to edit the current goal objective");
            }
        }
    }

    async fn show_goal_status<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        match app_server.thread_goal_get(self.thread_id).await {
            Ok(response) => {
                self.record_active_goal(response.goal);
                match &self.active_goal {
                    Some(goal) => self.push_status(format!(
                        "goal {}. {}",
                        goal_status_label(goal.status),
                        goal_usage_summary(goal)
                    )),
                    None => self.push_status(format!("no goal is currently set. {GOAL_USAGE}")),
                }
            }
            Err(err) => self.push_error(format!("failed to read goal: {err}")),
        }
    }

    async fn set_goal_objective<S>(&mut self, app_server: &mut S, objective: String)
    where
        S: AppShellBackend,
    {
        match app_server
            .thread_goal_set(
                self.thread_id,
                Some(objective),
                Some(ThreadGoalStatus::Active),
                /*token_budget*/ None,
            )
            .await
        {
            Ok(response) => {
                let objective = response.goal.objective.clone();
                self.record_active_goal(Some(response.goal));
                self.push_status(format!("goal set: {objective}"));
            }
            Err(err) => self.push_error(format!("failed to set goal: {err}")),
        }
    }

    async fn clear_goal<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        match app_server.thread_goal_clear(self.thread_id).await {
            Ok(response) => {
                self.record_active_goal(None);
                if response.cleared {
                    self.push_status("goal cleared");
                } else {
                    self.push_status("no goal is currently set");
                }
            }
            Err(err) => self.push_error(format!("failed to clear goal: {err}")),
        }
    }

    async fn update_goal_status<S>(
        &mut self,
        app_server: &mut S,
        status: ThreadGoalStatus,
        action: &str,
    ) where
        S: AppShellBackend,
    {
        match app_server
            .thread_goal_set(
                self.thread_id,
                /*objective*/ None,
                Some(status),
                /*token_budget*/ None,
            )
            .await
        {
            Ok(response) => {
                self.record_active_goal(Some(response.goal));
                self.push_status(format!("goal {action}"));
            }
            Err(err) => self.push_error(format!("failed to update goal: {err}")),
        }
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

    async fn submit_prompt<S>(&mut self, app_server: &mut S, prompt: String) -> Result<()>
    where
        S: AppShellBackend,
    {
        if self.active_turn_id.is_some() {
            self.push_system("wait for the current turn to finish before sending another message");
            return Ok(());
        }

        self.scroll_transcript_to_bottom();
        let transcript_len_before_submit = self.transcript.len();
        self.push_user(prompt.clone());
        self.status = "thinking".to_string();
        self.clear_streaming_transcript();
        let params = AppShellTurnStart {
            thread_id: self.thread_id,
            items: vec![UserInput::Text {
                text: prompt.clone(),
                text_elements: Vec::new(),
            }],
            cwd: self.cwd.clone().into(),
            approval_policy: self.approval_policy,
            approvals_reviewer: self.approvals_reviewer,
            permissions_override: TurnPermissionsOverride::Preserve,
            workspace_roots: self.runtime_workspace_roots.clone(),
            model: self.model.clone(),
            effort: self.reasoning_effort.clone(),
            summary: None,
            service_tier: Some(self.service_tier.clone()),
            collaboration_mode: self.collaboration_mode.as_deref().cloned(),
            personality: self.personality,
            output_schema: None,
        };
        let response = app_server.turn_start(params.clone()).await?;
        self.composer.remember_submission(&prompt);
        self.composer.clear();
        self.active_turn_id = Some(response.turn.id.clone());
        self.record_safety_buffering_turn(
            response.turn.id,
            params,
            prompt,
            transcript_len_before_submit,
        );
        Ok(())
    }

    async fn interrupt_active_turn<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
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

    async fn steer_active_turn<S>(&mut self, app_server: &mut S, prompt: String) -> Result<()>
    where
        S: AppShellBackend,
    {
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

    async fn resolve_pending_approval<S>(
        &mut self,
        app_server: &mut S,
        choice: ApprovalChoice,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
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

    async fn handle_pending_approval_action<S>(
        &mut self,
        app_server: &mut S,
        action: ApprovalAction,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
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

    async fn edit_pending_approval<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
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

    fn apply_composer_backspace_action(&mut self, action: ComposerBackspaceAction) {
        match action {
            ComposerBackspaceAction::DeleteChar => self.composer.backspace(),
            ComposerBackspaceAction::DeleteWordLeft => self.composer.delete_word_left(),
            ComposerBackspaceAction::Clear => self.composer.clear(),
        }
    }

    async fn handle_user_input_key<S>(&mut self, key: KeyEvent, app_server: &mut S) -> Result<bool>
    where
        S: AppShellBackend,
    {
        if let Some(action) = composer_backspace_action_from_key(key) {
            self.apply_composer_backspace_action(action);
            return Ok(false);
        }

        match key.code {
            KeyCode::Esc => Ok(false),
            KeyCode::Enter => {
                if is_composer_newline_key(key) {
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

    async fn resolve_pending_user_input<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
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

    async fn resolve_pending_elicitation<S>(
        &mut self,
        app_server: &mut S,
        choice: ElicitationChoice,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
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
        self.streaming_assistant_revision = next_transcript_render_revision();
        self.push_assistant(message);
    }

    fn finish_streaming_plan(&mut self) {
        if self.streaming_plan.trim().is_empty() {
            return;
        }
        let plan = std::mem::take(&mut self.streaming_plan);
        self.streaming_plan_revision = next_transcript_render_revision();
        self.push_plan(plan);
    }

    fn push_streaming_assistant_delta(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        self.streaming_assistant.push_str(delta);
        self.streaming_assistant_revision = next_transcript_render_revision();
    }

    fn push_streaming_plan_delta(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        self.streaming_plan.push_str(delta);
        self.streaming_plan_revision = next_transcript_render_revision();
    }

    fn clear_streaming_assistant(&mut self) {
        if self.streaming_assistant.is_empty() {
            return;
        }
        self.streaming_assistant.clear();
        self.streaming_assistant_revision = next_transcript_render_revision();
    }

    fn clear_streaming_plan(&mut self) {
        if self.streaming_plan.is_empty() {
            return;
        }
        self.streaming_plan.clear();
        self.streaming_plan_revision = next_transcript_render_revision();
    }

    fn clear_streaming_transcript(&mut self) {
        self.clear_streaming_assistant();
        self.clear_streaming_plan();
    }

    fn ingest_completed_item(&mut self, item: ThreadItem, origin: CompletedItemOrigin) {
        self.agent_activity.reduce_completed(&item);
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
                        self.clear_streaming_assistant();
                    }
                    self.push_assistant(text);
                }
            }
            ThreadItem::Plan { text, .. } => {
                if !text.is_empty() {
                    if self.streaming_plan == text {
                        self.clear_streaming_plan();
                    }
                    self.push_plan(text);
                }
            }
            ThreadItem::Reasoning {
                summary, content, ..
            } => {
                let text = if summary.is_empty() {
                    content.join("\n\n")
                } else {
                    crate::reasoning_summary::split_reasoning_summary_parts(&summary).1
                };
                let text = text.trim();
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
                let tool_status = command_tool_status(&status, exit_code);
                self.upsert_tool_activity(
                    id.clone(),
                    title.clone(),
                    format!("{status:?}").to_lowercase(),
                );
                self.push_tool_with_status_for_item(id.clone(), title, tool_status);
                if let Some(output) = aggregated_output.and_then(compact_output_text) {
                    self.push_output_with_status_for_item(id, output, tool_status);
                } else {
                    self.update_output_status_for_item(&id, tool_status);
                }
            }
            ThreadItem::FileChange {
                id,
                changes,
                status,
            } => {
                let summary = file_change_summary(&changes);
                self.latest_diff = Some(diff_summary_from_changes(&changes));
                self.upsert_tool_activity(
                    id.clone(),
                    summary,
                    format!("{status:?}").to_lowercase(),
                );
                self.push_diff_with_status_for_item(
                    id,
                    file_change_detail(&changes),
                    tool_status_from_debug(&status),
                );
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
                let tool_status = tool_status_from_debug(&status);
                self.upsert_tool_activity(
                    id.clone(),
                    title.clone(),
                    format!("{status:?}").to_lowercase(),
                );
                self.push_tool_with_status_for_item(id, title, tool_status);
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
                let tool_status = dynamic_tool_status(&status, success);
                self.upsert_tool_activity(
                    id.clone(),
                    title.clone(),
                    format!("{status:?}").to_lowercase(),
                );
                self.push_tool_with_status_for_item(id, title, tool_status);
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
                    id.clone(),
                    title.clone(),
                    format!("{status:?}").to_lowercase(),
                );
                match origin {
                    CompletedItemOrigin::Historical => {}
                    CompletedItemOrigin::Live => self.push_tool_with_status_for_item(
                        id,
                        title,
                        tool_status_from_debug(&status),
                    ),
                }
            }
            ThreadItem::SubAgentActivity {
                id,
                kind,
                agent_path,
                ..
            } => {
                let title = format!("subagent {kind:?}: {agent_path}");
                self.upsert_subagent_activity(id, title, "recorded".to_string());
            }
            ThreadItem::WebSearch(item) => {
                let title = format!("web search: {}", item.query);
                self.upsert_tool_activity(
                    item.id.clone(),
                    title.clone(),
                    item.action
                        .as_ref()
                        .map_or_else(|| "completed".to_string(), |action| format!("{action:?}")),
                );
                self.push_tool_with_status_for_item(item.id, title, ToolBlockStatus::Success);
            }
            ThreadItem::ImageView { id, path } => {
                let title = format!("view image: {path}");
                self.upsert_tool_activity(id.clone(), title.clone(), "completed".to_string());
                self.push_tool_with_status_for_item(id, title, ToolBlockStatus::Success);
            }
            ThreadItem::Sleep { id, duration_ms } => {
                let title = format!("sleep {duration_ms}ms");
                self.upsert_tool_activity(id.clone(), title.clone(), "completed".to_string());
                self.push_tool_with_status_for_item(id, title, ToolBlockStatus::Success);
            }
            ThreadItem::ImageGeneration(item) => {
                let title = item
                    .saved_path
                    .map(|path| format!("image generation: {}", path.as_path().display()))
                    .unwrap_or_else(|| "image generation".to_string());
                let tool_status = tool_status_from_str(&item.status);
                self.upsert_tool_activity(item.id.clone(), title.clone(), item.status);
                self.push_tool_with_status_for_item(item.id, title, tool_status);
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

    fn record_item_activity(&mut self, item: &ThreadItem, title: String, status: &str) {
        let id = item.id().to_string();
        match item {
            ThreadItem::CollabAgentToolCall { .. } | ThreadItem::SubAgentActivity { .. } => {
                self.upsert_subagent_activity(id, title, status.to_string());
            }
            _ => self.upsert_tool_activity(id, title, status.to_string()),
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
        self.push_tool_with_status(text, ToolBlockStatus::Running);
    }

    fn push_tool_with_status(&mut self, text: impl Into<String>, status: ToolBlockStatus) {
        self.push_line(TranscriptLine::new(TranscriptKind::Tool, text).tool_status(status));
    }

    fn push_tool_with_status_for_item(
        &mut self,
        item_id: impl Into<String>,
        text: impl Into<String>,
        status: ToolBlockStatus,
    ) {
        self.upsert_line(
            TranscriptLine::new(TranscriptKind::Tool, text)
                .tool_status(status)
                .item_id(item_id),
        );
    }

    #[cfg(test)]
    fn push_diff(&mut self, text: impl Into<String>) {
        self.push_diff_with_status(text, ToolBlockStatus::Success);
    }

    fn push_diff_with_status(&mut self, text: impl Into<String>, status: ToolBlockStatus) {
        self.push_line(TranscriptLine::new(TranscriptKind::Diff, text).tool_status(status));
    }

    fn push_diff_with_status_for_item(
        &mut self,
        item_id: impl Into<String>,
        text: impl Into<String>,
        status: ToolBlockStatus,
    ) {
        self.upsert_line(
            TranscriptLine::new(TranscriptKind::Diff, text)
                .tool_status(status)
                .item_id(item_id),
        );
    }

    #[cfg(test)]
    fn push_output(&mut self, text: impl Into<String>) {
        self.push_output_with_status(text, ToolBlockStatus::Running);
    }

    fn push_output_with_status(&mut self, text: impl Into<String>, status: ToolBlockStatus) {
        self.push_line(TranscriptLine::new(TranscriptKind::Output, text).tool_status(status));
    }

    fn push_turn_separator(&mut self) {
        self.push_line(TranscriptLine::new(TranscriptKind::Separator, ""));
    }

    fn push_output_with_status_for_item(
        &mut self,
        item_id: impl Into<String>,
        text: impl Into<String>,
        status: ToolBlockStatus,
    ) {
        self.upsert_line(
            TranscriptLine::new(TranscriptKind::Output, text)
                .tool_status(status)
                .item_id(item_id),
        );
    }

    fn push_output_delta_with_status_for_item(
        &mut self,
        item_id: impl Into<String>,
        delta: impl Into<String>,
        status: ToolBlockStatus,
    ) {
        let item_id = item_id.into();
        let delta = delta.into();
        if delta.is_empty() {
            return;
        }

        if let Some(existing) = self.transcript.iter_mut().rev().find(|existing| {
            existing.kind == TranscriptKind::Output && existing.item_id.as_deref() == Some(&item_id)
        }) {
            existing.text.push_str(&delta);
            existing.text = compact_output_for_transcript(std::mem::take(&mut existing.text));
            existing.tool_status = Some(status);
            existing.mark_render_changed();
            return;
        }

        self.push_output_with_status_for_item(
            item_id,
            compact_output_for_transcript(delta),
            status,
        );
    }

    fn update_output_status_for_item(&mut self, item_id: &str, status: ToolBlockStatus) {
        if let Some(existing) = self.transcript.iter_mut().rev().find(|existing| {
            existing.kind == TranscriptKind::Output && existing.item_id.as_deref() == Some(item_id)
        }) {
            existing.tool_status = Some(status);
            existing.mark_render_changed();
        }
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

    fn upsert_line(&mut self, line: TranscriptLine) {
        if let Some(item_id) = line.item_id.as_deref()
            && let Some(existing) = self.transcript.iter_mut().rev().find(|existing| {
                existing.kind == line.kind && existing.item_id.as_deref() == Some(item_id)
            })
        {
            *existing = line;
            return;
        }

        self.push_line(line);
    }

    fn resume_hint(&self) -> Option<String> {
        let thread = self
            .thread_name
            .clone()
            .unwrap_or_else(|| self.thread_id.to_string());
        Some(format!("codex resume {thread}"))
    }

    fn dashboard_focused(&self) -> bool {
        self.dashboard_visible
            && (self.session_list.focused || self.settings.focused || self.agents_focused)
    }

    #[cfg(test)]
    fn snapshot_fixture() -> Self {
        let mut shell = Self {
            thread_id: ThreadId::from_string("01900000-0000-7000-8000-000000000001")
                .expect("valid snapshot thread id"),
            thread_name: Some("stage-one".to_string()),
            model: "gpt-5-codex".to_string(),
            available_models: Vec::new(),
            cwd: "/workspace/better-codex".to_string(),
            approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer::User,
            permission_profile: codex_protocol::models::PermissionProfile::default(),
            runtime_workspace_roots: Vec::new(),
            reasoning_effort: None,
            service_tier: None,
            collaboration_mode: None,
            max_concurrent_threads_per_session: 4,
            personality: None,
            transcript: VecDeque::new(),
            transcript_scroll: 0,
            transcript_scroll_max: Cell::new(0),
            transcript_selection: None,
            transcript_render_cache: RefCell::new(TranscriptRenderCache::default()),
            session_list: SessionListState::default(),
            settings: SettingsState::default(),
            mcp_inventory: McpInventorySummary::default(),
            mcp_catalog: None,
            plugin_inventory: PluginInventorySummary::default(),
            plugin_catalog: None,
            tui_theme: None,
            animations: true,
            show_tooltips: true,
            command_palette: None,
            selector: None,
            codex_home: std::path::PathBuf::from("/tmp/codex-home"),
            dashboard_route: DashboardRoute::Sessions,
            dashboard_visible: true,
            pointer_position: None,
            agents_focused: false,
            composer: {
                let mut composer = ComposerState::default();
                composer.set_text("Summarize the new shell architecture");
                composer
            },
            workspace_command_runner: None,
            pending_shell_command: None,
            session_hydration: SessionHydrationState::default(),
            exit_confirmation_pending: false,
            clipboard_lease: None,
            active_turn_id: None,
            pending_approval: None,
            pending_elicitation: None,
            pending_external_agent_import: None,
            pending_mcp_management: None,
            pending_plugin_management: None,
            pending_user_input: None,
            safety_buffering: SafetyBufferingState::default(),
            streaming_assistant: "The new shell owns the fullscreen surface.".to_string(),
            streaming_assistant_revision: next_transcript_render_revision(),
            streaming_plan: String::new(),
            streaming_plan_revision: next_transcript_render_revision(),
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
            agent_activity: AgentActivityState::default(),
            agent_log: None,
            agent_history_task: None,
            active_agent_thread_ids: HashSet::new(),
            deferred_unsubscribe_thread_ids: Vec::new(),
            subscription_cleanup_task: None,
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
            context_token_usage: TokenUsage {
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
        shell.push_diff("3 files +128 -24");
        shell
    }
}

#[doc(hidden)]
pub mod bench_support {
    use super::render::ShellView;
    use super::*;
    use itertools::Itertools;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const BENCH_AREA: Rect = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 120, /*height*/ 40,
    );
    const LONG_HISTORY_TURNS: usize = MAX_TRANSCRIPT_LINES / 2;
    const APPROX_OUTPUT_TOKENS_PER_TURN: usize = 260;
    const APPROX_STREAMING_OUTPUT_TOKENS: usize = 50_000;
    const BENCH_TOOL_ITEM_ID: &str = "tool-output-bench";

    /// Reusable app-shell state for render benchmarks.
    pub struct RenderFixture {
        shell: ShellState,
        next_tool_output_line: usize,
    }

    impl RenderFixture {
        pub fn render(&self) -> String {
            render_to_string(&self.shell)
        }

        pub fn scroll_and_render(&mut self) -> String {
            self.toggle_scroll();
            self.render()
        }

        pub fn append_tool_output_and_render(&mut self) -> String {
            self.append_tool_output();
            self.render()
        }

        pub fn append_tool_output_scroll_and_render(&mut self) -> String {
            self.append_tool_output();
            self.toggle_scroll();
            self.render()
        }

        fn toggle_scroll(&mut self) {
            if self.shell.transcript_scroll == 0 {
                self.shell.scroll_transcript_up(TRANSCRIPT_PAGE_SCROLL_STEP);
            } else {
                self.shell
                    .scroll_transcript_down(TRANSCRIPT_PAGE_SCROLL_STEP);
            }
        }

        fn append_tool_output(&mut self) {
            let line = self.next_tool_output_line;
            self.next_tool_output_line = self.next_tool_output_line.saturating_add(1);
            self.shell.push_output_delta_with_status_for_item(
                BENCH_TOOL_ITEM_ID,
                format!(
                    "cargo build output line {line}: compiling a representative workspace dependency\n"
                ),
                ToolBlockStatus::Running,
            );
        }
    }

    pub fn long_history_fixture() -> RenderFixture {
        let mut shell = bench_fixture();
        shell.transcript.clear();
        shell.streaming_assistant.clear();

        let assistant_output =
            std::iter::repeat_n("response", APPROX_OUTPUT_TOKENS_PER_TURN).join(" ");
        for index in 0..LONG_HISTORY_TURNS {
            shell.push_user(format!(
                "long history user turn {index}: continue the benchmark conversation"
            ));
            shell.push_assistant(format!("turn {index}: {assistant_output}"));
        }

        RenderFixture {
            shell,
            next_tool_output_line: 0,
        }
    }

    pub fn long_streaming_turn_fixture() -> RenderFixture {
        let mut shell = bench_fixture();
        shell.transcript.clear();
        shell.streaming_assistant =
            std::iter::repeat_n("streaming", APPROX_STREAMING_OUTPUT_TOKENS).join(" ");

        RenderFixture {
            shell,
            next_tool_output_line: 0,
        }
    }

    pub fn active_tool_output_fixture() -> RenderFixture {
        let mut fixture = long_history_fixture();
        fixture.shell.push_tool_with_status_for_item(
            BENCH_TOOL_ITEM_ID,
            "exec cargo build --workspace",
            ToolBlockStatus::Running,
        );
        let initial_output = (0..TRANSCRIPT_OUTPUT_HIGH_WATER_LINES)
            .map(|line| {
                format!(
                    "cargo build output line {line}: compiling a representative workspace dependency"
                )
            })
            .join("\n");
        fixture.shell.push_output_with_status_for_item(
            BENCH_TOOL_ITEM_ID,
            compact_output_for_transcript(initial_output),
            ToolBlockStatus::Running,
        );
        fixture.next_tool_output_line = TRANSCRIPT_OUTPUT_HIGH_WATER_LINES;
        fixture
    }

    fn bench_fixture() -> ShellState {
        let mut shell = ShellState {
            thread_id: ThreadId::new(),
            thread_name: Some("bench".to_string()),
            model: "gpt-5-codex".to_string(),
            available_models: Vec::new(),
            cwd: "/workspace/better-codex".to_string(),
            approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer::User,
            permission_profile: codex_protocol::models::PermissionProfile::default(),
            runtime_workspace_roots: Vec::new(),
            reasoning_effort: None,
            service_tier: None,
            collaboration_mode: None,
            max_concurrent_threads_per_session: 4,
            personality: None,
            transcript: VecDeque::new(),
            transcript_scroll: 0,
            transcript_scroll_max: Cell::new(0),
            transcript_selection: None,
            transcript_render_cache: RefCell::new(TranscriptRenderCache::default()),
            session_list: SessionListState::default(),
            settings: SettingsState::default(),
            mcp_inventory: McpInventorySummary::default(),
            mcp_catalog: None,
            plugin_inventory: PluginInventorySummary::default(),
            plugin_catalog: None,
            tui_theme: None,
            animations: true,
            show_tooltips: true,
            command_palette: None,
            selector: None,
            codex_home: std::path::PathBuf::from("/tmp/codex-home"),
            dashboard_route: DashboardRoute::Sessions,
            dashboard_visible: true,
            pointer_position: None,
            agents_focused: false,
            composer: {
                let mut composer = ComposerState::default();
                composer.set_text("Benchmark the app shell render path");
                composer
            },
            workspace_command_runner: None,
            pending_shell_command: None,
            session_hydration: SessionHydrationState::default(),
            exit_confirmation_pending: false,
            clipboard_lease: None,
            active_turn_id: Some("turn-bench-1234567890".to_string()),
            pending_approval: None,
            pending_elicitation: None,
            pending_external_agent_import: None,
            pending_mcp_management: None,
            pending_plugin_management: None,
            pending_user_input: None,
            safety_buffering: SafetyBufferingState::default(),
            streaming_assistant: String::new(),
            streaming_assistant_revision: next_transcript_render_revision(),
            streaming_plan: String::new(),
            streaming_plan_revision: next_transcript_render_revision(),
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
            agent_activity: AgentActivityState::default(),
            agent_log: None,
            agent_history_task: None,
            active_agent_thread_ids: HashSet::new(),
            deferred_unsubscribe_thread_ids: Vec::new(),
            subscription_cleanup_task: None,
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
            context_token_usage: TokenUsage {
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

fn command_tool_status<T: std::fmt::Debug>(status: &T, exit_code: Option<i32>) -> ToolBlockStatus {
    if exit_code.is_some_and(|exit_code| exit_code != 0) {
        return ToolBlockStatus::Fail;
    }
    tool_status_from_debug(status)
}

fn dynamic_tool_status<T: std::fmt::Debug>(status: &T, success: Option<bool>) -> ToolBlockStatus {
    match success {
        Some(true) => ToolBlockStatus::Success,
        Some(false) => ToolBlockStatus::Fail,
        None => tool_status_from_debug(status),
    }
}

fn tool_status_from_debug<T: std::fmt::Debug>(status: &T) -> ToolBlockStatus {
    tool_status_from_str(&format!("{status:?}"))
}

fn tool_status_from_str(status: &str) -> ToolBlockStatus {
    let status = status.to_ascii_lowercase();
    if status.contains("fail")
        || status.contains("error")
        || status.contains("cancel")
        || status.contains("declin")
    {
        ToolBlockStatus::Fail
    } else if status.contains("complete")
        || status.contains("success")
        || status.contains("succeed")
        || status == "ok"
        || status.contains("done")
    {
        ToolBlockStatus::Success
    } else {
        ToolBlockStatus::Running
    }
}

fn file_change_summary(changes: &[FileUpdateChange]) -> String {
    let summary = diff_summary_from_changes(changes);
    format!(
        "{} files +{} -{}",
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

fn compact_output_text(text: String) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(compact_output_for_transcript(text.to_string()))
    }
}

fn compact_output_for_transcript(text: String) -> String {
    let was_compacted = text.starts_with(TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX);
    let source = if was_compacted {
        &text[TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX.len()..]
    } else {
        text.as_str()
    };
    let needs_compaction = source
        .chars()
        .nth(TRANSCRIPT_OUTPUT_HIGH_WATER_CHARS)
        .is_some()
        || source
            .split(['\n', '\r'])
            .nth(TRANSCRIPT_OUTPUT_HIGH_WATER_LINES)
            .is_some();
    if !needs_compaction {
        return text;
    }

    let normalized = source.replace('\r', "\n");
    let mut tail_lines = normalized
        .lines()
        .rev()
        .take(TRANSCRIPT_OUTPUT_LOW_WATER_LINES)
        .collect::<Vec<_>>();
    tail_lines.reverse();
    let mut compact = tail_lines.join("\n");
    if compact
        .chars()
        .nth(TRANSCRIPT_OUTPUT_LOW_WATER_CHARS)
        .is_some()
    {
        let mut tail_chars = compact
            .chars()
            .rev()
            .take(TRANSCRIPT_OUTPUT_LOW_WATER_CHARS)
            .collect::<Vec<_>>();
        tail_chars.reverse();
        compact = tail_chars.into_iter().collect();
    }
    let mut output = String::with_capacity(
        TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX.len() + TRANSCRIPT_OUTPUT_HIGH_WATER_CHARS,
    );
    output.push_str(TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX);
    output.push_str(compact.trim_start_matches('\n'));
    output
}

fn dashboard_route_from_key(key: KeyEvent) -> Option<DashboardRoute> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    if key_hint::ctrl(KeyCode::Char('1')).is_press(key) {
        return Some(DashboardRoute::Settings);
    }
    if key_hint::ctrl(KeyCode::Char('2')).is_press(key) {
        return Some(DashboardRoute::Agents);
    }
    if key_hint::ctrl(KeyCode::Char(' ')).is_press(key) {
        return Some(DashboardRoute::Workspace);
    }
    if key_hint::ctrl(KeyCode::Char('3')).is_press(key) {
        return Some(DashboardRoute::Workspace);
    }
    if key_hint::ctrl(KeyCode::Char('4')).is_press(key) {
        return Some(DashboardRoute::Sessions);
    }
    if key_hint::ctrl(KeyCode::Char('5')).is_press(key) {
        return Some(DashboardRoute::Help);
    }

    match key {
        KeyEvent {
            code: KeyCode::Char('\u{0000}') | KeyCode::Null,
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers.contains(KeyModifiers::CONTROL) => {
            Some(DashboardRoute::Workspace)
        }
        KeyEvent {
            code: KeyCode::Char('\u{001b}'),
            modifiers,
            ..
        } if modifiers.is_empty() => Some(DashboardRoute::Settings),
        KeyEvent {
            code: KeyCode::Esc,
            modifiers,
            ..
        } if modifiers.contains(KeyModifiers::CONTROL) => Some(DashboardRoute::Settings),
        _ => None,
    }
}

fn is_unmodified_action_key(key: KeyEvent) -> bool {
    is_unmodified_key_event(key)
        && (is_unmodified_key_press(key)
            || key.kind == KeyEventKind::Repeat
                && matches!(
                    key.code,
                    KeyCode::Up
                        | KeyCode::Down
                        | KeyCode::Left
                        | KeyCode::Right
                        | KeyCode::Home
                        | KeyCode::End
                        | KeyCode::PageUp
                        | KeyCode::PageDown
                ))
}

fn is_unmodified_key_event(key: KeyEvent) -> bool {
    key.modifiers == KeyModifiers::NONE
        && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

fn is_unmodified_key_press(key: KeyEvent) -> bool {
    key.modifiers == KeyModifiers::NONE && key.kind == KeyEventKind::Press
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DashboardRouteStep {
    Previous,
    Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerWordMotion {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerBackspaceAction {
    DeleteChar,
    DeleteWordLeft,
    Clear,
}

fn composer_backspace_action_from_key(key: KeyEvent) -> Option<ComposerBackspaceAction> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    let is_backspace = matches!(key.code, KeyCode::Backspace)
        || matches!(key.code, KeyCode::Char('\u{007f}'))
            && (key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT));
    if !is_backspace {
        return None;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        Some(ComposerBackspaceAction::Clear)
    } else if key.modifiers.contains(KeyModifiers::ALT) {
        Some(ComposerBackspaceAction::DeleteWordLeft)
    } else {
        Some(ComposerBackspaceAction::DeleteChar)
    }
}

fn is_composer_newline_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
}

fn composer_word_motion_from_key(key: KeyEvent) -> Option<ComposerWordMotion> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    match key {
        KeyEvent {
            code: KeyCode::Left,
            modifiers,
            ..
        } if modifiers.contains(KeyModifiers::CONTROL) || modifiers.contains(KeyModifiers::ALT) => {
            Some(ComposerWordMotion::Left)
        }
        KeyEvent {
            code: KeyCode::Right,
            modifiers,
            ..
        } if modifiers.contains(KeyModifiers::CONTROL) || modifiers.contains(KeyModifiers::ALT) => {
            Some(ComposerWordMotion::Right)
        }
        _ => composer_word_motion_fallback(key),
    }
}

fn composer_word_motion_fallback(key: KeyEvent) -> Option<ComposerWordMotion> {
    // Terminals without enhanced keyboard reporting can encode Alt+Left/Right as Alt+b/f.
    // Recognize both forms on every platform so Ubuntu terminal configurations behave like the
    // canonical arrow bindings.
    match key {
        KeyEvent {
            code: KeyCode::Char('b'),
            modifiers: KeyModifiers::ALT,
            ..
        } => Some(ComposerWordMotion::Left),
        KeyEvent {
            code: KeyCode::Char('f'),
            modifiers: KeyModifiers::ALT,
            ..
        } => Some(ComposerWordMotion::Right),
        _ => None,
    }
}

fn dashboard_route_step_from_key(
    key: KeyEvent,
    allow_word_motion_fallback: bool,
) -> Option<DashboardRouteStep> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    if key.modifiers.contains(KeyModifiers::ALT) {
        match key.code {
            KeyCode::Left => return Some(DashboardRouteStep::Previous),
            KeyCode::Right => return Some(DashboardRouteStep::Next),
            _ => {}
        }
    }
    dashboard_route_word_motion_fallback(key, allow_word_motion_fallback)
}

fn dashboard_route_word_motion_fallback(
    key: KeyEvent,
    allow_word_motion_fallback: bool,
) -> Option<DashboardRouteStep> {
    if !allow_word_motion_fallback {
        return None;
    }

    match key {
        KeyEvent {
            code: KeyCode::Char('b'),
            modifiers: KeyModifiers::ALT,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => Some(DashboardRouteStep::Previous),
        KeyEvent {
            code: KeyCode::Char('f'),
            modifiers: KeyModifiers::ALT,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        } => Some(DashboardRouteStep::Next),
        _ => None,
    }
}

fn approval_action_from_key(key: KeyEvent) -> Option<ApprovalAction> {
    if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
        return None;
    }
    match key.code {
        KeyCode::Enter | KeyCode::Char('a' | 'A' | 'y' | 'Y') => {
            Some(ApprovalAction::Choose(ApprovalChoice::Approve))
        }
        KeyCode::Esc | KeyCode::Char('d' | 'D' | 'n' | 'N') => {
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
