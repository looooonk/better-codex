use super::*;
use crate::app_shell::ShellState;
use crate::app_shell::agent_log_view::render_agent_log;
use codex_app_server_protocol::FileUpdateChange;
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
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
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
async fn async_load_formats_positioned_styles_with_the_selected_app_theme() {
    let load_task = tokio::spawn(async { Ok(themed_thread()) });
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while !load_task.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("agent log load task should finish");
    let mut log = AgentLogState::loading(target(), load_task, RawReasoningVisibility::Visible);
    let initial_palette = crate::app_theme::palette();

    assert!(log.poll(TuiAppTheme::CatppuccinMocha).await);

    assert_eq!(
        positioned_styles(log.lines()),
        vec![
            PositionedLine::new(
                /*line*/ 0,
                vec![
                    StyledSpan::new(
                        /*span*/ 0,
                        "Turn 1",
                        Style::new()
                            .fg(Color::Rgb(203, 166, 247))
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    ),
                    StyledSpan::new(/*span*/ 1, "  ", Style::new()),
                    StyledSpan::new(
                        /*span*/ 2,
                        "Completed",
                        Style::new().fg(Color::Rgb(166, 227, 161)),
                    ),
                    StyledSpan::new(/*span*/ 3, "  ", Style::new()),
                    StyledSpan::new(
                        /*span*/ 4,
                        "1.0s",
                        Style::new().fg(Color::Rgb(127, 132, 156)),
                    ),
                ],
            ),
            PositionedLine::new(
                /*line*/ 1,
                vec![StyledSpan::new(
                    /*span*/ 0,
                    "Reasoning",
                    Style::new()
                        .fg(Color::Rgb(127, 132, 156))
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )],
            ),
            PositionedLine::new(
                /*line*/ 2,
                vec![StyledSpan::new(
                    /*span*/ 0,
                    "Reasoning follows the active palette.",
                    Style::new(),
                )],
            ),
            PositionedLine::new(/*line*/ 3, Vec::new()),
            PositionedLine::new(
                /*line*/ 4,
                vec![StyledSpan::new(
                    /*span*/ 0,
                    "Assistant",
                    Style::new()
                        .fg(Color::Rgb(203, 166, 247))
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )],
            ),
            PositionedLine::new(
                /*line*/ 5,
                vec![
                    StyledSpan::new(/*span*/ 0, "Use ", Style::new()),
                    StyledSpan::new(
                        /*span*/ 1,
                        "themed code",
                        Style::new().fg(Color::Rgb(137, 220, 235)),
                    ),
                    StyledSpan::new(/*span*/ 2, " and ", Style::new()),
                    StyledSpan::new(/*span*/ 3, "themed links", Style::new(),),
                    StyledSpan::new(/*span*/ 4, " (", Style::new()),
                    StyledSpan::new(
                        /*span*/ 5,
                        "https://example.com",
                        Style::new()
                            .fg(Color::Rgb(137, 220, 235))
                            .add_modifier(ratatui::style::Modifier::UNDERLINED),
                    ),
                    StyledSpan::new(/*span*/ 6, ")", Style::new()),
                    StyledSpan::new(/*span*/ 7, ".", Style::new()),
                ],
            ),
            PositionedLine::new(/*line*/ 6, Vec::new()),
            PositionedLine::new(
                /*line*/ 7,
                vec![StyledSpan::new(
                    /*span*/ 0,
                    "Plan",
                    Style::new()
                        .fg(Color::Rgb(166, 227, 161))
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )],
            ),
            PositionedLine::new(
                /*line*/ 8,
                vec![StyledSpan::new(
                    /*span*/ 0,
                    "Ship the themed agent log.",
                    Style::new(),
                )],
            ),
            PositionedLine::new(/*line*/ 9, Vec::new()),
            PositionedLine::new(
                /*line*/ 10,
                vec![
                    StyledSpan::new(
                        /*span*/ 0,
                        "Edits",
                        Style::new()
                            .fg(Color::Rgb(203, 166, 247))
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    ),
                    StyledSpan::new(/*span*/ 1, "  ", Style::new()),
                    StyledSpan::new(
                        /*span*/ 2,
                        "Completed",
                        Style::new().fg(Color::Rgb(166, 227, 161)),
                    ),
                    StyledSpan::new(/*span*/ 3, "  ", Style::new()),
                    StyledSpan::new(
                        /*span*/ 4,
                        "1 file",
                        Style::new().fg(Color::Rgb(127, 132, 156)),
                    ),
                ],
            ),
            PositionedLine::new(
                /*line*/ 11,
                vec![
                    StyledSpan::new(/*span*/ 0, "  ", Style::new()),
                    StyledSpan::new(
                        /*span*/ 1,
                        "M",
                        Style::new()
                            .fg(Color::Rgb(249, 226, 175))
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    ),
                    StyledSpan::new(/*span*/ 2, "  ", Style::new()),
                    StyledSpan::new(
                        /*span*/ 3,
                        "tui/src/app_shell/agent_log.rs",
                        Style::new(),
                    ),
                ],
            ),
        ]
    );
    assert_eq!(crate::app_theme::palette(), initial_palette);
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
            items: vec![
                ThreadItem::Reasoning {
                    id: "reasoning-1".to_string(),
                    summary: vec!["Hidden summary".to_string()],
                    content: vec!["Reasoning follows the active palette.".to_string()],
                },
                ThreadItem::AgentMessage {
                    id: "message-1".to_string(),
                    text: "Use `themed code` and [themed links](https://example.com).".to_string(),
                    phase: None,
                    memory_citation: None,
                },
                ThreadItem::Plan {
                    id: "plan-1".to_string(),
                    text: "Ship the themed agent log.".to_string(),
                },
                ThreadItem::FileChange {
                    id: "edit-1".to_string(),
                    changes: vec![FileUpdateChange {
                        path: "tui/src/app_shell/agent_log.rs".to_string(),
                        kind: codex_app_server_protocol::PatchChangeKind::Update {
                            move_path: None,
                        },
                        diff: String::new(),
                    }],
                    status: codex_app_server_protocol::PatchApplyStatus::Completed,
                },
            ],
            items_view: TurnItemsView::Full,
            status: TurnStatus::Completed,
            error: None,
            started_at: Some(1),
            completed_at: Some(2),
            duration_ms: Some(1_000),
        }],
    }
}

#[derive(Debug, PartialEq, Eq)]
struct PositionedLine {
    line: usize,
    style: Style,
    spans: Vec<StyledSpan>,
}

impl PositionedLine {
    fn new(line: usize, spans: Vec<StyledSpan>) -> Self {
        Self {
            line,
            style: Style::new(),
            spans,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct StyledSpan {
    span: usize,
    text: String,
    style: Style,
}

impl StyledSpan {
    fn new(span: usize, text: &str, style: Style) -> Self {
        Self {
            span,
            text: text.to_string(),
            style,
        }
    }
}

fn positioned_styles(lines: &[Line<'static>]) -> Vec<PositionedLine> {
    lines
        .iter()
        .enumerate()
        .map(|(line, value)| PositionedLine {
            line,
            style: value.style,
            spans: value
                .spans
                .iter()
                .enumerate()
                .map(|(span, value)| StyledSpan {
                    span,
                    text: value.content.to_string(),
                    style: value.style,
                })
                .collect(),
        })
        .collect()
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
