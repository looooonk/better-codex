use super::*;
use crate::app_shell::ShellState;
use crate::app_shell::agent_log_view::render_agent_log;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadSource;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_config::types::TuiAppTheme;
use codex_utils_absolute_path::AbsolutePathBuf;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

#[test]
fn renders_wide_and_compact_agent_log_popups() {
    let log = ready_log();

    insta::assert_snapshot!("wide_agent_log_popup", render_log(&log, 100, 30));
    insta::assert_snapshot!("compact_agent_log_popup", render_log(&log, 54, 16));
}

#[test]
fn navigation_scrolls_to_edges_and_escape_closes() {
    let mut shell = ShellState::snapshot_fixture();
    shell.agent_log = Some(ready_log());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 18,
    );
    let mut buffer = Buffer::empty(area);
    render_agent_log(
        shell.agent_log.as_ref().expect("log should be open"),
        area,
        &mut buffer,
    );

    assert!(shell.handle_agent_log_key(key(KeyCode::End)));
    assert!(
        shell
            .agent_log
            .as_ref()
            .expect("log should remain open")
            .scroll()
            > 0
    );
    assert!(shell.handle_agent_log_key(key(KeyCode::Home)));
    assert_eq!(
        shell
            .agent_log
            .as_ref()
            .expect("log should remain open")
            .scroll(),
        0
    );
    assert!(shell.handle_agent_log_key(key(KeyCode::Esc)));
    assert!(shell.agent_log.is_none());
}

#[tokio::test]
async fn loaded_log_is_formatted_with_the_selected_app_theme() {
    let load_task = tokio::spawn(async { Ok(themed_thread()) });
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while !load_task.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("agent log load task should finish");
    let mut log = AgentLogState::loading(target(), load_task, RawReasoningVisibility::Hidden);
    let expected_muted = {
        let _theme = crate::app_theme::activate(TuiAppTheme::GruvboxDark);
        crate::app_theme::palette().muted
    };
    let expected_purple = {
        let _theme = crate::app_theme::activate(TuiAppTheme::GruvboxDark);
        crate::app_theme::palette().purple
    };
    let expected_cyan = {
        let _theme = crate::app_theme::activate(TuiAppTheme::GruvboxDark);
        crate::app_theme::palette().cyan
    };

    assert!(log.poll(TuiAppTheme::GruvboxDark).await);
    let assistant = log
        .lines
        .iter()
        .flat_map(|line| &line.spans)
        .find(|span| span.content == "Assistant")
        .expect("agent message header should render");
    let inline_code = log
        .lines
        .iter()
        .flat_map(|line| &line.spans)
        .find(|span| span.content == "theme")
        .expect("inline code should render");

    assert_eq!(assistant.style.fg, Some(expected_purple));
    assert_eq!(inline_code.style.fg, Some(expected_cyan));
    assert_eq!(
        log.lines[0]
            .spans
            .last()
            .expect("turn duration should render")
            .style
            .fg,
        Some(expected_muted)
    );
}

fn ready_log() -> AgentLogState {
    AgentLogState {
        target: target(),
        load_task: None,
        lines: (0..30)
            .map(|index| {
                if index % 5 == 0 {
                    Line::from(format!("Assistant checkpoint {index}"))
                } else {
                    Line::from(format!(
                        "log line {index}: inspected lifecycle events and transcript output"
                    ))
                }
            })
            .collect(),
        error: None,
        raw_reasoning_visibility: RawReasoningVisibility::Hidden,
        wrapped_cache: RefCell::new(None),
        scroll: Cell::new(0),
        scroll_max: Cell::new(0),
    }
}

fn target() -> AgentLogTarget {
    AgentLogTarget {
        thread_id: "01900000-0000-7000-8000-000000000099".to_string(),
        display_name: "reviewer".to_string(),
        path: "/root/reviewer".to_string(),
        task_summary: Some("Review the restored session and verify the TUI state.".to_string()),
        status: AgentLifecycleStatus::Shutdown,
    }
}

fn themed_thread() -> Thread {
    Thread {
        id: "01900000-0000-7000-8000-000000000099".to_string(),
        extra: None,
        session_id: "01900000-0000-7000-8000-000000000099".to_string(),
        forked_from_id: None,
        parent_thread_id: None,
        preview: String::new(),
        ephemeral: false,
        history_mode: ThreadHistoryMode::Legacy,
        model_provider: "openai".to_string(),
        created_at: 1,
        updated_at: 1,
        recency_at: Some(1),
        status: ThreadStatus::NotLoaded,
        path: None,
        cwd: AbsolutePathBuf::from_absolute_path_checked("/workspace")
            .expect("absolute workspace path"),
        cli_version: "test".to_string(),
        source: SessionSource::Exec,
        thread_source: Some(ThreadSource::Subagent),
        agent_nickname: None,
        agent_role: None,
        git_info: None,
        name: None,
        turns: vec![Turn {
            id: "turn-1".to_string(),
            items: vec![ThreadItem::AgentMessage {
                id: "message-1".to_string(),
                text: "Used `theme` to style the response.".to_string(),
                phase: None,
                memory_citation: None,
            }],
            items_view: TurnItemsView::Full,
            status: TurnStatus::Completed,
            error: None,
            started_at: Some(1),
            completed_at: Some(2),
            duration_ms: Some(1_000),
        }],
    }
}

fn render_log(log: &AgentLogState, width: u16, height: u16) -> String {
    let area = Rect::new(/*x*/ 0, /*y*/ 0, width, height);
    let mut buffer = Buffer::empty(area);
    render_agent_log(log, area, &mut buffer);
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .filter_map(|x| buffer.cell((x, y)))
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
