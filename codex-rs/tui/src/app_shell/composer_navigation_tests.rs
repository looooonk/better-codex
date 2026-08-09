use super::super::render::ShellView;
use super::*;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;

#[test]
fn soft_wrapped_arrow_navigation_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    let text = "You should do items A, B, plus items C and D, and C.";
    shell.composer.set_text(text);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 50, /*height*/ 16,
    );
    let layout = ComposerNavigationLayout::Area(area);

    shell.move_composer_up(layout);

    assert_eq!(shell.composer.cursor(), 6);
    insta::assert_snapshot!(render_shell_with_visible_cursor(&shell, area));

    shell.move_composer_down(layout);

    assert_eq!(shell.composer.cursor(), text.len());
}

#[test]
fn vertical_navigation_maps_expanded_tabs_back_to_source_offsets() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    let text = "a\tfirst\nfour";
    shell.composer.set_text(text);
    let layout = ComposerNavigationLayout::Area(Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 50, /*height*/ 16,
    ));

    shell.move_composer_up(layout);
    assert_eq!(shell.composer.cursor(), "a".len());
}

#[test]
fn up_at_the_first_visual_row_preserves_history_recall() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.composer.remember_submission("remembered request");
    shell.composer.set_text("draft");
    shell.composer.set_cursor(/*cursor*/ 0);
    let layout = ComposerNavigationLayout::Area(Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 50, /*height*/ 16,
    ));

    shell.move_composer_up(layout);

    assert_eq!(shell.composer.text(), "remembered request");
}

fn render_shell_with_visible_cursor(shell: &ShellState, area: Rect) -> String {
    let mut buf = Buffer::empty(area);
    let view = ShellView { shell };
    view.render(area, &mut buf);
    if let Some(position) = view.cursor_position(area) {
        buf[position].set_symbol("▏");
    }
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
