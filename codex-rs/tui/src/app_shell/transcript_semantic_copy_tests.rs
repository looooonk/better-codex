use super::*;
use pretty_assertions::assert_eq;

fn copy_message(markdown: &str, width: u16) -> String {
    copy_with_math(markdown, width, None)
}

fn copy_with_math(markdown: &str, width: u16, cell: Option<crate::equations::CellSize>) -> String {
    let mut shell = ShellState::snapshot_fixture();
    shell.graphics.borrow_mut().cell = cell;
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
fn copying_wrapped_headings_restores_markers_and_inline_formatting() {
    let markdown = "### A long heading with *emphasis* and `code` at the end";
    for width in [32, 96] {
        assert_eq!(
            copy_message(markdown, width),
            markdown.replace("*emphasis*", "_emphasis_")
        );
        assert_eq!(copy_message("## `type_name`", width), "## `type_name`");
        assert_eq!(
            copy_message("First line\nsecond line\n---", width),
            "## First line second line"
        );
        assert_eq!(
            copy_message("> ### Quoted title", width),
            "### Quoted title"
        );
        let copied = copy_message("##\n\nBody", width);
        assert_eq!(
            copied.lines().map(str::trim_end).collect::<Vec<_>>(),
            ["##", "", "Body"]
        );
    }
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

#[test]
fn copying_centered_and_inline_math_preserves_original_tex() {
    let cell = crate::equations::CellSize {
        width: 8,
        height: 16,
    };
    for width in [40, 87] {
        let markdown = r"Before $x^2$ then \(\frac{a}{b}\).

\[\begin{pmatrix}a&b\\c&d\end{pmatrix}\]

After $$x+y$$ end.";
        let result = copy_with_math(markdown, width, Some(cell));
        // The ordinary Markdown serializer can use two trailing spaces for a hard break.
        let paragraphs = result
            .lines()
            .map(str::trim_end)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(
            paragraphs,
            vec![
                r"Before $x^2$ then \(\frac{a}{b}\).",
                r"\[\begin{pmatrix}a&b\\c&d\end{pmatrix}\]",
                "After",
                "$$x+y$$",
                "end."
            ]
        );
    }
}
