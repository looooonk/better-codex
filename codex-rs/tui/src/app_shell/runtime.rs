//! Fullscreen lifecycle and event-loop orchestration for the app shell.

use super::BACKEND_ACTION_POLL_INTERVAL;
use super::LocalSlashCommandOutcome;
use super::ResumeCwdRuntime;
use super::ShellClientConfig;
use super::ShellState;
use super::backend::AppShellBackend;
use super::backend::shutdown_app_shell_backend;
use super::backend_actions::TurnSubmission;
use super::local_app_theme;
use super::reasoning_ripple;
use super::render::draw_shell;
use super::shell_layout::terminal_size_supported;
use super::startup_availability_nux;
use super::startup_model_migration;
use super::vim_input;
use crate::app_exit::AppExitInfo;
use crate::app_exit::ExitReason;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::AppServerStartedThread;
use crate::legacy_core::config::Config;
use crate::resume_picker::SessionSelection;
use crate::token_usage::TokenUsage;
use crate::tui;
use crate::tui::TuiEvent;
use crate::workspace_command::AppServerWorkspaceCommandRunner;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use std::sync::Arc;
use std::time::Duration;
use tokio::select;
use tokio_stream::StreamExt;

const APP_SERVER_FRAME_INTERVAL: Duration = Duration::from_millis(33);
const AGENT_HISTORY_POLL_INTERVAL: Duration = Duration::from_millis(50);
const RATE_LIMITS_REFRESH_INTERVAL: Duration = Duration::from_secs(/*secs*/ 60);
const WORKSPACE_STATUS_POLL_INTERVAL: Duration = Duration::from_secs(/*secs*/ 5);
const STATUS_SPINNER_FRAME_INTERVAL: Duration = Duration::from_millis(120);
const TURN_TIMER_REFRESH_INTERVAL: Duration = Duration::from_secs(/*secs*/ 1);

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
        thread_status,
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
    shell.restore_thread_lifecycle(thread_status, &turns);
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
    shell.request_queue_hydration(&app_server);
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
            let terminal_size = tui.terminal.size()?;
            if terminal_size_supported(terminal_size.width, terminal_size.height)
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
                    let accepts_interaction = terminal_size_supported(size.width, size.height);
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
                            match shell
                                .handle_key_in_area(area, key, &config, &mut app_server)
                                .await
                            {
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
                    shell.request_thread_usage_refresh();
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

pub(super) async fn start_selected_session<S>(
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
