use super::*;
use crate::app_shell::ShellState;
use crate::app_shell::tool_output_view::render_tool_output;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn renders_wide_and_compact_tool_output_popups() {
    let output = ready_output(ToolBlockStatus::Running);

    insta::assert_snapshot!("wide_tool_output_popup", render_output(&output, 100, 30));
    insta::assert_snapshot!("compact_tool_output_popup", render_output(&output, 54, 16));
}

#[test]
fn renders_truncated_tool_output_popup() {
    let output = ToolOutputState::new(
        ToolOutputTarget {
            item_id: "exec-large".to_string(),
            title: "exec generate-large-report".to_string(),
            status: ToolBlockStatus::Success,
        },
        (0..6_000)
            .map(|index| format!("report line {index:04}: {}", "x".repeat(40)))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    assert!(output.is_truncated());
    assert!(output.output().len() <= TOOL_OUTPUT_HIGH_WATER_BYTES);
    assert!(count_line_breaks(output.output()) <= TOOL_OUTPUT_HIGH_WATER_LINE_BREAKS);
    assert!(output.output().starts_with(TOOL_OUTPUT_TRUNCATION_NOTICE));
    assert!(!output.output().contains("report line 0000"));
    assert!(output.output().contains("report line 5999"));
    output.scroll_to_top();

    insta::assert_snapshot!(
        "truncated_tool_output_popup",
        render_output(&output, 80, 16)
    );
}

#[test]
fn live_output_retention_stays_bounded_across_the_core_delta_limit() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_output_with_status_for_item(
        "exec-large",
        "stream line 00000: first\n",
        ToolBlockStatus::Running,
    );
    assert!(shell.open_tool_output_at(/*transcript_index*/ 0));
    let padding = "x".repeat(1_000);
    for index in 1..=10_000 {
        shell.push_output_delta_with_status_for_item(
            "exec-large",
            format!("stream line {index:05}: {padding}\n"),
            ToolBlockStatus::Running,
        );
    }
    shell.push_output_delta_with_status_for_item(
        "exec-large",
        "stream line final: \u{B05D}\n",
        ToolBlockStatus::Running,
    );

    let retained = shell.transcript[0]
        .full_text
        .as_ref()
        .expect("streamed output should be retained");
    let open = shell
        .tool_output
        .as_ref()
        .expect("streamed output should remain open");
    assert_eq!(&**retained, open.output());
    assert!(retained.is_truncated());
    assert!(retained.len() <= TOOL_OUTPUT_HIGH_WATER_BYTES);
    assert!(retained.starts_with(TOOL_OUTPUT_TRUNCATION_NOTICE));
    assert!(!retained.contains("stream line 00000"));
    assert!(retained.contains("stream line 10000"));
    assert!(retained.ends_with("stream line final: \u{B05D}\n"));
}

#[test]
fn full_output_survives_preview_compaction_and_live_updates() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_tool_with_status_for_item(
        "exec-1",
        "exec cargo build --workspace",
        ToolBlockStatus::Running,
    );
    let initial = (0..=super::super::TRANSCRIPT_OUTPUT_HIGH_WATER_LINES)
        .map(|index| format!("compile line {index:03}: checking workspace dependency"))
        .collect::<Vec<_>>()
        .join("\n");
    shell.push_output_with_status_for_item("exec-1", initial.clone(), ToolBlockStatus::Running);

    let preview = &shell.transcript[1].text;
    assert!(preview.starts_with(super::super::TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX));
    assert!(!preview.contains("compile line 000"));
    assert!(shell.open_tool_output_at(/*transcript_index*/ 1));
    assert_eq!(
        shell
            .tool_output
            .as_ref()
            .expect("output should be open")
            .output(),
        initial
    );

    shell.push_output_delta_with_status_for_item(
        "exec-1",
        "\ncompile line 999: finished",
        ToolBlockStatus::Running,
    );
    let open = shell
        .tool_output
        .as_ref()
        .expect("output should remain open");
    assert!(open.output().contains("compile line 000"));
    assert!(open.output().ends_with("compile line 999: finished"));

    let completed = format!("{initial}\ncompile line 999: finished");
    shell.push_output_with_status_for_item("exec-1", completed.clone(), ToolBlockStatus::Success);
    let open = shell
        .tool_output
        .as_ref()
        .expect("output should remain open");
    assert_eq!(open.output(), completed);
    assert_eq!(open.target.status, ToolBlockStatus::Success);
}

#[test]
fn completion_keeps_a_richer_streamed_capture() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    let streamed = "compile line 1\ncompile line 2\ncompile line 3\n";
    shell.push_output_with_status_for_item("exec-1", streamed, ToolBlockStatus::Running);
    assert!(shell.open_tool_output_at(/*transcript_index*/ 0));

    shell.push_output_with_status_for_item("exec-1", "compile line 1\n", ToolBlockStatus::Success);

    let output = shell
        .transcript
        .front()
        .expect("completed output should remain in the transcript");
    assert_eq!(output.text, "compile line 1\n");
    assert_eq!(output.full_text.as_deref(), Some(streamed));
    let open = shell
        .tool_output
        .as_ref()
        .expect("completed output should remain open");
    assert_eq!(open.output(), streamed);
    assert_eq!(open.target.status, ToolBlockStatus::Success);
}

