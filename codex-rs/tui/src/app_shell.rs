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
use crate::text_input::text_input_action_from_key;
use crate::token_usage::TokenUsage;
use crate::tui;
use crate::tui::TuiEvent;
use crate::workspace_command::AppServerWorkspaceCommandRunner;
use crate::workspace_command::WorkspaceCommandRunner;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::ListMcpServerStatusParams;
use codex_app_server_protocol::ListMcpServerStatusResponse;
use codex_app_server_protocol::McpServerStatusDetail;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::PluginListParams;
use codex_app_server_protocol::PluginListResponse;
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnPlanStep;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_config::types::TuiAppTheme;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ModelPreset;
use codex_utils_absolute_path::AbsolutePathBuf;
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

mod account_auth;
mod agent_activity;
mod agent_activity_controller;
mod agent_activity_render;
mod agent_history;
mod agent_log;
mod agent_log_format;
mod agent_log_view;
mod approval;
mod backend;
mod backend_actions;
mod backend_background;
mod backend_cleanup;
mod command_display;
mod command_palette;
mod command_palette_view;
mod composer;
mod composer_render;
mod dashboard;
mod dashboard_help;
mod dashboard_rate_limits;
mod dashboard_resize;
mod dashboard_view;
mod dashboard_workspace;
mod design;
mod diff_horizontal_scroll;
mod diff_metadata_view;
mod diff_model;
mod diff_path;
mod diff_session;
mod diff_style;
mod diff_view;
mod diff_view_controller;
mod diff_view_view;
mod elicitation;
mod events;
mod external_agent_import;
mod header;
mod input_request_layout;
mod input_request_view;
mod input_router;
mod integrations;
mod interactive_requests;
mod local_app_theme;
mod mcp_management;
mod modal_view;
mod navigation;
mod paste;
mod plugin_management;
mod pointer;
mod queued_message_popup_view;
mod queued_messages;
mod reasoning_ripple;
mod render;
mod rewind;
mod safety_buffering;
mod scrollback_view;
mod selection_controller;
mod selector;
mod selector_controller;
mod session_delete;
mod session_hydration;
mod session_lifecycle;
mod session_list_controller;
mod session_switch;
mod sessions;
mod settings;
mod shell_command;
mod shell_layout;
mod slash_command_popup;
mod slash_command_popup_view;
mod slash_commands;
mod startup;
mod startup_availability_nux;
mod startup_layout;
mod startup_login;
mod startup_model_migration;
mod terminal_output;
mod text_selection;
mod tool_output;
mod tool_output_view;
mod transcript_render;
mod transcript_selection;
mod transcript_view;
mod turn_timer;
mod user_input;
mod vim_input;
mod workspace;
use account_auth::AccountAuthState;
use agent_activity::AgentActivityState;
use agent_log::AgentLogState;
use approval::ApprovalAction;
use approval::ApprovalSelectionDirection;
use approval::PendingApproval;
use backend::AppShellBackend;
use backend::AppShellTurnStart;
use backend::AppShellTurnSteer;
use backend::shutdown_app_shell_backend;
use backend_actions::ActionGroup;
use backend_actions::BackendActionResult;
use backend_actions::TurnSubmission;
use command_palette::CommandPaletteAction;
use command_palette::CommandPaletteContext;
use command_palette::CommandPaletteEntry;
use command_palette::CommandPaletteState;
use command_palette::command_palette_entries;
use composer::ComposerState;
use diff_view::DiffStore;
use diff_view::DiffViewState;
use elicitation::ElicitationChoice;
use elicitation::PendingElicitation;
use elicitation::elicitation_action_from_key;
use external_agent_import::ExternalAgentImportState;
use integrations::McpInventorySummary;
use integrations::PluginInventorySummary;
use interactive_requests::InteractiveRequestRemoval;
use interactive_requests::PendingInteractiveRequest;
use mcp_management::McpManagementState;
use navigation::DashboardRoute;
use plugin_management::PluginManagementState;
use reasoning_ripple::ReasoningRipple;
use render::draw_shell;
use safety_buffering::SafetyBufferingState;
use selection_controller::TextSelectionState;
use selector::SelectorState;
use selector::SelectorValue;
use session_delete::PendingSessionDelete;
use session_hydration::SessionHydrationState;
use sessions::SessionListState;
use settings::SettingsAction;
use settings::SettingsState;
use shell_command::PendingShellCommand;
use shell_command::ShellCommand;
use shell_layout::terminal_width_supported;
use slash_command_popup::SlashCommandPopupKeyResult;
use slash_command_popup::SlashCommandPopupState;
use slash_commands::GoalSlashCommand;
use slash_commands::LocalSlashCommand;
pub(crate) use startup::StartupOnboardingOutcome;
pub(crate) use startup::run_startup_onboarding;
pub(crate) use startup_login::LoginOnboardingOutcome;
pub(crate) use startup_login::run_login_onboarding;
use tool_output::ToolOutputBuffer;
use tool_output::ToolOutputState;
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
const BACKEND_ACTION_POLL_INTERVAL: Duration = Duration::from_millis(25);
const AGENT_HISTORY_POLL_INTERVAL: Duration = Duration::from_millis(50);
const RATE_LIMITS_REFRESH_INTERVAL: Duration = Duration::from_secs(/*secs*/ 60);
const WORKSPACE_STATUS_POLL_INTERVAL: Duration = Duration::from_secs(/*secs*/ 5);
const STATUS_SPINNER_FRAME_INTERVAL: Duration = Duration::from_millis(120);
const TURN_TIMER_REFRESH_INTERVAL: Duration = Duration::from_secs(/*secs*/ 1);

fn next_transcript_render_revision() -> u64 {
    static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);
    NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
}

fn next_local_output_item_id() -> String {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("local-output:{id}")
}

