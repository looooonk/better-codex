use super::render::ShellView;
use super::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn renders_first_stage_shell_snapshot() {
    let shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let mut buf = Buffer::empty(area);

    ShellView { shell: &shell }.render(area, &mut buf);

    insta::assert_snapshot!(buffer_contents(&buf, area));
}

fn buffer_contents(buf: &Buffer, area: Rect) -> String {
    let mut rows = Vec::new();
    for y in area.y..area.bottom() {
        let mut row = String::new();
        for x in area.x..area.right() {
            row.push_str(buf.cell((x, y)).expect("cell should exist").symbol());
        }
        rows.push(row.trim_end().to_string());
    }
    rows.join("\n")
}
