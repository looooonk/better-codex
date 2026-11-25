use super::*;
use crate::app_shell::render::ShellView;
use crate::app_shell::tests::render_shell;
use pretty_assertions::assert_eq;
use ratatui::style::Color;

#[test]
fn loading_screen_identifies_a_writer_handoff_and_its_cancel_key() {
    let area = Rect::new(0, 0, 90, 3);
    let mut buf = Buffer::empty(area);
    render_loading(
        area,
        0,
        crate::app_server_session::writer_handoff::WAIT_MESSAGE,
        &mut buf,
    );
    let text = (0..area.width)
        .map(|x| buf[(x, 1)].symbol())
        .collect::<String>();
    insta::assert_snapshot!(text.trim(), @"⠋ Waiting for another Codex session to release this chat. Esc cancels.");
}

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
fn cursor_glow_follows_the_pointer_across_the_logo_while_the_glint_passes() {
    let mut shell = ShellState::snapshot_fixture();
    shell.clear_visible_transcript();
    shell.dashboard_visible = false;
    shell.chrome_animation_frame = 9;
    let screen = Rect::new(
        /*x*/ 4, /*y*/ 2, /*width*/ 70, /*height*/ 24,
    );
    let frame = |shell: &ShellState| {
        let mut buf = Buffer::empty(screen);
        (ShellView { shell }).render(screen, &mut buf);
        buf
    };
    let sweep = frame(&shell);
    let rim: Vec<_> = screen
        .positions()
        .filter(|&pos| sweep[pos].symbol() == "⣿")
        .collect();
    let [left, right] = rim[..] else {
        panic!("expected two sides of the ring")
    };

    // Approach from just outside the ring, opposite the moving glint.
    shell.set_pointer_position(Position::new(right.x + 1, right.y));
    let hovered = frame(&shell);
    assert_eq!(hovered[left].fg, sweep[left].fg);
    assert_ne!(hovered[right].fg, sweep[right].fg);
    let mark = Rect::new(left.x, left.y - 4, right.x - left.x + 1, LOGO.len() as u16);
    for pos in screen.positions() {
        if !mark.contains(pos) || sweep[pos].symbol() == " " {
            assert_eq!(&hovered[pos], &sweep[pos]);
        }
    }
    let shades = mark
        .rows()
        .map(|row| {
            row.columns()
                .map(|cell| {
                    let cell = &hovered[(cell.x, cell.y)];
                    match (cell.symbol(), cell.fg) {
                        (" ", _) => ' ',
                        (_, Color::Rgb(144, _, _)) => '·',
                        (_, Color::Rgb(145..=179, _, _)) => '░',
                        (_, Color::Rgb(180..=214, _, _)) => '▒',
                        (_, Color::Rgb(215..=239, _, _)) => '▓',
                        (_, Color::Rgb(240..=255, _, _)) => '█',
                        _ => panic!("unexpected logo color"),
                    }
                })
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("cursor_and_sweep_highlights", shades);

    shell.chrome_animation_frame = 0;
    shell.set_pointer_position(left);
    let moved = frame(&shell);
    assert_eq!(moved[right].fg, sweep[right].fg);
    assert_eq!(moved[left].fg, sweep[left].fg);
    shell.shimmer_enabled = false;
    let static_frame = frame(&shell);
    shell.clear_pointer_position();
    assert_eq!(frame(&shell), static_frame);
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
        render(&shell, layout, /*hover_position*/ None, &mut buf);
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
