use super::*;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;

#[test]
fn message_pane_mouse_wheel_moves_the_composer_cursor_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.composer.set_text(
        (1..=10)
            .map(|line| format!("message line {line:02}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 16,
    );
    let input = (ShellView { shell: &shell }).input_area(area);
    let pointer = Position::new(input.x.saturating_add(1), input.y.saturating_add(1));

    for _ in 0..3 {
        shell.handle_mouse_scroll(area, pointer, MouseScrollDirection::Up);
    }

    assert_eq!(shell.composer.cursor_position(), (6, 15));
    insta::assert_snapshot!(render_shell(&shell, area));

    shell.handle_mouse_scroll(area, pointer, MouseScrollDirection::Down);
    assert_eq!(shell.composer.cursor_position(), (7, 15));
}

fn render_shell(shell: &ShellState, area: Rect) -> String {
    let mut buf = Buffer::empty(area);
    ShellView { shell }.render(area, &mut buf);
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .filter_map(|x| buf.cell((x, y)))
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
