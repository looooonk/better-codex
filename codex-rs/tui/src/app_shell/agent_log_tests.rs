use super::*;
use crate::app_shell::ShellState;
use crate::app_shell::agent_log_view::render_agent_log;
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

fn ready_log() -> AgentLogState {
    AgentLogState {
        target: AgentLogTarget {
            thread_id: "01900000-0000-7000-8000-000000000099".to_string(),
            display_name: "reviewer".to_string(),
            path: "/root/reviewer".to_string(),
            task_summary: Some("Review the restored session and verify the TUI state.".to_string()),
            status: AgentLifecycleStatus::Shutdown,
        },
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
