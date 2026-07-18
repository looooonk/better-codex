use super::ComposerInput;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn render(composer: &ComposerInput, area: Rect) -> String {
    let mut buffer = Buffer::empty(area);
    composer.render_ref(area, &mut buffer);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn composer_input_empty_and_multiline_snapshot() {
    let mut composer = ComposerInput::new();
    composer.set_hint_items(vec![("Enter", "send"), ("Shift+Enter", "newline")]);
    let area = Rect::new(0, 0, 32, 5);
    assert_snapshot!("composer_input_empty", render(&composer, area));

    for ch in "first line".chars() {
        let _ = composer.input(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let _ = composer.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    let _ = composer.handle_paste("second line wraps across the available width".to_string());
    assert_snapshot!("composer_input_multiline", render(&composer, area));
}

#[test]
fn cursor_position_accounts_for_wrapped_prior_lines() {
    let mut composer = ComposerInput::new();
    let _ = composer.handle_paste("123456789\nab".to_string());

    assert_eq!(composer.cursor_pos(Rect::new(0, 0, 10, 5)), Some((4, 2)));
}

#[test]
fn composer_input_uses_macos_cursor_shortcuts() {
    let mut composer = ComposerInput::new();
    assert!(composer.handle_paste("alpha\nbeta gamma".to_string()));

    let _ = composer.input(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    let _ = composer.input(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
    assert_eq!(composer.text(), "alpha\nXbeta gamma");
    assert_snapshot!("composer_input_middle_cursor", composer.text_with_cursor());

    let _ = composer.input(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
    let _ = composer.input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL));
    assert_eq!(composer.text(), "alpha\nXbeta ");
}
