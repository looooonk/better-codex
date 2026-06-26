use super::render::ShellView;
use super::*;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn renders_first_stage_shell_snapshot() {
    let shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_scrolled_transcript_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.push_status("first checkpoint");
    shell.push_status("second checkpoint");
    shell.push_status("third checkpoint");
    shell.push_status("fourth checkpoint");
    shell.scroll_transcript_up(4);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 16,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_narrow_shell_snapshot() {
    let shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn renders_markdown_transcript_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.composer.clear();
    shell.streaming_assistant.clear();
    shell.push_assistant(
        "# Result\n\
        - Render `assistant` text as markdown.\n\
        - Preserve local links like [render.rs](/workspace/better-codex/codex-rs/tui/src/app_shell/render.rs:1).\n\
        \n\
        ```rust\n\
        fn transcript() -> &'static str {\n\
            \"markdown\"\n\
        }\n\
        ```\n\
        \n\
        | Area | Status |\n\
        | --- | --- |\n\
        | code | done |\n\
        | table | done |",
    );
    shell.push_plan(
        "1. Keep transcript rendering width-aware.\n\
        2. Leave selection and copy mode for the next slice.",
    );
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 112, /*height*/ 28,
    );

    insta::assert_snapshot!(render_shell(&shell, area));
}

#[test]
fn transcript_scroll_clamps_to_last_rendered_range() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript_scroll_max.set(10);

    shell.scroll_transcript_up(TRANSCRIPT_PAGE_SCROLL_STEP);
    assert_eq!(shell.transcript_scroll, 8);

    shell.scroll_transcript_up(TRANSCRIPT_PAGE_SCROLL_STEP);
    assert_eq!(shell.transcript_scroll, 10);

    shell.scroll_transcript_down(3);
    assert_eq!(shell.transcript_scroll, 7);

    shell.scroll_transcript_to_top();
    assert_eq!(shell.transcript_scroll, 10);

    shell.scroll_transcript_to_bottom();
    assert_eq!(shell.transcript_scroll, 0);
}

#[test]
fn composer_edits_multiline_text_at_cursor() {
    let mut composer = ComposerState::default();
    composer.insert_str("alpha\nbeta");
    composer.move_left();
    composer.move_left();
    composer.insert_char('X');

    assert_eq!(
        (composer.text().to_string(), composer.cursor_position()),
        ("alpha\nbeXta".to_string(), (1, 3))
    );

    composer.move_up_or_recall_history();
    composer.insert_newline();

    assert_eq!(
        (composer.text().to_string(), composer.cursor_position()),
        ("alp\nha\nbeXta".to_string(), (1, 0))
    );
}

#[test]
fn composer_recalls_submission_history_from_draft() {
    let mut composer = ComposerState::default();
    composer.remember_submission("first");
    composer.remember_submission("second");
    composer.set_text("draft");

    composer.move_up_or_recall_history();
    assert_eq!(composer.text(), "second");

    composer.move_up_or_recall_history();
    assert_eq!(composer.text(), "first");

    composer.move_down_or_recall_history();
    assert_eq!(composer.text(), "second");

    composer.move_down_or_recall_history();
    assert_eq!(composer.text(), "draft");
}

fn render_shell(shell: &ShellState, area: Rect) -> String {
    let mut buf = Buffer::empty(area);
    ShellView { shell }.render(area, &mut buf);
    buffer_contents(&buf, area)
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

#[test]
fn summarizes_unified_diff_for_dashboard() {
    let diff = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,3 @@
-old
+new
+extra
 unchanged
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1 +1 @@
-left
+right
";

    assert_eq!(
        diff_summary_from_unified_diff(diff),
        DiffSummary {
            files: 2,
            additions: 3,
            removals: 2,
        }
    );
}
