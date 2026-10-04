use super::*;
use pretty_assertions::assert_eq;

fn copy_message(markdown: &str, width: u16) -> String {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_assistant(markdown);
    let area = Rect::new(/*x*/ 0, /*y*/ 0, width, /*height*/ 50);
    let viewport = transcript_viewport(&shell, area);
    let last = viewport.layout.total_lines - 1;
    let text = rendered_line_text(viewport.layout.row_at(last).unwrap().line().unwrap());
    let start = VisualGraphemeHit::new(/*row*/ 0, /*column*/ 0, /*width*/ 1);
    let end = grapheme_hit_at(
        &text,
        last,
        crate::width::display_width(&text).saturating_sub(1),
    )
    .unwrap();
    selected_markdown(&shell, area, NormalizedVisualRange::from_hits(start, end)).unwrap()
}

#[test]
fn selection_preserves_balanced_markup_and_joins_soft_wraps() {
    assert_eq!(
        copy_message("A **bold phrase with many words** and `inline_code`.", 34),
        "A **bold phrase with many words** and `inline_code`."
    );
}

#[test]
fn selection_preserves_task_state_and_local_link_labels() {
    let copied = copy_message(
        "- [x] Read [the implementation](/workspace/src/main.rs:12)\n- [ ] Keep `next_step` ready",
        54,
    );
    insta::assert_snapshot!(copied);
}

#[test]
fn selecting_part_of_an_emphasized_unicode_word_balances_the_markup() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_assistant("Before **café界** after");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 15,
    );
    let viewport = transcript_viewport(&shell, area);
    let text = rendered_line_text(viewport.layout.row_at(0).unwrap().line().unwrap());
    let start = crate::width::display_width(&text[..text.find("café").unwrap()]);
    let anchor = grapheme_hit_at(&text, /*row*/ 0, start).unwrap();
    let focus = grapheme_hit_at(&text, /*row*/ 0, start + 5).unwrap();
    assert_eq!(
        selected_markdown(
            &shell,
            area,
            NormalizedVisualRange::from_hits(anchor, focus)
        ),
        Some("**café界**".into())
    );
}
