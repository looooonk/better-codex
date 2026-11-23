use super::*;
use pretty_assertions::assert_eq;

#[test]
fn white_highlight_moves_right_across_gray_activity_and_rate() {
    let area = Rect::new(
        /*x*/ 2, /*y*/ 1, /*width*/ 40, /*height*/ 1,
    );
    let frame = |elapsed| {
        let mut buf = Buffer::empty(area);
        render_activity("thinking", /*rate*/ 32, elapsed, area, &mut buf);
        buf
    };
    let static_text = frame(None);
    let early = frame(Some(Duration::from_millis(/*millis*/ 400)));
    let late = frame(Some(Duration::from_millis(/*millis*/ 1400)));
    let start = area.x;
    let colors = |buf: &Buffer| [2, 17].map(|offset| buf[(start + offset, area.y)].fg);
    let gray = rgb_color((152, 152, 152));
    let white = rgb_color((248, 248, 248));
    assert_eq!(
        [colors(&static_text), colors(&early), colors(&late)],
        [[gray, gray], [white, gray], [gray, white]]
    );
    assert_eq!(
        early
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>()
            .trim(),
        "Thinking... · 32 tok/s"
    );
}

#[test]
fn activity_follows_tool_cards_and_wrapped_output_and_scrolls_with_them() {
    use super::super::ToolBlockStatus;
    use super::super::tests::render_shell;
    use unicode_width::UnicodeWidthStr;

    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.finish_streaming_assistant();
    shell.transcript.clear();
    shell.record_active_turn_started("turn".into());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 60, /*height*/ 18,
    );
    shell.push_tool_with_status_for_item(
        "tool",
        "Inspect files",
        ToolBlockStatus::Running,
        Some(crate::app_shell::tool_groups::ToolAction::Edit),
    );
    let rendered = render_shell(&shell, area);
    let rows: Vec<_> = rendered.lines().collect();
    let tool = rows
        .iter()
        .position(|row| row.contains("Editing files"))
        .unwrap();
    assert!(rows[tool + 1].is_empty());
    assert!(rows[tool + 2].contains("Thinking..."));

    shell.push_streaming_assistant_delta("message", &"output ".repeat(80));
    shell.push_streaming_assistant_delta("message", "\n\nfinal line");
    let rendered = render_shell(&shell, area);
    let rows: Vec<_> = rendered.lines().collect();
    let last = rows
        .iter()
        .position(|row| row.contains("final line"))
        .unwrap();
    assert!(
        rows[last + 1].trim_matches([' ', '│', '┃']).is_empty(),
        "{rendered}"
    );
    let (indent, _) = rows[last + 2].split_once("Thinking...").unwrap();
    let output_indent = rows[last].width() - rows[last].trim_start().width();
    assert_eq!(indent.width(), output_indent);
    shell.scroll_transcript_to_top();
    assert!(render_shell(&shell, area).contains("Editing files"));
    assert!(!render_shell(&shell, area).contains("Thinking..."));
    shell.scroll_transcript_to_bottom();
    assert_eq!(render_shell(&shell, area), rendered);
}

#[test]
fn compaction_activity_only_spans_the_current_turns_compaction_item() {
    use super::super::tests::render_shell;
    use codex_app_server_protocol::ItemCompletedNotification;
    use codex_app_server_protocol::ItemStartedNotification;
    use codex_app_server_protocol::ServerNotification;
    use codex_app_server_protocol::ThreadItemPayload;

    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.finish_streaming_assistant();
    shell.transcript.clear();
    shell.push_assistant("Here is the latest result.");
    shell.record_active_turn_started("current".into());
    let thread = shell.thread_id.to_string();
    let item = |id: &str| ThreadItemPayload::ContextCompaction { id: id.into() };
    let started = |turn: &str| {
        ServerNotification::ItemStarted(ItemStartedNotification::new(
            thread.clone(),
            turn.into(),
            item("compact"),
            /*started_at_ms*/ 1,
        ))
    };
    let completed = |id: &str| {
        ServerNotification::ItemCompleted(ItemCompletedNotification::new(
            thread.clone(),
            "current".into(),
            item(id),
            Some(1),
            /*completed_at_ms*/ 2,
        ))
    };
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 60, /*height*/ 18,
    );

    shell.handle_notification(started("previous"));
    assert!(render_shell(&shell, area).contains("Thinking..."));
    shell.handle_notification(started("current"));
    insta::assert_snapshot!("compaction_activity", render_shell(&shell, area));
    shell.handle_notification(completed("other"));
    assert!(render_shell(&shell, area).contains("Compacting..."));
    shell.handle_notification(completed("compact"));
    assert!(render_shell(&shell, area).contains("Thinking..."));
    shell.handle_notification(started("current"));
    shell.clear_active_turn();
    shell.record_active_turn_started("next".into());
    assert!(render_shell(&shell, area).contains("Thinking..."));
}
