use super::super::render::ShellView;
use super::*;
use pretty_assertions::assert_eq;

#[test]
fn popup_shows_only_the_two_most_recent_messages_snapshot() {
    let shell = queued_shell();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 18,
    );
    let rendered = render_shell(&shell, area);

    assert!(!rendered.contains("oldest hidden message"));
    assert!(rendered.contains("second queued message"));
    assert!(rendered.contains("newest queued message continued"));
    insta::assert_snapshot!(rendered);
}

#[test]
fn popup_rows_map_to_queue_indices_and_start_editing_the_clicked_message() {
    let mut shell = queued_shell();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 18,
    );
    let view = ShellView { shell: &shell };
    let transcript = view.transcript_area(area);
    let input = view.input_area(area);
    let layout = popup_layout(&shell, transcript, input).expect("queue popup should be visible");
    let first = Position::new(
        layout.content.x.saturating_add(1),
        layout.content.y.saturating_add(1),
    );
    let second = Position::new(first.x, first.y.saturating_add(1));

    assert_eq!(
        (
            hit_at(&shell, transcript, input, first),
            hit_at(&shell, transcript, input, second),
            hit_at(
                &shell,
                transcript,
                input,
                Position::new(first.x, layout.content.y),
            ),
        ),
        (
            Some(QueuedMessagePopupHit::Message(1)),
            Some(QueuedMessagePopupHit::Message(2)),
            Some(QueuedMessagePopupHit::Chrome),
        )
    );

    shell.pointer_position = Some(second);
    let mut buf = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut buf);
    assert_eq!(buf[second].style().bg, Some(palette::border()));

    let Some(QueuedMessagePopupHit::Message(clicked)) = hit_at(&shell, transcript, input, second)
    else {
        panic!("newest row should hit");
    };
    assert!(shell.composer.edit_queued_message(clicked));
    assert_eq!(shell.composer.text(), "newest queued message\ncontinued");
    insta::assert_snapshot!("queued_message_popup_editing", render_shell(&shell, area));
    assert!(shell.composer.finish_queued_message_edit());
    assert_eq!(shell.composer.text(), "ordinary draft");
}

#[test]
fn slash_command_suggestions_take_precedence_over_the_queue_popup() {
    let mut shell = queued_shell();
    shell.composer.set_text("/");
    let rendered = render_shell(
        &shell,
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 18,
        ),
    );

    assert!(rendered.contains("COMMANDS"));
    assert!(!rendered.contains("QUEUED"));
}

fn queued_shell() -> ShellState {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    for message in [
        "oldest hidden message",
        "second queued message",
        "newest queued message\ncontinued",
    ] {
        shell.composer.set_text(message);
        assert!(shell.composer.queue_current_message());
    }
    shell.composer.set_text("ordinary draft");
    shell
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
