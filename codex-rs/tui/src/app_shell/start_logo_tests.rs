use super::*;
use crate::app_shell::render::ShellView;
use crate::app_shell::tests::render_shell;
use pretty_assertions::assert_eq;

#[test]
fn centered_logo_yields_to_the_first_message() {
    let mut shell = ShellState::snapshot_fixture();
    shell.clear_visible_transcript();
    let screen = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 24,
    );
    let visible_logo = |shell: &ShellState| {
        let layout = super::super::shell_layout::calculate(shell, screen).expect("supported size");
        let dashboard = layout.dashboard.expect("dashboard overlay").area();
        render_shell(shell, screen)
            .lines()
            .skip(usize::from(layout.transcript.y))
            .take(usize::from(layout.transcript.height))
            .map(|line| {
                line.chars()
                    .take(usize::from(dashboard.x))
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    insta::assert_snapshot!("empty_conversation_logo", visible_logo(&shell));
    shell.push_user("Hello!");
    let content = visible_logo(&shell);
    assert!(content.contains("Hello!"));
    assert!(!content.contains('⣿'));
}

#[test]
fn compact_logo_shimmers_only_when_effects_are_enabled() {
    let mut shell = ShellState::snapshot_fixture();
    shell.clear_visible_transcript();
    shell.dashboard_visible = false;
    let screen = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 50, /*height*/ 13,
    );
    let logo_cells = |shell: &ShellState| {
        let view = ShellView { shell };
        let mut buf = Buffer::empty(screen);
        view.render(screen, &mut buf);
        view.transcript_area(screen)
            .rows()
            .flat_map(|row| row.columns().map(|cell| (cell.x, cell.y)))
            .filter_map(|pos| {
                let cell = &buf[pos];
                (cell.symbol() != " ").then_some((cell.symbol().to_owned(), cell.fg))
            })
            .collect::<Vec<_>>()
    };
    let static_colors = logo_cells(&shell);
    assert!(static_colors.iter().any(|(symbol, _)| symbol == "⡴"));
    shell.chrome_animation_frame = 8;
    assert_ne!(logo_cells(&shell), static_colors);
    shell.shimmer_enabled = false;
    assert_eq!(logo_cells(&shell), static_colors);
    shell.shimmer_enabled = true;
    shell.animations = false;
    assert_eq!(logo_cells(&shell), static_colors);
}

#[test]
fn logo_fits_short_or_narrow_conversation_panes() {
    let shell = ShellState::snapshot_fixture();
    let mut rows = Vec::new();
    for (width, height) in [(10, 5), (6, 3), (4, 1)] {
        let area = Rect::new(/*x*/ 0, /*y*/ 0, width, height);
        let layout = ShellLayout {
            status: None,
            transcript: area,
            input: Rect::default(),
            dashboard: None,
        };
        let mut buf = Buffer::empty(area);
        render(&shell, layout, &mut buf);
        for row in area.rows() {
            let text: String = row
                .columns()
                .map(|cell| buf[(cell.x, cell.y)].symbol())
                .collect();
            rows.push(text.trim_end().to_string());
        }
        rows.push(String::new());
    }
    insta::assert_snapshot!("small_conversation_logos", rows.join("\n"));
}
