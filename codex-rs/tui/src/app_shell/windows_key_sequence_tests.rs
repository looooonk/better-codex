use super::*;
use crate::tui::windows_key_sequence::WindowsKeySequence;
use crossterm::event::Event;
use pretty_assertions::assert_eq;
use tokio_stream::StreamExt;

#[tokio::test]
async fn mapped_shift_enter_renders_newlines_without_submitting() {
    let config = test_config().await;
    let mut shell = ShellState::snapshot_fixture();
    let mut backend = RecordingBackend::default();
    shell.composer.clear();
    let events = "first line\u{1b}[13;2u\u{1b}[13;2usecond line [13;2u"
        .chars()
        .map(|ch| {
            Ok(Event::Key(KeyEvent::new(
                if ch == '\u{1b}' {
                    KeyCode::Esc
                } else {
                    KeyCode::Char(ch)
                },
                KeyModifiers::NONE,
            )))
        });
    let mut stream = WindowsKeySequence::new(tokio_stream::iter(events));
    while let Some(event) = stream.next().await {
        let Event::Key(key) = event.unwrap() else {
            unreachable!();
        };
        assert!(!shell.handle_key(key, &config, &mut backend).await.unwrap());
    }
    assert_eq!(shell.composer.text(), "first line\n\nsecond line [13;2u");
    assert_eq!(backend.calls(), Vec::new());
    insta::assert_snapshot!(render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 24
        )
    ));
}
