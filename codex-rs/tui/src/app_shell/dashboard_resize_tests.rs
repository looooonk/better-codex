use super::*;
use crate::app_shell::render::ShellView;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;

#[test]
fn dragging_the_divider_resizes_and_clamps_the_sidebar() {
    let mut shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 120, /*height*/ 28,
    );
    let divider = shell_layout::calculate(&shell, area)
        .and_then(|layout| layout.dashboard)
        .expect("dashboard should be visible")
        .area()
        .x;

    assert!(shell.begin_dashboard_resize(area, Position::new(divider, 10)));
    assert!(shell.update_dashboard_resize(area, Position::new(52, 10)));
    assert!(shell.finish_dashboard_resize(area, Position::new(48, 10)));
    assert_eq!(
        shell.dashboard_resize,
        DashboardResizeState {
            preferred_width: Some(72),
            dragging: false,
        }
    );

    assert!(shell.begin_dashboard_resize(area, Position::new(48, 10)));
    assert!(shell.update_dashboard_resize(area, Position::new(100, 10)));
    assert!(shell.finish_dashboard_resize(area, Position::new(100, 10)));
    assert_eq!(
        (
            shell.dashboard_resize,
            shell_layout::calculate(&shell, area),
        ),
        (
            DashboardResizeState {
                preferred_width: Some(32),
                dragging: false,
            },
            Some(shell_layout::ShellLayout {
                header: Rect::new(
                    /*x*/ 0, /*y*/ 0, /*width*/ 88, /*height*/ 3,
                ),
                transcript: Rect::new(
                    /*x*/ 0, /*y*/ 3, /*width*/ 88, /*height*/ 19,
                ),
                input: Rect::new(
                    /*x*/ 0, /*y*/ 22, /*width*/ 88, /*height*/ 6,
                ),
                dashboard: Some(shell_layout::DashboardPlacement::Sidebar(Rect::new(
                    /*x*/ 88, /*y*/ 0, /*width*/ 32, /*height*/ 28,
                ))),
            }),
        )
    );
}

#[test]
fn keyboard_resize_moves_the_divider_in_both_directions() {
    let mut shell = ShellState::snapshot_fixture();
    shell.session_list.focused = true;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 120, /*height*/ 28,
    );
    let modifiers = KeyModifiers::SHIFT | KeyModifiers::ALT;

    assert!(shell.handle_dashboard_resize_key(area, KeyEvent::new(KeyCode::Left, modifiers),));
    assert_eq!(
        shell.dashboard_resize,
        DashboardResizeState {
            preferred_width: Some(54),
            dragging: false,
        }
    );
    assert!(shell.handle_dashboard_resize_key(area, KeyEvent::new(KeyCode::Right, modifiers),));
    assert_eq!(
        shell.dashboard_resize,
        DashboardResizeState {
            preferred_width: Some(50),
            dragging: false,
        }
    );
}

#[test]
fn dragging_the_overlay_border_resizes_without_hiding_the_input() {
    let mut shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24,
    );
    let divider = shell_layout::calculate(&shell, area)
        .and_then(|layout| layout.dashboard)
        .expect("dashboard should be visible")
        .area()
        .x;

    assert!(shell.begin_dashboard_resize(area, Position::new(divider, 10)));
    assert!(shell.finish_dashboard_resize(area, Position::new(60, 10)));
    assert_eq!(
        shell_layout::calculate(&shell, area),
        Some(shell_layout::ShellLayout {
            header: Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 3,
            ),
            transcript: Rect::new(
                /*x*/ 0, /*y*/ 3, /*width*/ 78, /*height*/ 15,
            ),
            input: Rect::new(
                /*x*/ 0, /*y*/ 18, /*width*/ 78, /*height*/ 6,
            ),
            dashboard: Some(shell_layout::DashboardPlacement::Overlay(Rect::new(
                /*x*/ 46, /*y*/ 3, /*width*/ 32, /*height*/ 15,
            ))),
        })
    );
}

#[test]
fn resized_dashboard_divider_hover_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 120, /*height*/ 28,
    );
    shell.dashboard_resize.preferred_width = Some(58);
    let divider = shell_layout::calculate(&shell, area)
        .and_then(|layout| layout.dashboard)
        .expect("dashboard should be visible")
        .area()
        .x;
    shell.pointer_position = Some(Position::new(divider, 10));

    insta::assert_snapshot!(render_shell(&shell, area));
}

fn render_shell(shell: &ShellState, area: Rect) -> String {
    let mut buffer = Buffer::empty(area);
    ShellView { shell }.render(area, &mut buffer);
    let mut rows = Vec::new();
    for y in area.y..area.bottom() {
        let mut row = String::new();
        for x in area.x..area.right() {
            row.push_str(buffer.cell((x, y)).expect("cell should exist").symbol());
        }
        rows.push(row.trim_end().to_string());
    }
    rows.join("\n")
}
