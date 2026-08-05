use super::*;
use pretty_assertions::assert_eq;

fn selected_message(shell: &ShellState) -> Option<(TranscriptKind, String)> {
    shell
        .selected_transcript_copy_text()
        .map(|(kind, text)| (kind, text.to_string()))
}

#[test]
fn selection_navigation_only_visits_user_messages() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.push_system("system");
    shell.push_user("first");
    shell.push_assistant("assistant");
    shell.push_tool("tool");
    shell.push_user("second");
    shell.push_diff("diff");
    shell.push_user("third");
    shell.push_assistant("latest assistant");

    let mut selected = Vec::new();
    shell.move_transcript_selection_up(/*rows*/ 1);
    selected.push(selected_message(&shell));
    shell.move_transcript_selection_up(/*rows*/ 1);
    selected.push(selected_message(&shell));
    shell.move_transcript_selection_up(/*rows*/ 2);
    selected.push(selected_message(&shell));
    shell.move_transcript_selection_up(/*rows*/ 1);
    selected.push(selected_message(&shell));
    shell.move_transcript_selection_down(/*rows*/ 1);
    selected.push(selected_message(&shell));
    shell.move_transcript_selection_down(/*rows*/ 2);
    selected.push(selected_message(&shell));
    shell.move_transcript_selection_down(/*rows*/ 1);
    selected.push(selected_message(&shell));

    assert_eq!(
        selected,
        vec![
            Some((TranscriptKind::User, "third".to_string())),
            Some((TranscriptKind::User, "second".to_string())),
            Some((TranscriptKind::User, "first".to_string())),
            Some((TranscriptKind::User, "first".to_string())),
            Some((TranscriptKind::User, "second".to_string())),
            Some((TranscriptKind::User, "third".to_string())),
            Some((TranscriptKind::User, "third".to_string())),
        ]
    );

    shell.select_first_transcript_item();
    assert_eq!(
        selected_message(&shell),
        Some((TranscriptKind::User, "first".to_string()))
    );
    shell.select_latest_transcript_item();
    assert_eq!(
        selected_message(&shell),
        Some((TranscriptKind::User, "third".to_string()))
    );
}

#[test]
fn selection_remains_empty_without_user_messages() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.push_system("system");
    shell.push_assistant("assistant");

    shell.select_first_transcript_item();
    shell.move_transcript_selection_down(/*rows*/ 1);
    shell.move_transcript_selection_up(/*rows*/ 1);
    shell.select_latest_transcript_item();

    assert_eq!(shell.transcript_selection, None);
}
