use super::*;
use crate::app_shell::CompletedItemOrigin;
use crate::app_shell::ToolBlockStatus;
use crate::app_shell::transcript_render::TranscriptTarget;
use crate::app_shell::transcript_view;
use crate::app_shell::transcript_view::TranscriptCardHit;
use codex_app_server_protocol::ThreadItem;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use serde_json::json;
use std::path::Path;

fn shell() -> ShellState {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_transcript();
    shell
}

fn rows(shell: &ShellState) -> String {
    let layout = shell.transcript_render_cache.borrow_mut().layout(
        shell,
        /*width*/ 52,
        Path::new(&shell.cwd),
    );
    layout
        .visible_hyperlink_lines(/*visible_from*/ 0, layout.total_lines)
        .iter()
        .map(|line| line.line.to_string().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn live_and_restored_calls_fold_at_message_boundaries_and_retain_output() {
    let render = |origin| {
        let mut shell = shell();
        shell.push_assistant("I’ll check both sources.");
        for value in [
            json!({"type":"functionCallOutput", "id":"cmd", "name":"exec_command", "namespace":"functions", "output":"build passed"}),
            json!({"type":"collabAgentToolCall", "id":"agent", "tool":"listAgents", "status":"completed", "senderThreadId":"parent", "receiverThreadIds":[], "agentsStates":{}}),
            json!({"type":"webSearch", "id":"web", "query":"build documentation"}),
            json!({"type":"agentMessage", "id":"message", "text":"The build passed."}),
            json!({"type":"dynamicToolCall", "id":"lookup", "namespace":"docs", "tool":"lookup", "arguments":{}, "status":"completed", "success":true}),
        ] {
            shell.ingest_completed_item(serde_json::from_value(value).unwrap(), origin);
        }
        let folded = rows(&shell);
        let start = shell.tool_groups().next().unwrap().rows.start;
        shell.toggle_tool_group(start);
        let expanded = rows(&shell);
        shell.toggle_tool_group(start);
        assert_eq!(rows(&shell), folded);
        (folded, expanded)
    };
    let live = render(CompletedItemOrigin::Live);
    assert_eq!(live, render(CompletedItemOrigin::Historical));
    insta::assert_snapshot!("tool_groups_folded", live.0);
    insta::assert_snapshot!("tool_groups_expanded", live.1);
}

#[test]
fn expanding_a_long_group_keeps_its_header_clickable_at_the_same_position() {
    let mut shell = shell();
    shell.push_user("Run checks");
    for i in 0..30 {
        let id = format!("exec-{i}");
        shell.push_tool_with_status_for_item(
            &id,
            format!("exec check-{i}"),
            ToolBlockStatus::Running,
            Some("Running command".into()),
        );
        shell.push_output_with_status_for_item(&id, "progress", ToolBlockStatus::Running);
    }
    shell.push_assistant("Done");
    let area = Rect::new(
        /*x*/ 4, /*y*/ 3, /*width*/ 45, /*height*/ 12,
    );
    let target = Some(TranscriptCardHit::ToolGroup {
        transcript_index: 1,
    });
    let position = area
        .positions()
        .find(|&position| transcript_view::transcript_card_at(&shell, area, position) == target)
        .unwrap();
    transcript_view::toggle_tool_group_at(&mut shell, area, 1);
    assert_eq!(
        transcript_view::transcript_card_at(&shell, area, position),
        target
    );
    assert!(rows(&shell).contains("check-29"));
    transcript_view::toggle_tool_group_at(&mut shell, area, 1);
    assert_eq!(
        transcript_view::transcript_card_at(&shell, area, position),
        target
    );
    assert!(!rows(&shell).contains("check-29"));
}

#[test]
fn pending_calls_keep_their_group_position_and_expansion_across_late_completion() {
    let mut shell = shell();
    shell.push_tool_with_status_for_item(
        "mcp",
        "mcp files/read",
        ToolBlockStatus::Running,
        Some("Calling files/read".into()),
    );
    shell.toggle_tool_group(0);
    shell.push_tool_with_status_for_item(
        "second",
        "exec pwd",
        ToolBlockStatus::Running,
        Some("Running command".into()),
    );
    shell.push_assistant("Another update");
    let error: ThreadItem = serde_json::from_value(json!({
        "type":"mcpToolCall", "id":"mcp", "server":"files", "tool":"read", "status":"failed",
        "arguments":{}, "error":{"message":"access denied"}
    }))
    .unwrap();
    shell.ingest_completed_item(error, CompletedItemOrigin::Live);
    let rendered = rows(&shell);
    assert!(rendered.contains("⌄ Running command..."));
    assert!(rendered.find("access denied").unwrap() < rendered.find("exec pwd").unwrap());

    shell.toggle_tool_group(0);
    assert!(!rows(&shell).contains("access denied"));
    shell.select_transcript_match(shell.transcript[1].render_revision);
    let layout = shell.transcript_render_cache.borrow_mut().layout(
        &shell,
        /*width*/ 40,
        Path::new(&shell.cwd),
    );
    assert!(layout.target_rows(TranscriptTarget::Item(1)).is_some());
}

#[test]
fn output_arriving_before_its_call_is_never_absorbed_by_an_unrelated_call() {
    let mut shell = shell();
    shell.push_tool_with_status_for_item(
        "first",
        "exec pwd",
        ToolBlockStatus::Success,
        Some("Running command".into()),
    );
    shell.push_output_with_status_for_item("late", "unmatched result", ToolBlockStatus::Running);
    assert!(rows(&shell).contains("unmatched result"));

    shell.push_tool_with_status_for_item(
        "late",
        "docs/lookup",
        ToolBlockStatus::Success,
        Some("Calling docs/lookup".into()),
    );
    let folded = rows(&shell);
    assert!(folded.contains("> Calling docs/lookup..."));
    assert!(!folded.contains("unmatched result"));
    shell.toggle_tool_group(0);
    assert!(rows(&shell).contains("unmatched result"));
}

#[test]
fn keyboard_cycles_tool_groups_and_folds_each_independently() {
    let mut shell = shell();
    shell.push_user("Request");
    shell.push_diff("one");
    shell.push_assistant("Progress");
    shell.push_diff("two");
    shell.select_first_transcript_item();
    let select = |shell: &mut ShellState, key| {
        shell.handle_transcript_selection_key(KeyEvent::new(key, KeyModifiers::NONE));
    };
    select(&mut shell, KeyCode::Tab);
    select(&mut shell, KeyCode::Char(' '));
    select(&mut shell, KeyCode::Tab);
    assert_eq!(shell.transcript_selection, Some(3));
    select(&mut shell, KeyCode::BackTab);
    assert_eq!(shell.transcript_selection, Some(1));
    assert_eq!(
        shell
            .tool_groups()
            .map(|group| group.expanded)
            .collect::<Vec<_>>(),
        vec![true, false]
    );
}

#[test]
fn trimming_an_expanded_group_evicts_whole_calls_and_preserves_the_remaining_group() {
    let mut shell = shell();
    for i in 0..crate::app_shell::MAX_TRANSCRIPT_LINES / 2 {
        let id = format!("exec-{i}");
        shell.push_tool_with_status_for_item(
            &id,
            format!("exec {i}"),
            ToolBlockStatus::Success,
            Some("Running command".into()),
        );
        shell.push_output_with_status_for_item(
            &id,
            format!("output {i}"),
            ToolBlockStatus::Success,
        );
    }
    shell.toggle_tool_group(0);
    shell.push_assistant("done");

    assert_eq!(shell.transcript[0].item_id.as_deref(), Some("exec-1"));
    let group = shell.tool_groups().next().unwrap();
    assert!(group.expanded);
    assert_eq!(group.rows, 0..shell.transcript.len() - 1);
    shell.toggle_tool_group(0);
    let folded = rows(&shell);
    assert!(!folded.contains("output"));
    assert!(folded.contains("> Running command..."));
}
