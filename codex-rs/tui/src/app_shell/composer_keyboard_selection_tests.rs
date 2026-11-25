use super::*;
use crate::app_shell::composer_layout::composer_body;
use crate::app_shell::design::text_selection_style;
use crate::app_shell::keybindings::ShellKeymap;
use crossterm::event::KeyEventKind;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn shift_motions_extend_shrink_and_reverse_the_same_selection() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    let mut backend = RecordingBackend::default();
    let text = "first\ncafe\u{301} fox\nlast";
    shell.composer.set_text(text);
    shell.composer.set_cursor("first\ncafe\u{301}".len());
    for (code, modifier, expected) in [
        (KeyCode::Left, KeyModifiers::NONE, Some("e\u{301}")),
        (KeyCode::Left, KeyModifiers::ALT, Some("cafe\u{301}")),
        (KeyCode::Right, KeyModifiers::CONTROL, None),
        (KeyCode::Right, KeyModifiers::ALT, Some(" fox")),
        (KeyCode::Left, KeyModifiers::CONTROL, Some(" ")),
        (KeyCode::Left, KeyModifiers::NONE, None),
        (KeyCode::Home, KeyModifiers::NONE, Some("cafe\u{301}")),
        (KeyCode::End, KeyModifiers::NONE, Some(" fox")),
        (KeyCode::Right, KeyModifiers::SUPER, Some(" fox\nlast")),
        (
            KeyCode::Left,
            KeyModifiers::SUPER,
            Some("first\ncafe\u{301}"),
        ),
        (KeyCode::End, KeyModifiers::CONTROL, Some(" fox\nlast")),
        (
            KeyCode::Home,
            KeyModifiers::CONTROL,
            Some("first\ncafe\u{301}"),
        ),
    ] {
        shell
            .handle_key(
                KeyEvent::new(code, modifier | KeyModifiers::SHIFT),
                &config,
                &mut backend,
            )
            .await
            .unwrap();
        assert_eq!(
            shell.composer.selected_text(),
            expected,
            "{code:?} + {modifier:?}"
        );
    }
    shell
        .handle_key(key_char('X'), &config, &mut backend)
        .await
        .unwrap();
    assert_eq!(composer_cursor_text(&shell.composer), "X| fox\nlast");
    assert!(backend.calls().is_empty());
}

#[tokio::test]
async fn held_shift_arrow_extends_and_highlights_even_with_remapped_cursor_navigation() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.keybindings =
        ShellKeymap::from_config(&toml::from_str("[editor]\nmove_left = 'f19'").unwrap()).unwrap();
    shell.composer.set_text("alpha beta");
    let mut backend = RecordingBackend::default();
    let area = Rect::new(0, 0, 70, 20);
    for kind in [
        KeyEventKind::Press,
        KeyEventKind::Repeat,
        KeyEventKind::Repeat,
        KeyEventKind::Repeat,
        KeyEventKind::Release,
    ] {
        shell
            .handle_key_in_area(
                area,
                KeyEvent::new_with_kind(KeyCode::Left, KeyModifiers::SHIFT, kind),
                &config,
                &mut backend,
            )
            .await
            .unwrap();
    }
    assert_eq!(shell.composer.selected_text(), Some("beta"));
    let body = composer_body((ShellView { shell: &shell }).input_area(area));
    let buffer = render_shell_buffer(&shell, area);
    let mut row = String::new();
    let mut highlights = String::new();
    for x in body.x..body.right() {
        let cell = &buffer[(x, body.y)];
        row.push_str(cell.symbol());
        highlights.push(if cell.style().bg == text_selection_style().bg {
            '^'
        } else {
            ' '
        });
    }
    insta::assert_snapshot!(format!("{}\n{}", row.trim_end(), highlights.trim_end()), @"> alpha beta\n        ^^^^");
}

#[tokio::test]
async fn shift_up_selects_wrapped_rows_without_recalling_history() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    let mut backend = RecordingBackend::default();
    shell
        .composer
        .remember_submission("unrelated previous message");
    let text = "You should do items A, B, plus items C and D, and C.";
    shell.composer.set_text(text);
    let area = Rect::new(0, 0, 50, 16);
    let up = KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT);
    for expected in [&text[6..], text, text] {
        shell
            .handle_key_in_area(area, up, &config, &mut backend)
            .await
            .unwrap();
        assert_eq!(shell.composer.selected_text(), Some(expected));
    }
    shell
        .handle_key_in_area(
            area,
            KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    assert_eq!(shell.composer.selected_text(), Some("and C."));
}