pub(crate) async fn run(
    tui: &mut tui::Tui,
    mut app_server: AppServerSession,
    mut config: Config,
    resume_cwd_runtime: ResumeCwdRuntime,
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
    let client_config_path = local_app_theme::selected_config_path(&config);
    let mut shell = ShellState::new(
        session,
        fallback_model,
        bootstrap.available_models,
        ShellClientConfig {
            codex_home: config.codex_home.to_path_buf(),
            config_path: client_config_path,
            app_theme: config.tui_app_theme,
            tui_theme: config.tui_theme.clone(),
            animations: config.animations,
            show_tooltips: config.show_tooltips,
        },
        resume_cwd_runtime,
        config.multi_agent_v2.max_concurrent_threads_per_session,
    );
    shell.workspace_command_runner = Some(workspace_command_runner.clone());
    shell.ingest_turn_history(turns);
    shell.install_agent_history(agent_threads, agent_history_task);
    for error in tui.take_startup_errors() {
        shell.push_error(error);
    }
    if let Some(message) = availability_nux {
        shell.push_system(message);
    }
    // Paint the restored conversation and start accepting input before secondary dashboard data
    // completes. These lookups can cross a remote app-server boundary, so their results are
    // revision-guarded and applied from the event loop as they become available.
    let mut pending_initial_prompt = initial_prompt.filter(|prompt| !prompt.trim().is_empty());
    let has_initial_prompt = pending_initial_prompt.is_some();
    draw_shell(tui, &shell)?;
    shell.start_initial_dashboard_hydration(&app_server);
    if !has_initial_prompt {
        shell.start_initial_goal_hydration(&app_server);
    }

    let run_result: Result<ExitReason> = async {
        let mut tui_events = tui.event_stream();
        let mut agent_history_poll = tokio::time::interval(AGENT_HISTORY_POLL_INTERVAL);
        agent_history_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut backend_action_poll = tokio::time::interval(BACKEND_ACTION_POLL_INTERVAL);
        backend_action_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut rate_limits_refresh = tokio::time::interval_at(
            tokio::time::Instant::now() + RATE_LIMITS_REFRESH_INTERVAL,
            RATE_LIMITS_REFRESH_INTERVAL,
        );
        rate_limits_refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut workspace_status_poll = tokio::time::interval_at(
            tokio::time::Instant::now() + WORKSPACE_STATUS_POLL_INTERVAL,
            WORKSPACE_STATUS_POLL_INTERVAL,
        );
        workspace_status_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut status_spinner = tokio::time::interval(STATUS_SPINNER_FRAME_INTERVAL);
        status_spinner.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut reasoning_ripple_refresh = tokio::time::interval(reasoning_ripple::FRAME_INTERVAL);
        reasoning_ripple_refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut turn_timer_refresh = tokio::time::interval_at(
            tokio::time::Instant::now() + TURN_TIMER_REFRESH_INTERVAL,
            TURN_TIMER_REFRESH_INTERVAL,
        );
        turn_timer_refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let exit_reason = 'event_loop: loop {
            if terminal_width_supported(tui.terminal.size()?.width)
                && let Some(prompt) = pending_initial_prompt.take()
            {
                shell.start_turn(&app_server, prompt, TurnSubmission::Initial);
                tui.frame_requester().schedule_frame();
            }
            let user_input_auto_resolution_deadline =
                shell.pending_user_input_auto_resolution_deadline();
            select! {
                event = tui_events.next() => {
                    let Some(event) = event else {
                        break ExitReason::UserRequested;
                    };
                    let size = tui.terminal.size()?;
                    let accepts_interaction = terminal_width_supported(size.width);
                    match event {
                        TuiEvent::Key(key) => {
                            if !accepts_interaction {
                                let exits_warning = key.kind == KeyEventKind::Press
                                    && (matches!(key.code, KeyCode::Esc)
                                        || (key.modifiers.contains(KeyModifiers::CONTROL)
                                            && matches!(key.code, KeyCode::Char('c'))));
                                if exits_warning {
                                    break ExitReason::UserRequested;
                                }
                                continue;
                            }
                            let area = ratatui::layout::Rect::new(
                                /*x*/ 0,
                                /*y*/ 0,
                                size.width,
                                size.height,
                            );
                            if shell.handle_dashboard_resize_key(area, key) {
                                tui.frame_requester().schedule_frame();
                                continue;
                            }
                            match shell.handle_key(key, &config, &mut app_server).await {
                                Ok(true) => break ExitReason::UserRequested,
                                Ok(false) => {}
                                Err(err) => shell.report_action_error("action failed", err),
                            }
                            while let Some(request) = shell.take_vim_input_request() {
                                let originating_thread_id = request.thread_id();
                                draw_shell(tui, &shell)?;
                                let wait_outcome = tui
                                    .with_restored_terminal(|| {
                                        vim_input::wait_while_processing_events(
                                            &mut shell,
                                            &mut app_server,
                                            vim_input::run(request),
                                        )
                                    })
                                    .await
                                    .wrap_err("failed to restore terminal around Vim input")?;
                                let result = match wait_outcome {
                                    vim_input::VimInputWaitOutcome::Completed(result) => result,
                                    vim_input::VimInputWaitOutcome::AppServerDisconnected => {
                                        shell.push_system("app-server disconnected");
                                        break 'event_loop ExitReason::Fatal(
                                            "app-server disconnected".to_string(),
                                        );
                                    }
                                };
                                match shell
                                    .complete_vim_input(
                                        originating_thread_id,
                                        result,
                                        &config,
                                        &mut app_server,
                                    )
                                    .await
                                {
                                    Ok(LocalSlashCommandOutcome::Continue) => {}
                                    Ok(LocalSlashCommandOutcome::Exit) => {
                                        break 'event_loop ExitReason::UserRequested;
                                    }
                                    Err(err) => {
                                        shell.report_action_error("failed to submit Vim input", err);
                                    }
                                }
                            }
                            tui.frame_requester().schedule_frame();
                        }
                        TuiEvent::MouseClick(position) => {
                            if !accepts_interaction {
                                continue;
                            }
                            if let Err(err) = shell
                                .handle_mouse_selection_down(
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
                                .await
                            {
                                shell.report_action_error("action failed", err);
                            }
                            tui.frame_requester().schedule_frame();
                        }
                        TuiEvent::MouseDrag(position) => {
                            if !accepts_interaction {
                                continue;
                            }
                            shell.handle_mouse_selection_drag(
                                ratatui::layout::Rect::new(
                                    /*x*/ 0,
                                    /*y*/ 0,
                                    size.width,
                                    size.height,
                                ),
                                position,
                            );
                            tui.frame_requester().schedule_frame();
                        }
                        TuiEvent::MouseRelease(position) => {
                            if !accepts_interaction {
                                continue;
                            }
                            if let Err(err) = shell
                                .handle_mouse_selection_release(
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
                                .await
                            {
                                shell.report_action_error("action failed", err);
                            }
                            tui.frame_requester().schedule_frame();
                        }
                        TuiEvent::MouseMove(position) => {
                            if !accepts_interaction {
                                continue;
                            }
                            if shell.set_pointer_position(position) {
                                tui.frame_requester().schedule_frame();
                            }
                        }
                        TuiEvent::MouseScroll {
                            position,
                            direction,
                        } => {
                            if !accepts_interaction {
                                continue;
                            }
                            let load_more = shell.handle_mouse_scroll(
                                ratatui::layout::Rect::new(
                                    /*x*/ 0,
                                    /*y*/ 0,
                                    size.width,
                                    size.height,
                                ),
                                position,
                                direction,
                            );
                            if load_more {
                                shell.start_session_list_next_page(&app_server);
                            }
                            tui.frame_requester().schedule_frame();
                        }
                        TuiEvent::Paste(text) => {
                            if !accepts_interaction {
                                continue;
                            }
                            shell.insert_pasted_text(&text);
                            tui.frame_requester().schedule_frame();
                        }
                        TuiEvent::Resize => {
                            shell.cancel_dashboard_resize();
                            shell.clear_text_selections();
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
                            if let Err(err) = shell
                                .handle_app_server_event(
                                    &mut app_server,
                                    event,
                                )
                                .await
                            {
                                shell.report_action_error("failed to handle app-server event", err);
                            }
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
                _ = backend_action_poll.tick(), if shell.has_pending_backend_actions() => {
                    if shell.poll_backend_actions(&app_server).await {
                        if let Err(err) = shell
                            .dispatch_pending_prompt_submission(&mut app_server)
                            .await
                        {
                            shell.report_action_error("failed to submit deferred input", err);
                        }
                        tui.frame_requester().schedule_frame();
                    }
                }
                _ = rate_limits_refresh.tick() => {
                    shell.request_rate_limits_refresh();
                }
                _ = workspace_status_poll.tick() => {
                    shell.poll_workspace_status_if_visible();
                }
                _ = tokio::time::sleep_until(
                    user_input_auto_resolution_deadline
                        .unwrap_or_else(tokio::time::Instant::now)
                ), if user_input_auto_resolution_deadline.is_some() => {
                    if shell.start_expired_user_input_resolution(&app_server) {
                        tui.frame_requester().schedule_frame();
                    }
                }
                _ = reasoning_ripple_refresh.tick(), if shell.reasoning_ripple.is_some() => {
                    let now = std::time::Instant::now();
                    if shell
                        .reasoning_ripple
                        .as_ref()
                        .is_some_and(|ripple| ripple.is_expired(now))
                    {
                        shell.reasoning_ripple = None;
                    }
                    tui.frame_requester().schedule_frame();
                }
                _ = status_spinner.tick() => {
                    if shell.status_spinner_active() {
                        shell.status_spinner_frame = shell.status_spinner_frame.wrapping_add(1);
                        tui.frame_requester().schedule_frame();
                    }
                }
                _ = turn_timer_refresh.tick(), if shell.active_turn_elapsed_seconds().is_some() => {
                    tui.frame_requester().schedule_frame();
                }
            }
        };
        Ok(exit_reason)
    }
    .await;

    shell.cancel_shell_command();
    shell.close_agent_log();
    shell.close_tool_output();
    shell.close_diff_view();
    shell.close_account_auth(&mut app_server).await;
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
    full_text: Option<ToolOutputBuffer>,
    tool_status: Option<ToolBlockStatus>,
    item_id: Option<String>,
    rewind_anchor: Option<rewind::RewindAnchor>,
    render_revision: u64,
}

impl TranscriptLine {
    fn new(kind: TranscriptKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            full_text: None,
            tool_status: None,
            item_id: None,
            rewind_anchor: None,
            render_revision: next_transcript_render_revision(),
        }
    }

    fn output(text: impl Into<ToolOutputBuffer>, status: ToolBlockStatus, item_id: String) -> Self {
        let full_text = text.into();
        Self {
            kind: TranscriptKind::Output,
            text: compact_output_for_transcript(full_text.to_string()),
            full_text: Some(full_text),
            tool_status: Some(status),
            item_id: Some(item_id),
            rewind_anchor: None,
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

    fn rewind_anchor(mut self, anchor: rewind::RewindAnchor) -> Self {
        self.rewind_anchor = Some(anchor);
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
            full_text: self.full_text.clone(),
            tool_status: self.tool_status,
            item_id: self.item_id.clone(),
            rewind_anchor: self.rewind_anchor.clone(),
            render_revision: next_transcript_render_revision(),
        }
    }
}

impl PartialEq for TranscriptLine {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.text == other.text
            && self.full_text == other.full_text
            && self.tool_status == other.tool_status
            && self.item_id == other.item_id
            && self.rewind_anchor == other.rewind_anchor
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalSlashCommandOutcome {
    Continue,
    Exit,
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

#[derive(Debug, Clone)]
pub(crate) struct ResumeCwdRuntime {
    pub(crate) launch_cwd: std::path::PathBuf,
    pub(crate) explicit_cwd: Option<std::path::PathBuf>,
    pub(crate) uses_remote_workspace_or_environment: bool,
}

struct ShellState {
    thread_id: ThreadId,
    session_unavailable_reason: Option<&'static str>,
    thread_name: Option<String>,
    model: String,
    available_models: Vec<ModelPreset>,
    cwd: String,
    approval_policy: codex_app_server_protocol::AskForApproval,
    approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer,
    permission_profile: codex_protocol::models::PermissionProfile,
    active_permission_profile: Option<codex_protocol::models::ActivePermissionProfile>,
    runtime_workspace_roots: Vec<codex_utils_absolute_path::AbsolutePathBuf>,
    reasoning_effort: Option<codex_protocol::openai_models::ReasoningEffort>,
    reasoning_ripple: Option<ReasoningRipple>,
    service_tier: Option<String>,
    collaboration_mode: Option<Box<codex_protocol::config_types::CollaborationMode>>,
    max_concurrent_threads_per_session: usize,
    personality: Option<codex_protocol::config_types::Personality>,
    transcript: VecDeque<TranscriptLine>,
    transcript_scroll: usize,
    transcript_scroll_max: Cell<usize>,
    transcript_selection: Option<usize>,
    text_selection: TextSelectionState,
    transcript_render_cache: RefCell<TranscriptRenderCache>,
    session_list: SessionListState,
    settings: SettingsState,
    mcp_inventory: McpInventorySummary,
    mcp_catalog: Option<ListMcpServerStatusResponse>,
    plugin_inventory: PluginInventorySummary,
    plugin_catalog: Option<PluginListResponse>,
    tui_theme: Option<String>,
    app_theme: TuiAppTheme,
    animations: bool,
    show_tooltips: bool,
    command_palette: Option<CommandPaletteState>,
    selector: Option<SelectorState<SelectorValue>>,
    pending_account_auth: Option<AccountAuthState>,
    codex_home: std::path::PathBuf,
    client_config_path: AbsolutePathBuf,
    resume_cwd_runtime: ResumeCwdRuntime,
    dashboard_route: DashboardRoute,
    dashboard_visible: bool,
    dashboard_resize: dashboard_resize::DashboardResizeState,
    dashboard_scroll: Cell<usize>,
    pointer_position: Option<ratatui::layout::Position>,
    agents_focused: bool,
    composer: ComposerState,
    slash_command_popup: SlashCommandPopupState,
    rewind: rewind::RewindState,
    workspace_command_runner: Option<WorkspaceCommandRunner>,
    pending_shell_command: Option<PendingShellCommand>,
    session_hydration: SessionHydrationState,
    exit_confirmation_pending: bool,
    clipboard_lease: Option<ClipboardLease>,
    active_turn_id: Option<String>,
    turn_started_at: Option<std::time::Instant>,
    pending_approval: Option<PendingApproval>,
    pending_session_delete: Option<PendingSessionDelete>,
    pending_elicitation: Option<PendingElicitation>,
    queued_interactive_requests: VecDeque<PendingInteractiveRequest>,
    pending_external_agent_import: Option<ExternalAgentImportState>,
    pending_mcp_management: Option<McpManagementState>,
    pending_plugin_management: Option<PluginManagementState>,
    pending_prompt_submission: Option<String>,
    pending_user_input: Option<PendingUserInput>,
    pending_vim_input: Option<vim_input::VimInputRequest>,
    safety_buffering: SafetyBufferingState,
    streaming_assistant: String,
    streaming_assistant_item_id: Option<String>,
    streaming_assistant_revision: u64,
    streaming_plan: String,
    streaming_plan_item_id: Option<String>,
    streaming_plan_revision: u64,
    plan_explanation: Option<String>,
    plan_steps: Vec<TurnPlanStep>,
    active_goal: Option<ThreadGoal>,
    tool_activity: VecDeque<ToolActivity>,
    agent_activity: AgentActivityState,
    agent_log: Option<AgentLogState>,
    tool_output: Option<ToolOutputState>,
    diff_store: DiffStore,
    diff_view: Option<DiffViewState>,
    agent_history_task: Option<AgentHistoryTask>,
    active_agent_thread_ids: HashSet<String>,
    deferred_unsubscribe_thread_ids: Vec<ThreadId>,
    subscription_cleanup_task: Option<JoinHandle<()>>,
    backend_actions: backend_actions::BackendActions,
    subagent_activity: VecDeque<ToolActivity>,
    latest_diff: Option<DiffSummary>,
    workspace_git_status: Option<WorkspaceGitStatus>,
    workspace_status_refresh_due: bool,
    rate_limits: Vec<RateLimitSnapshot>,
    rate_limit_reset_credits: Option<i64>,
    status: String,
    status_spinner_frame: usize,
    token_usage: TokenUsage,
    context_token_usage: TokenUsage,
    model_context_window: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletedItemOrigin {
    Historical,
    UnconfirmedHistorical,
    Live,
}

struct ShellClientConfig {
    codex_home: std::path::PathBuf,
    config_path: AbsolutePathBuf,
    app_theme: TuiAppTheme,
    tui_theme: Option<String>,
    animations: bool,
    show_tooltips: bool,
}

impl ShellState {
    fn new(
        session: ThreadSessionState,
        fallback_model: String,
        available_models: Vec<ModelPreset>,
        client_config: ShellClientConfig,
        resume_cwd_runtime: ResumeCwdRuntime,
        max_concurrent_threads_per_session: usize,
    ) -> Self {
        let ShellClientConfig {
            codex_home,
            config_path: client_config_path,
            app_theme,
            tui_theme,
            animations,
            show_tooltips,
        } = client_config;
        let model = if session.model.is_empty() {
            fallback_model
        } else {
            session.model.clone()
        };
        let mut shell = Self {
            thread_id: session.thread_id,
            session_unavailable_reason: None,
            thread_name: session.thread_name,
            model,
            available_models,
            cwd: session.cwd.to_string_lossy().to_string(),
            approval_policy: session.approval_policy,
            approvals_reviewer: session.approvals_reviewer,
            permission_profile: session.permission_profile,
            active_permission_profile: session.active_permission_profile,
            runtime_workspace_roots: session.runtime_workspace_roots,
            reasoning_effort: session.reasoning_effort,
            reasoning_ripple: None,
            service_tier: session.service_tier,
            collaboration_mode: session.collaboration_mode,
            max_concurrent_threads_per_session,
            personality: session.personality,
            transcript: VecDeque::new(),
            transcript_scroll: 0,
            transcript_scroll_max: Cell::new(0),
            transcript_selection: None,
            text_selection: TextSelectionState::default(),
            transcript_render_cache: RefCell::new(TranscriptRenderCache::default()),
            session_list: SessionListState::default(),
            settings: SettingsState::default(),
            mcp_inventory: McpInventorySummary::default(),
            mcp_catalog: None,
            plugin_inventory: PluginInventorySummary::default(),
            plugin_catalog: None,
            tui_theme,
            app_theme,
            animations,
            show_tooltips,
            command_palette: None,
            selector: None,
            pending_account_auth: None,
            codex_home,
            client_config_path,
            resume_cwd_runtime,
            dashboard_route: DashboardRoute::Status,
            dashboard_visible: true,
            dashboard_resize: dashboard_resize::DashboardResizeState::default(),
            dashboard_scroll: Cell::new(0),
            pointer_position: None,
            agents_focused: false,
            composer: ComposerState::default(),
            slash_command_popup: SlashCommandPopupState::default(),
            rewind: rewind::RewindState::default(),
            workspace_command_runner: None,
            pending_shell_command: None,
            session_hydration: SessionHydrationState::default(),
            exit_confirmation_pending: false,
            clipboard_lease: None,
            active_turn_id: None,
            turn_started_at: None,
            pending_approval: None,
            pending_session_delete: None,
            pending_elicitation: None,
            queued_interactive_requests: VecDeque::new(),
            pending_external_agent_import: None,
            pending_mcp_management: None,
            pending_plugin_management: None,
            pending_prompt_submission: None,
            pending_user_input: None,
            pending_vim_input: None,
            safety_buffering: SafetyBufferingState::default(),
            streaming_assistant: String::new(),
            streaming_assistant_item_id: None,
            streaming_assistant_revision: next_transcript_render_revision(),
            streaming_plan: String::new(),
            streaming_plan_item_id: None,
            streaming_plan_revision: next_transcript_render_revision(),
            plan_explanation: None,
            plan_steps: Vec::new(),
            active_goal: None,
            tool_activity: VecDeque::new(),
            agent_activity: AgentActivityState::for_root(session.thread_id.to_string()),
            agent_log: None,
            tool_output: None,
            diff_store: DiffStore::with_display_root(session.cwd.as_path()),
            diff_view: None,
            agent_history_task: None,
            active_agent_thread_ids: HashSet::new(),
            deferred_unsubscribe_thread_ids: Vec::new(),
            subscription_cleanup_task: None,
            backend_actions: backend_actions::BackendActions::default(),
            subagent_activity: VecDeque::new(),
            latest_diff: None,
            workspace_git_status: None,
            workspace_status_refresh_due: false,
            rate_limits: Vec::new(),
            rate_limit_reset_credits: None,
            status: "ready".to_string(),
            status_spinner_frame: 0,
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
            let terminal_turn_has_in_progress_file_change = turn.status != TurnStatus::InProgress
                && turn.items.iter().any(|item| {
                    matches!(
                        item,
                        ThreadItem::FileChange {
                            status: PatchApplyStatus::InProgress,
                            ..
                        }
                    )
                });
            if turn.items_view != TurnItemsView::Full || terminal_turn_has_in_progress_file_change {
                self.diff_store.mark_history_truncated();
            }
            let turn_id = turn.id;
            for (item_index, item) in turn.items.into_iter().enumerate() {
                let rewind_anchor = if item_index == 0 {
                    rewind::RewindAnchor::for_opening_item(&turn_id, &item)
                } else {
                    None
                };
                let origin = if turn.status != TurnStatus::InProgress
                    && matches!(
                        &item,
                        ThreadItem::FileChange {
                            status: PatchApplyStatus::InProgress,
                            ..
                        }
                    ) {
                    CompletedItemOrigin::UnconfirmedHistorical
                } else {
                    CompletedItemOrigin::Historical
                };
                self.ingest_completed_item_for_turn(&turn_id, item, origin, rewind_anchor);
            }
            if let Some(error) = turn.error {
                self.push_error(error.message);
            }
            self.push_turn_separator();
        }
    }

    async fn refresh_workspace_status(
        &mut self,
        runner: &dyn crate::workspace_command::WorkspaceCommandExecutor,
    ) {
        let status = workspace::load_git_status(runner, std::path::Path::new(&self.cwd)).await;
        self.record_workspace_git_probe(status);
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
        self.request_rate_limits_refresh();
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

    fn open_command_palette(&mut self) {
        self.close_agent_log();
        self.close_tool_output();
        self.close_diff_view();
        self.selector = None;
        self.command_palette = Some(CommandPaletteState::default());
        self.clear_transcript_selection();
        self.clear_text_selections();
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
        self.dashboard_scroll.set(0);
    }

    fn toggle_dashboard(&mut self) {
        self.dashboard_visible = !self.dashboard_visible;
        if !self.dashboard_visible {
            self.cancel_dashboard_resize();
            self.session_list.focused = false;
            self.settings.focused = false;
            self.agents_focused = false;
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
                if !self.open_selected_diff_view() && !self.open_selected_tool_output() {
                    self.copy_selected_transcript_with(crate::clipboard_copy::copy_to_clipboard);
                }
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
            KeyCode::Char('e') if is_unmodified_key_press(key) => {
                self.begin_rewind_edit();
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

    fn clear_visible_transcript(&mut self) {
        self.clear_text_selections();
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
        config: &Config,
        app_server: &mut S,
    ) -> Result<LocalSlashCommandOutcome>
    where
        S: AppShellBackend,
    {
        self.composer.remember_submission(&prompt);
        self.composer.clear();
        self.slash_command_popup.reset();
        let account_change_blocked = self.active_turn_id.is_some()
            || self.has_pending_shell_command()
            || self.has_pending_backend_actions()
            || self.composer.has_queued_messages();
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
            LocalSlashCommand::Login => {
                if account_change_blocked {
                    self.push_status("finish active work before logging in");
                } else {
                    self.open_account_auth(config.forced_login_method);
                }
                LocalSlashCommandOutcome::Continue
            }
            LocalSlashCommand::Logout => {
                if account_change_blocked {
                    self.push_status("finish active work before logging out");
                    LocalSlashCommandOutcome::Continue
                } else {
                    match app_server.logout_account().await {
                        Ok(()) => {
                            self.push_status("logged out; run /login to sign in again");
                            LocalSlashCommandOutcome::Continue
                        }
                        Err(err) => {
                            self.push_error(format!("logout failed: {err}"));
                            LocalSlashCommandOutcome::Continue
                        }
                    }
                }
            }
            LocalSlashCommand::Vim => {
                self.request_vim_input();
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
        self.transcript.get(selected).map(|line| {
            (
                line.kind,
                line.full_text.as_deref().unwrap_or(line.text.as_str()),
            )
        })
    }

    fn selected_transcript_is_output(&self) -> bool {
        self.transcript_selection
            .and_then(|selected| self.transcript.get(selected))
            .is_some_and(|line| line.kind == TranscriptKind::Output)
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

    fn submit_prompt<S>(&mut self, app_server: &S, prompt: String)
    where
        S: AppShellBackend,
    {
        self.start_turn(app_server, prompt, TurnSubmission::Interactive);
    }

    fn start_turn<S>(&mut self, app_server: &S, prompt: String, submission: TurnSubmission)
    where
        S: AppShellBackend,
    {
        if self.reject_oversized_input(prompt.len()) {
            return;
        }
        if self.reject_unavailable_session_action() {
            return;
        }
        if self.active_turn_id.is_some() {
            self.push_system("wait for the current turn to finish before sending another message");
            return;
        }

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
        let request = app_server.turn_start_in_background(params.clone());
        if self.start_backend_action(ActionGroup::TurnStart, "thinking", async move {
            BackendActionResult::TurnStart {
                params,
                prompt,
                submission,
                result: request.await,
            }
        }) && submission == TurnSubmission::Interactive
        {
            self.composer.clear();
        }
    }

    fn complete_turn_start<S>(
        &mut self,
        app_server: &S,
        params: AppShellTurnStart,
        prompt: String,
        submission: TurnSubmission,
        result: Result<codex_app_server_protocol::TurnStartResponse>,
    ) where
        S: AppShellBackend,
    {
        let response = match result {
            Ok(response) => response,
            Err(err) => {
                if submission != TurnSubmission::Queued {
                    self.composer.restore_failed_submission(&prompt);
                }
                self.report_action_error("failed to submit turn", err);
                return;
            }
        };
        self.scroll_transcript_to_bottom();
        self.push_line(
            TranscriptLine::new(TranscriptKind::User, prompt.clone()).rewind_anchor(
                rewind::RewindAnchor {
                    before_turn_id: response.turn.id.clone(),
                },
            ),
        );
        self.status = "thinking".to_string();
        self.clear_streaming_transcript();
        match submission {
            TurnSubmission::Queued => {
                self.composer.confirm_next_queued_message(&prompt);
            }
            TurnSubmission::Initial | TurnSubmission::Interactive => {
                self.composer.remember_submission(&prompt);
            }
        }
        self.record_active_turn_started(response.turn.id.clone());
        self.record_safety_buffering_turn(response.turn.id, params);
        if submission == TurnSubmission::Initial {
            self.start_initial_goal_hydration(app_server);
        }
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
        if self.reject_oversized_input(prompt.len()) {
            return Ok(());
        }
        let Some(turn_id) = self.active_turn_id.clone() else {
            self.submit_prompt(app_server, prompt);
            return Ok(());
        };
        let client_user_message_id = format!("better-codex-steer-{}", uuid::Uuid::new_v4());
        app_server
            .turn_steer(AppShellTurnSteer {
                thread_id: self.thread_id,
                turn_id,
                client_user_message_id: client_user_message_id.clone(),
                items: vec![UserInput::Text {
                    text: prompt.clone(),
                    text_elements: Vec::new(),
                }],
            })
            .await
            .wrap_err("failed to steer active turn")?;
        self.scroll_transcript_to_bottom();
        self.push_user_with_client_id(prompt.clone(), client_user_message_id);
        self.composer.remember_submission(&prompt);
        self.composer.clear();
        self.status = "thinking".to_string();
        Ok(())
    }

    fn resolve_pending_approval<S>(
        &mut self,
        app_server: &S,
        option_index: usize,
        edit_prompt: Option<String>,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(pending) = self.pending_approval.as_ref() else {
            return Ok(());
        };
        let request_id = pending.request_id();
        let result = pending.result(option_index)?;
        let request = app_server.resolve_server_request_in_background(request_id.clone(), result);
        self.start_backend_action(ActionGroup::Approval, "resolving approval", async move {
            BackendActionResult::Approval {
                request_id,
                edit_prompt,
                result: request.await,
            }
        });
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
            ApprovalAction::Choose(option_index) => {
                self.resolve_pending_approval(app_server, option_index, None)
            }
            ApprovalAction::Edit => self.edit_pending_approval(app_server),
            ApprovalAction::Explain => {
                self.explain_pending_approval();
                Ok(())
            }
            ApprovalAction::Select(direction) => {
                if let Some(pending) = &mut self.pending_approval {
                    pending.move_selection(direction);
                }
                Ok(())
            }
            ApprovalAction::ScrollUp => {
                if let Some(pending) = &mut self.pending_approval {
                    pending.scroll_up(/*amount*/ 1);
                }
                Ok(())
            }
            ApprovalAction::ScrollDown => {
                if let Some(pending) = &mut self.pending_approval {
                    pending.scroll_down(/*amount*/ 1);
                }
                Ok(())
            }
            ApprovalAction::PageUp => {
                if let Some(pending) = &mut self.pending_approval {
                    pending.scroll_up(/*amount*/ 5);
                }
                Ok(())
            }
            ApprovalAction::PageDown => {
                if let Some(pending) = &mut self.pending_approval {
                    pending.scroll_down(/*amount*/ 5);
                }
                Ok(())
            }
        }
    }

    fn edit_pending_approval<S>(&mut self, app_server: &S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(pending) = self.pending_approval.as_ref() else {
            return Ok(());
        };
        let edit_prompt = pending.edit_prompt().to_string();
        let Some(denial_index) = pending.denial_index() else {
            self.push_error("this approval request cannot be denied for editing");
            return Ok(());
        };
        self.resolve_pending_approval(app_server, denial_index, Some(edit_prompt))
    }

    fn explain_pending_approval(&mut self) {
        let Some(pending) = self.pending_approval.as_ref() else {
            return;
        };
        self.push_decision_audit("approval", "explained", &pending.explanation());
    }

    fn seed_composer_with_edit_prompt(&mut self, edit_prompt: String) {
        self.slash_command_popup.reset();
        let composer_text = self.composer.text().trim();
        if composer_text.is_empty() {
            self.composer.set_text(edit_prompt);
        } else {
            self.composer
                .set_text(format!("{composer_text}\n\n{edit_prompt}"));
        }
    }

    async fn handle_user_input_key<S>(&mut self, key: KeyEvent, app_server: &mut S) -> Result<bool>
    where
        S: AppShellBackend,
    {
        if self.pending_elicitation.is_some()
            && key.modifiers == KeyModifiers::CONTROL
            && key.code == KeyCode::Char('d')
        {
            self.resolve_pending_elicitation(app_server, ElicitationChoice::Decline)
                .await?;
            return Ok(false);
        }
        if self.pending_elicitation.is_some() && key.code == KeyCode::Esc {
            self.resolve_pending_elicitation(app_server, ElicitationChoice::Cancel)
                .await?;
            return Ok(false);
        }
        if let Some(action) = text_input_action_from_key(key) {
            self.composer.apply_text_input_action(action);
            return Ok(false);
        }

        match key.code {
            KeyCode::Esc => Ok(false),
            KeyCode::Enter => {
                if is_composer_newline_key(key) {
                    let result = self.composer.insert_newline();
                    self.report_composer_insert(result);
                    return Ok(false);
                }
                if self.reject_oversized_composer() {
                    return Ok(false);
                }
                if self.pending_elicitation.is_some() {
                    self.resolve_pending_elicitation(app_server, ElicitationChoice::Accept)
                        .await?;
                } else {
                    self.resolve_pending_user_input(app_server).await?;
                }
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
            KeyCode::Char(ch) => {
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    let result = self.composer.insert_char(ch);
                    self.report_composer_insert(result);
                }
                Ok(false)
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let result = self.composer.insert_str("    ");
                self.report_composer_insert(result);
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
            KeyCode::Backspace
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
            | KeyCode::Modifier(_) => Ok(false),
        }
    }

    async fn resolve_pending_user_input<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        if self.has_pending_backend_action(ActionGroup::UserInput) {
            self.push_status("tool input is already being resolved");
            return Ok(());
        }
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
                let completed_request_id = request_id.clone();
                app_server
                    .resolve_server_request(request_id, result)
                    .await
                    .wrap_err("failed to resolve app-server tool input request")?;
                let removal = self.remove_interactive_request(&completed_request_id);
                self.push_decision_audit("tool input", "submitted", &title);
                if removal == InteractiveRequestRemoval::Active {
                    self.activate_next_interactive_request();
                }
            }
            Err(message) => {
                self.push_error(message);
            }
        }
        Ok(())
    }

    fn finish_streaming_assistant(&mut self) {
        if self.streaming_assistant.trim().is_empty() {
            self.clear_streaming_assistant();
            return;
        }
        let message = std::mem::take(&mut self.streaming_assistant);
        let item_id = self.streaming_assistant_item_id.take();
        self.streaming_assistant_revision = next_transcript_render_revision();
        if let Some(item_id) = item_id {
            self.push_assistant_for_item(item_id, message);
        } else {
            self.push_assistant(message);
        }
    }

    fn finish_streaming_plan(&mut self) {
        if self.streaming_plan.trim().is_empty() {
            self.clear_streaming_plan();
            return;
        }
        let plan = std::mem::take(&mut self.streaming_plan);
        let item_id = self.streaming_plan_item_id.take();
        self.streaming_plan_revision = next_transcript_render_revision();
        if let Some(item_id) = item_id {
            self.push_plan_for_item(item_id, plan);
        } else {
            self.push_plan(plan);
        }
    }

    fn push_streaming_assistant_delta(&mut self, item_id: &str, delta: &str) {
        if delta.is_empty() {
            return;
        }
        if self.transcript.iter().rev().any(|item| {
            item.kind == TranscriptKind::Assistant && item.item_id.as_deref() == Some(item_id)
        }) {
            return;
        }
        if self
            .streaming_assistant_item_id
            .as_deref()
            .is_some_and(|streaming_item_id| streaming_item_id != item_id)
        {
            self.clear_streaming_assistant();
        }
        self.streaming_assistant_item_id
            .get_or_insert_with(|| item_id.to_string());
        self.streaming_assistant.push_str(delta);
        self.streaming_assistant_revision = next_transcript_render_revision();
    }

    fn push_streaming_plan_delta(&mut self, item_id: &str, delta: &str) {
        if delta.is_empty() {
            return;
        }
        if self.transcript.iter().rev().any(|item| {
            item.kind == TranscriptKind::Plan && item.item_id.as_deref() == Some(item_id)
        }) {
            return;
        }
        if self
            .streaming_plan_item_id
            .as_deref()
            .is_some_and(|streaming_item_id| streaming_item_id != item_id)
        {
            self.clear_streaming_plan();
        }
        self.streaming_plan_item_id
            .get_or_insert_with(|| item_id.to_string());
        self.streaming_plan.push_str(delta);
        self.streaming_plan_revision = next_transcript_render_revision();
    }

    fn clear_streaming_assistant(&mut self) {
        if self.streaming_assistant.is_empty() && self.streaming_assistant_item_id.is_none() {
            return;
        }
        self.streaming_assistant.clear();
        self.streaming_assistant_item_id = None;
        self.streaming_assistant_revision = next_transcript_render_revision();
    }

    fn clear_streaming_plan(&mut self) {
        if self.streaming_plan.is_empty() && self.streaming_plan_item_id.is_none() {
            return;
        }
        self.streaming_plan.clear();
        self.streaming_plan_item_id = None;
        self.streaming_plan_revision = next_transcript_render_revision();
    }

    fn clear_streaming_transcript(&mut self) {
        self.clear_streaming_assistant();
        self.clear_streaming_plan();
    }

    fn ingest_completed_item(&mut self, item: ThreadItem, origin: CompletedItemOrigin) {
        let turn_id = self
            .active_turn_id
            .clone()
            .unwrap_or_else(|| "unscoped".to_string());
        self.ingest_completed_item_for_turn(&turn_id, item, origin, /*rewind_anchor*/ None);
    }

    fn ingest_completed_item_for_turn(
        &mut self,
        turn_id: &str,
        item: ThreadItem,
        origin: CompletedItemOrigin,
        rewind_anchor: Option<rewind::RewindAnchor>,
    ) {
        self.agent_activity.reduce_completed(&item);
        match item {
            ThreadItem::UserMessage {
                client_id, content, ..
            } => {
                let text = format_user_inputs(&content);
                if !text.is_empty() {
                    let mut line = TranscriptLine::new(TranscriptKind::User, text);
                    if let Some(anchor) = rewind_anchor {
                        line = line.rewind_anchor(anchor);
                    }
                    if let Some(client_id) = client_id {
                        self.upsert_line(line.item_id(client_id));
                    } else {
                        self.push_line(line);
                    }
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
            ThreadItem::AgentMessage { id, text, .. } => {
                if self.streaming_assistant_item_id.as_deref() == Some(id.as_str())
                    || (self.streaming_assistant_item_id.is_none()
                        && self.streaming_assistant == text)
                {
                    self.clear_streaming_assistant();
                }
                if !text.is_empty() {
                    self.push_assistant_for_item(id, text);
                }
            }
            ThreadItem::Plan { id, text, .. } => {
                if self.streaming_plan_item_id.as_deref() == Some(id.as_str())
                    || (self.streaming_plan_item_id.is_none() && self.streaming_plan == text)
                {
                    self.clear_streaming_plan();
                }
                if !text.is_empty() {
                    self.push_plan_for_item(id, text);
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
                let title = command_display::completed_summary(&command, exit_code, duration_ms);
                let tool_status = command_tool_status(&status, exit_code);
                self.upsert_tool_activity(
                    id.clone(),
                    title.clone(),
                    format!("{status:?}").to_lowercase(),
                );
                self.push_tool_with_status_for_item(id.clone(), title, tool_status);
                if let Some(output) = aggregated_output.and_then(nonempty_output_text) {
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
                if origin != CompletedItemOrigin::UnconfirmedHistorical {
                    self.record_file_changes(turn_id, &id, &changes, status.clone());
                }
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
                    CompletedItemOrigin::Historical
                    | CompletedItemOrigin::UnconfirmedHistorical => {}
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
            ThreadItem::FileChange { .. } => {}
            _ => self.upsert_tool_activity(id, title, status.to_string()),
        }
    }

    fn push_system(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::System, text));
    }

    fn push_user(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::User, text));
    }

    fn push_user_with_client_id(
        &mut self,
        text: impl Into<String>,
        client_user_message_id: String,
    ) {
        self.upsert_line(
            TranscriptLine::new(TranscriptKind::User, text).item_id(client_user_message_id),
        );
    }

    fn push_assistant(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::Assistant, text));
    }

    fn push_assistant_for_item(&mut self, item_id: String, text: impl Into<String>) {
        self.upsert_line(TranscriptLine::new(TranscriptKind::Assistant, text).item_id(item_id));
    }

    fn push_plan(&mut self, text: impl Into<String>) {
        self.push_line(TranscriptLine::new(TranscriptKind::Plan, text));
    }

    fn push_plan_for_item(&mut self, item_id: String, text: impl Into<String>) {
        self.upsert_line(TranscriptLine::new(TranscriptKind::Plan, text).item_id(item_id));
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
        let item_id = item_id.into();
        let text = text.into();
        self.update_open_tool_output_title(&item_id, &text, status);
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
        self.push_output_with_status_for_item(next_local_output_item_id(), text.into(), status);
    }

    fn push_turn_separator(&mut self) {
        self.push_line(TranscriptLine::new(TranscriptKind::Separator, ""));
    }

    fn push_output_with_status_for_item(
        &mut self,
        item_id: impl Into<String>,
        text: impl Into<ToolOutputBuffer>,
        status: ToolBlockStatus,
    ) {
        let item_id = item_id.into();
        let text = text.into();
        let existing_full_text = self
            .transcript
            .iter()
            .rev()
            .find(|existing| {
                existing.kind == TranscriptKind::Output
                    && existing.item_id.as_deref() == Some(&item_id)
                    && existing.tool_status == Some(ToolBlockStatus::Running)
            })
            .map(|existing| {
                existing
                    .full_text
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| existing.text.clone().into())
            })
            .or_else(|| {
                self.tool_output
                    .as_ref()
                    .filter(|output| {
                        output.target.item_id == item_id
                            && output.target.status == ToolBlockStatus::Running
                    })
                    .map(|output| output.output_buffer().clone())
            });
        let full_text = if status != ToolBlockStatus::Running {
            existing_full_text
                .filter(|existing| existing.len() > text.len())
                .unwrap_or_else(|| text.clone())
        } else {
            text.clone()
        };
        self.replace_open_tool_output(&item_id, full_text.clone(), status);
        let mut line = TranscriptLine::output(text, status, item_id);
        line.full_text = Some(full_text);
        self.upsert_line(line);
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

        self.append_open_tool_output(&item_id, &delta, status);

        if let Some(existing) = self.transcript.iter_mut().rev().find(|existing| {
            existing.kind == TranscriptKind::Output && existing.item_id.as_deref() == Some(&item_id)
        }) {
            let full_text = existing
                .full_text
                .get_or_insert_with(|| existing.text.clone().into());
            full_text.append(&delta);
            existing.text = compact_output_for_transcript(full_text.to_string());
            existing.tool_status = Some(status);
            existing.mark_render_changed();
            return;
        }

        let output = self
            .tool_output
            .as_ref()
            .filter(|output| output.target.item_id == item_id)
            .map_or_else(|| delta.into(), |output| output.output_buffer().clone());
        self.push_output_with_status_for_item(item_id, output, status);
    }

    fn update_output_status_for_item(&mut self, item_id: &str, status: ToolBlockStatus) {
        self.update_open_tool_output_status(item_id, status);
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
        self.trim_transcript();
    }

    fn trim_transcript(&mut self) {
        if self.transcript.len() > MAX_TRANSCRIPT_LINES {
            self.clear_transcript_text_selection();
        }
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

        if let Some(item_id) = line.item_id.as_deref() {
            let insert_at = match line.kind {
                TranscriptKind::Output => self
                    .transcript
                    .iter()
                    .rposition(|existing| existing.item_id.as_deref() == Some(item_id))
                    .map(|index| index.saturating_add(1)),
                TranscriptKind::Tool => self.transcript.iter().position(|existing| {
                    existing.kind == TranscriptKind::Output
                        && existing.item_id.as_deref() == Some(item_id)
                }),
                TranscriptKind::System
                | TranscriptKind::User
                | TranscriptKind::Assistant
                | TranscriptKind::Plan
                | TranscriptKind::Diff
                | TranscriptKind::Separator
                | TranscriptKind::Status
                | TranscriptKind::Audit
                | TranscriptKind::Error => None,
            };
            if let Some(insert_at) = insert_at {
                self.clear_transcript_text_selection();
                if let Some(selected) = self.transcript_selection
                    && insert_at <= selected
                {
                    self.transcript_selection = Some(selected.saturating_add(1));
                }
                self.transcript.insert(insert_at, line);
                self.trim_transcript();
                return;
            }
        }

        self.push_line(line);
    }

    fn resume_hint(&self) -> Option<String> {
        let thread = self
            .thread_name
            .clone()
            .unwrap_or_else(|| self.thread_id.to_string());
        Some(format!("better-codex resume {thread}"))
    }

    fn dashboard_focused(&self) -> bool {
        self.dashboard_visible
            && (self.session_list.focused || self.settings.focused || self.agents_focused)
    }

    fn status_spinner_active(&self) -> bool {
        self.animations
            && (self.has_pending_backend_actions()
                || self.active_turn_id.is_some()
                    && matches!(
                        self.status.as_str(),
                        "thinking" | "reasoning" | "retrying" | "waiting"
                    ))
    }

    #[cfg(test)]
    fn snapshot_fixture() -> Self {
        let thread_id = ThreadId::from_string("01900000-0000-7000-8000-000000000001")
            .expect("valid snapshot thread id");
        let mut shell = Self {
            thread_id,
            session_unavailable_reason: None,
            thread_name: Some("stage-one".to_string()),
            model: "gpt-5-codex".to_string(),
            available_models: Vec::new(),
            cwd: "/workspace/better-codex".to_string(),
            approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer::User,
            permission_profile: codex_protocol::models::PermissionProfile::default(),
            active_permission_profile: None,
            runtime_workspace_roots: Vec::new(),
            reasoning_effort: None,
            reasoning_ripple: None,
            service_tier: None,
            collaboration_mode: None,
            max_concurrent_threads_per_session: 4,
            personality: None,
            transcript: VecDeque::new(),
            transcript_scroll: 0,
            transcript_scroll_max: Cell::new(0),
            transcript_selection: None,
            text_selection: TextSelectionState::default(),
            transcript_render_cache: RefCell::new(TranscriptRenderCache::default()),
            session_list: SessionListState::default(),
            settings: SettingsState::default(),
            mcp_inventory: McpInventorySummary::default(),
            mcp_catalog: None,
            plugin_inventory: PluginInventorySummary::default(),
            plugin_catalog: None,
            tui_theme: None,
            app_theme: TuiAppTheme::TokyoNight,
            animations: true,
            show_tooltips: true,
            command_palette: None,
            selector: None,
            pending_account_auth: None,
            codex_home: std::path::PathBuf::from("/tmp/codex-home"),
            client_config_path: AbsolutePathBuf::resolve_path_against_base(
                "codex-home/config.toml",
                std::env::temp_dir(),
            ),
            resume_cwd_runtime: ResumeCwdRuntime {
                launch_cwd: std::path::PathBuf::from("/workspace/better-codex"),
                explicit_cwd: None,
                uses_remote_workspace_or_environment: false,
            },
            dashboard_route: DashboardRoute::Sessions,
            dashboard_visible: true,
            dashboard_resize: dashboard_resize::DashboardResizeState::default(),
            dashboard_scroll: Cell::new(0),
            pointer_position: None,
            agents_focused: false,
            composer: {
                let mut composer = ComposerState::default();
                composer.set_text("Summarize the new shell architecture");
                composer
            },
            slash_command_popup: SlashCommandPopupState::default(),
            rewind: rewind::RewindState::default(),
            workspace_command_runner: None,
            pending_shell_command: None,
            session_hydration: SessionHydrationState::default(),
            exit_confirmation_pending: false,
            clipboard_lease: None,
            active_turn_id: None,
            turn_started_at: None,
            pending_approval: None,
            pending_session_delete: None,
            pending_elicitation: None,
            queued_interactive_requests: VecDeque::new(),
            pending_external_agent_import: None,
            pending_mcp_management: None,
            pending_plugin_management: None,
            pending_prompt_submission: None,
            pending_user_input: None,
            pending_vim_input: None,
            safety_buffering: SafetyBufferingState::default(),
            streaming_assistant: "The new shell owns the fullscreen surface.".to_string(),
            streaming_assistant_item_id: None,
            streaming_assistant_revision: next_transcript_render_revision(),
            streaming_plan: String::new(),
            streaming_plan_item_id: None,
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
            tool_activity: VecDeque::from([ToolActivity {
                id: "tool-1".to_string(),
                title: "exec just test -p codex-tui".to_string(),
                status: "in progress".to_string(),
            }]),
            agent_activity: AgentActivityState::for_root(thread_id.to_string()),
            agent_log: None,
            tool_output: None,
            diff_store: DiffStore::with_display_root(std::path::Path::new(
                "/workspace/better-codex",
            )),
            diff_view: None,
            agent_history_task: None,
            active_agent_thread_ids: HashSet::new(),
            deferred_unsubscribe_thread_ids: Vec::new(),
            subscription_cleanup_task: None,
            backend_actions: backend_actions::BackendActions::default(),
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
            status_spinner_frame: 0,
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
            initial_output,
            ToolBlockStatus::Running,
        );
        fixture.next_tool_output_line = TRANSCRIPT_OUTPUT_HIGH_WATER_LINES;
        fixture
    }

    fn bench_fixture() -> ShellState {
        let thread_id = ThreadId::new();
        let mut shell = ShellState {
            thread_id,
            session_unavailable_reason: None,
            thread_name: Some("bench".to_string()),
            model: "gpt-5-codex".to_string(),
            available_models: Vec::new(),
            cwd: "/workspace/better-codex".to_string(),
            approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
            approvals_reviewer: codex_protocol::config_types::ApprovalsReviewer::User,
            permission_profile: codex_protocol::models::PermissionProfile::default(),
            active_permission_profile: None,
            runtime_workspace_roots: Vec::new(),
            reasoning_effort: None,
            reasoning_ripple: None,
            service_tier: None,
            collaboration_mode: None,
            max_concurrent_threads_per_session: 4,
            personality: None,
            transcript: VecDeque::new(),
            transcript_scroll: 0,
            transcript_scroll_max: Cell::new(0),
            transcript_selection: None,
            text_selection: TextSelectionState::default(),
            transcript_render_cache: RefCell::new(TranscriptRenderCache::default()),
            session_list: SessionListState::default(),
            settings: SettingsState::default(),
            mcp_inventory: McpInventorySummary::default(),
            mcp_catalog: None,
            plugin_inventory: PluginInventorySummary::default(),
            plugin_catalog: None,
            tui_theme: None,
            app_theme: TuiAppTheme::TokyoNight,
            animations: true,
            show_tooltips: true,
            command_palette: None,
            selector: None,
            pending_account_auth: None,
            codex_home: std::path::PathBuf::from("/tmp/codex-home"),
            client_config_path: AbsolutePathBuf::resolve_path_against_base(
                "codex-home/config.toml",
                std::env::temp_dir(),
            ),
            resume_cwd_runtime: ResumeCwdRuntime {
                launch_cwd: std::path::PathBuf::from("/workspace/better-codex"),
                explicit_cwd: None,
                uses_remote_workspace_or_environment: false,
            },
            dashboard_route: DashboardRoute::Sessions,
            dashboard_visible: true,
            dashboard_resize: dashboard_resize::DashboardResizeState::default(),
            dashboard_scroll: Cell::new(0),
            pointer_position: None,
            agents_focused: false,
            composer: {
                let mut composer = ComposerState::default();
                composer.set_text("Benchmark the app shell render path");
                composer
            },
            slash_command_popup: SlashCommandPopupState::default(),
            rewind: rewind::RewindState::default(),
            workspace_command_runner: None,
            pending_shell_command: None,
            session_hydration: SessionHydrationState::default(),
            exit_confirmation_pending: false,
            clipboard_lease: None,
            active_turn_id: Some("turn-bench-1234567890".to_string()),
            turn_started_at: Some(std::time::Instant::now()),
            pending_approval: None,
            pending_session_delete: None,
            pending_elicitation: None,
            queued_interactive_requests: VecDeque::new(),
            pending_external_agent_import: None,
            pending_mcp_management: None,
            pending_plugin_management: None,
            pending_prompt_submission: None,
            pending_user_input: None,
            pending_vim_input: None,
            safety_buffering: SafetyBufferingState::default(),
            streaming_assistant: String::new(),
            streaming_assistant_item_id: None,
            streaming_assistant_revision: next_transcript_render_revision(),
            streaming_plan: String::new(),
            streaming_plan_item_id: None,
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
            agent_activity: AgentActivityState::for_root(thread_id.to_string()),
            agent_log: None,
            tool_output: None,
            diff_store: DiffStore::with_display_root(std::path::Path::new(
                "/workspace/better-codex",
            )),
            diff_view: None,
            agent_history_task: None,
            active_agent_thread_ids: HashSet::new(),
            deferred_unsubscribe_thread_ids: Vec::new(),
            subscription_cleanup_task: None,
            backend_actions: backend_actions::BackendActions::default(),
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
            status_spinner_frame: 0,
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
            UserInput::Audio { .. } => "[audio]".to_string(),
            UserInput::LocalAudio { path } => format!("[audio {}]", path.display()),
            UserInput::Skill { name, path } => format!("[skill {name} {}]", path.display()),
            UserInput::Mention { name, path } => format!("[mention {name} {path}]"),
        })
        .collect::<Vec<_>>()
        .join("\n")
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
        let path = diff_path::bounded_visible_path(&change.path);
        let line = match &change.kind {
            PatchChangeKind::Add => format!("  A {path}"),
            PatchChangeKind::Delete => format!("  D {path}"),
            PatchChangeKind::Update { move_path: None } => format!("  M {path}"),
            PatchChangeKind::Update {
                move_path: Some(move_path),
            } => {
                let move_path = move_path.to_string_lossy();
                let move_path = diff_path::bounded_visible_path(&move_path);
                format!("  R {path} -> {move_path}")
            }
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
    changes.iter().fold(
        DiffSummary {
            files: changes.len(),
            ..DiffSummary::default()
        },
        |mut summary, change| {
            match &change.kind {
                PatchChangeKind::Add => summary.additions += change.diff.lines().count(),
                PatchChangeKind::Delete => summary.removals += change.diff.lines().count(),
                PatchChangeKind::Update { .. } => {
                    let mut in_hunk = false;
                    for line in change.diff.lines() {
                        if diff_model::is_hunk_header(line) {
                            in_hunk = true;
                        } else if in_hunk && line.starts_with('+') {
                            summary.additions += 1;
                        } else if in_hunk && line.starts_with('-') {
                            summary.removals += 1;
                        }
                    }
                }
            }
            summary
        },
    )
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

fn nonempty_output_text(text: String) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
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
        return Some(DashboardRoute::Status);
    }
    if key_hint::ctrl(KeyCode::Char('2')).is_press(key) {
        return Some(DashboardRoute::Agents);
    }
    if key_hint::ctrl(KeyCode::Char(' ')).is_press(key) {
        return Some(DashboardRoute::Agents);
    }
    if key_hint::ctrl(KeyCode::Char('3')).is_press(key) {
        return Some(DashboardRoute::Sessions);
    }
    if key_hint::ctrl(KeyCode::Char('4')).is_press(key) {
        return Some(DashboardRoute::Help);
    }

    match key {
        KeyEvent {
            code: KeyCode::Char('\u{0000}') | KeyCode::Null,
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers.contains(KeyModifiers::CONTROL) => {
            Some(DashboardRoute::Agents)
        }
        KeyEvent {
            code: KeyCode::Char('\u{001b}'),
            modifiers,
            ..
        } if modifiers.is_empty() => Some(DashboardRoute::Sessions),
        KeyEvent {
            code: KeyCode::Esc,
            modifiers,
            ..
        } if modifiers.contains(KeyModifiers::CONTROL) => Some(DashboardRoute::Sessions),
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

fn is_composer_newline_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
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

fn approval_action_from_key(pending: &PendingApproval, key: KeyEvent) -> Option<ApprovalAction> {
    if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
        return None;
    }
    match key.code {
        KeyCode::Enter => Some(ApprovalAction::Choose(pending.selected_option())),
        KeyCode::Char('a' | 'A' | 'y' | 'Y') => Some(ApprovalAction::Choose(0)),
        KeyCode::Esc | KeyCode::Char('d' | 'D' | 'n' | 'N') => {
            pending.denial_index().map(ApprovalAction::Choose)
        }
        KeyCode::Char('e') | KeyCode::Char('E') => Some(ApprovalAction::Edit),
        KeyCode::Char('?') => Some(ApprovalAction::Explain),
        KeyCode::Up => Some(ApprovalAction::Select(ApprovalSelectionDirection::Previous)),
        KeyCode::Down => Some(ApprovalAction::Select(ApprovalSelectionDirection::Next)),
        KeyCode::Char('k' | 'K') => Some(ApprovalAction::ScrollUp),
        KeyCode::Char('j' | 'J') => Some(ApprovalAction::ScrollDown),
        KeyCode::PageUp => Some(ApprovalAction::PageUp),
        KeyCode::PageDown => Some(ApprovalAction::PageDown),
        KeyCode::Char(ch) => ch
            .to_digit(10)
            .and_then(|digit| usize::try_from(digit).ok())
            .and_then(|index| index.checked_sub(1))
            .filter(|index| *index < pending.option_count())
            .map(ApprovalAction::Choose),
        _ => None,
    }
}

#[cfg(test)]
#[path = "app_shell_tests.rs"]
mod tests;
