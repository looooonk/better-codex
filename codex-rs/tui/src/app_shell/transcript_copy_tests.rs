use super::*;
use crate::app_shell::ToolBlockStatus;
use crate::app_shell::TranscriptLine;
use crossterm::event::KeyEventKind;
use pretty_assertions::assert_eq;

#[test]
fn copies_requested_response_as_markdown() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.push_assistant("oldest");
    shell.push_user("prompt");
    shell.push_tool_with_status("tool", ToolBlockStatus::Success);
    shell.push_assistant("## Middle\n\n```rust\nfn main() {}\n```");
    shell.push_assistant("latest");
    let mut copied = None;

    shell.copy_response_with(
        ResponseOrdinal::from_ascii_digit('2').expect("2 should be a response ordinal"),
        |text| {
            copied = Some(text.to_string());
            Ok(None)
        },
    );

    assert_eq!(
        (copied, shell.transcript.back()),
        (
            Some("## Middle\n\n```rust\nfn main() {}\n```".to_string()),
            Some(&TranscriptLine::new(
                TranscriptKind::Status,
                "copied 2nd latest Codex response"
            ))
        )
    );
}

#[test]
fn reports_when_requested_response_is_unavailable() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.push_assistant("only response");

    shell.copy_response_with(
        ResponseOrdinal::from_ascii_digit('3').expect("3 should be a response ordinal"),
        |_| panic!("clipboard should not be called"),
    );

    assert_eq!(
        shell.transcript.back(),
        Some(&TranscriptLine::new(
            TranscriptKind::Error,
            "No 3rd latest Codex response to copy"
        ))
    );
}

#[test]
fn reports_when_latest_response_is_unavailable() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();

    shell.copy_response_request_with(
        CopyResponseRequest::Response(ResponseOrdinal::LATEST),
        |_| panic!("clipboard should not be called"),
    );

    assert_eq!(
        shell.transcript.back(),
        Some(&TranscriptLine::new(
            TranscriptKind::Error,
            "No Codex response to copy"
        ))
    );
}

#[test]
fn reports_clipboard_failure_for_requested_response() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.push_assistant("response");

    shell.copy_response_with(ResponseOrdinal::LATEST, |text| {
        assert_eq!(text, "response");
        Err("clipboard offline".to_string())
    });

    assert_eq!(
        shell.transcript.back(),
        Some(&TranscriptLine::new(
            TranscriptKind::Error,
            "Copy failed: clipboard offline"
        ))
    );
}

#[test]
fn invalid_copy_request_reports_usage() {
    let mut shell = ShellState::snapshot_fixture();

    shell.copy_response_request_with(CopyResponseRequest::Invalid, |_| {
        panic!("clipboard should not be called")
    });

    assert_eq!(
        shell.transcript.back(),
        Some(&TranscriptLine::new(
            TranscriptKind::Error,
            "Usage: /copy [1-9]"
        ))
    );
}

#[test]
fn alt_digits_map_to_response_ordinals() {
    let events = [
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('5'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('9'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('0'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        KeyEvent::new_with_kind(KeyCode::Char('1'), KeyModifiers::ALT, KeyEventKind::Release),
    ];

    assert_eq!(
        events.map(response_ordinal_from_alt_key),
        [
            ResponseOrdinal::from_ascii_digit('1'),
            ResponseOrdinal::from_ascii_digit('5'),
            ResponseOrdinal::from_ascii_digit('9'),
            None,
            None,
            None,
        ]
    );
}

#[test]
fn copies_selected_transcript_item() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript_selection = Some(1);
    let mut copied = None;

    shell.copy_selected_transcript_with(|text| {
        copied = Some(text.to_string());
        Ok(None)
    });

    assert_eq!(
        (copied, shell.transcript.back()),
        (
            Some("Create a divergent standalone TUI.".to_string()),
            Some(&TranscriptLine::new(
                TranscriptKind::Status,
                "copied you transcript item"
            ))
        )
    );
}

#[test]
fn copies_latest_assistant_without_selection() {
    let mut shell = ShellState::snapshot_fixture();
    let mut copied = None;

    shell.copy_selected_transcript_with(|text| {
        copied = Some(text.to_string());
        Ok(None)
    });

    assert_eq!(
        (copied, shell.transcript.back()),
        (
            Some("Started a fullscreen app shell backed by app-server turns.".to_string()),
            Some(&TranscriptLine::new(
                TranscriptKind::Status,
                "copied latest Codex response"
            ))
        )
    );
}