#[test]
fn live_popup_survives_transcript_row_eviction() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_output_with_status_for_item("exec-1", "first line", ToolBlockStatus::Running);
    assert!(shell.open_tool_output_at(/*transcript_index*/ 0));
    for index in 0..=super::super::MAX_TRANSCRIPT_LINES {
        shell.push_status(format!("unrelated status {index}"));
    }
    assert!(
        !shell
            .transcript
            .iter()
            .any(|line| line.item_id.as_deref() == Some("exec-1"))
    );

    shell.push_output_delta_with_status_for_item("exec-1", "\nlast line", ToolBlockStatus::Running);

    let expected = "first line\nlast line";
    let open = shell
        .tool_output
        .as_ref()
        .expect("live output should remain open");
    assert_eq!(open.output(), expected);
    let restored = shell
        .transcript
        .iter()
        .find(|line| line.item_id.as_deref() == Some("exec-1"))
        .expect("live output should be restored to the transcript");
    assert_eq!(restored.full_text.as_deref(), Some(expected));
}

#[test]
fn completion_preserves_open_output_after_transcript_row_eviction() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_output_with_status_for_item(
        "exec-1",
        "first line\nsecond line",
        ToolBlockStatus::Running,
    );
    assert!(shell.open_tool_output_at(/*transcript_index*/ 0));
    for index in 0..=super::super::MAX_TRANSCRIPT_LINES {
        shell.push_status(format!("unrelated status {index}"));
    }

    shell.push_output_with_status_for_item("exec-1", "first line", ToolBlockStatus::Success);

    let open = shell
        .tool_output
        .as_ref()
        .expect("completed output should remain open");
    assert_eq!(open.output(), "first line\nsecond line");
    assert_eq!(open.target.status, ToolBlockStatus::Success);
    let restored = shell
        .transcript
        .iter()
        .find(|line| line.item_id.as_deref() == Some("exec-1"))
        .expect("completed output should be restored to the transcript");
    assert_eq!(restored.full_text.as_deref(), Some(open.output()));
}

#[test]
fn navigation_pauses_and_resumes_live_tail_following() {
    let mut output = ready_output(ToolBlockStatus::Running);
    let initial = output.ready_viewport(/*width*/ 48, /*height*/ 6);
    assert!(initial.scroll > 0);

    output.scroll_up(/*amount*/ 2);
    let scrolled = output.scroll();
    output.append_output(
        "\nnew output while inspecting older rows",
        ToolBlockStatus::Running,
    );
    assert_eq!(
        output.ready_viewport(/*width*/ 48, /*height*/ 6).scroll,
        scrolled
    );

    output.scroll_to_bottom();
    output.append_output("\nlatest output at the live tail", ToolBlockStatus::Running);
    let followed = output.ready_viewport(/*width*/ 48, /*height*/ 6);
    assert_eq!(followed.scroll, output.scroll_max.get());
    assert!(
        followed
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .any(|span| span.content.contains("latest output"))
    );
}

#[test]
fn keyboard_navigation_reaches_edges_and_escape_closes() {
    let mut shell = ShellState::snapshot_fixture();
    shell.tool_output = Some(ready_output(ToolBlockStatus::Success));
    shell
        .tool_output
        .as_ref()
        .expect("output should be open")
        .ready_viewport(/*width*/ 48, /*height*/ 6);

    assert!(shell.handle_tool_output_key(key(KeyCode::Home)));
    assert_eq!(
        shell
            .tool_output
            .as_ref()
            .expect("output should remain open")
            .scroll(),
        0
    );
    assert!(shell.handle_tool_output_key(key(KeyCode::End)));
    assert!(
        shell
            .tool_output
            .as_ref()
            .expect("output should remain open")
            .scroll()
            > 0
    );
    assert!(shell.handle_tool_output_key(key(KeyCode::Esc)));
    assert!(shell.tool_output.is_none());
}

fn ready_output(status: ToolBlockStatus) -> ToolOutputState {
    ToolOutputState::new(
        ToolOutputTarget {
            item_id: "exec-42".to_string(),
            title: "exec cargo build --workspace --all-targets".to_string(),
            status,
        },
        (0..30)
            .map(|index| {
                if index == 4 {
                    "\u{1b}[32mFinished\u{1b}[0m dependency graph".to_string()
                } else {
                    format!("build line {index:02}: compiling a representative workspace crate")
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn render_output(output: &ToolOutputState, width: u16, height: u16) -> String {
    let area = Rect::new(/*x*/ 0, /*y*/ 0, width, height);
    let mut buffer = Buffer::empty(area);
    render_tool_output(output, area, &mut buffer);
    (area.y..area.bottom())
        .map(|y| {
            let line = (area.x..area.right())
                .filter_map(|x| buffer.cell((x, y)))
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>();
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
