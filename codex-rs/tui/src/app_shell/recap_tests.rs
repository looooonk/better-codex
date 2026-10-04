use super::*;
use pretty_assertions::assert_eq;

#[test]
fn recap_history_ignores_tool_data_and_preserves_recent_user_requests() {
    use super::super::TranscriptLine;
    let transcript = [
        TranscriptLine::new(TranscriptKind::User, "Fix the build"),
        TranscriptLine::new(TranscriptKind::Output, "private tool output"),
        TranscriptLine::new(TranscriptKind::Assistant, "The build passes"),
        TranscriptLine::new(TranscriptKind::User, "Also package it"),
    ]
    .into();
    assert_eq!(
        history::recap_history(&transcript),
        "User: Fix the build\n\nAssistant: The build passes\n\nPending user request: Also package it"
    );
}

#[test]
fn recap_history_is_bounded_and_keeps_both_ends_of_recent_corrections() {
    use super::super::TranscriptLine;
    let transcript = [
        TranscriptLine::new(
            TranscriptKind::User,
            format!("BEGIN{}END", "語".repeat(20_000)),
        ),
        TranscriptLine::new(TranscriptKind::Assistant, "Validation is still pending"),
        TranscriptLine::new(TranscriptKind::User, "Do not publish yet"),
    ]
    .into();
    let history = history::recap_history(&transcript);
    assert!(history.contains("BEGIN") && history.contains("END"));
    assert!(history.ends_with("Pending user request: Do not publish yet"));
    assert!(RecapPrompt::new(&history).render().len() <= history::RECAP_PROMPT_MAX_BYTES);
}

#[test]
fn recap_history_budget_includes_labels_for_many_large_exchanges() {
    use super::super::TranscriptLine;
    let transcript = (0..20)
        .flat_map(|turn| {
            [
                TranscriptLine::new(
                    TranscriptKind::User,
                    format!("Request {turn}: {}", "xyz ".repeat(3000)),
                ),
                TranscriptLine::new(
                    TranscriptKind::Assistant,
                    format!("Answer {turn}: {}", "語 ".repeat(3000)),
                ),
            ]
        })
        .collect();
    let rendered = RecapPrompt::new(&history::recap_history(&transcript)).render();
    assert!(rendered.len() <= history::RECAP_PROMPT_MAX_BYTES);
    assert!(rendered.contains("Request 19:") && rendered.contains("Answer 19:"));
    assert!(!rendered.contains("Request 0:"));
}

#[test]
fn recap_parser_normalizes_content_and_rejects_oversized_results() {
    assert_eq!(
        parse_recap(r#"{"summary":" Tests pass. ","next_action":" "}"#).unwrap(),
        GeneratedRecap {
            summary: "Tests pass.".to_string(),
            next_action: None
        }
    );
    assert!(
        parse_recap(&json!({ "summary": "x".repeat(701), "next_action": null }).to_string())
            .is_err()
    );
    assert!(parse_recap(r#"{"summary":"Ready","next_action":"Next","unexpected":true}"#).is_err());
}

#[test]
fn bounded_recap_renders_summary_and_next_action() {
    use crate::app_shell::render::ShellView;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    let recap = parse_recap(r#"{"summary":"The updated interface supports the current model catalog and keeps existing conversations available. Validation is still running.","next_action":"Review the remaining test failures before publishing."}"#).unwrap();
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.dashboard_visible = false;
    shell.push_system(recap.into_message());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let mut buffer = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut buffer);
    let screen = buffer
        .content
        .chunks(100)
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("conversation_recap", screen);
}
