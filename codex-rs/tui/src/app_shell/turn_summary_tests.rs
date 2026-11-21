use super::*;
use crate::app_server_session::ThreadTimeline;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadTimelineEntry;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnModelActivity;
use codex_app_server_protocol::TurnModelActivityNotification;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;

#[test]
fn live_and_saved_separators_use_reported_output_over_the_full_turn() {
    let turn = Turn {
        id: "turn".into(),
        root_turn_id: None,
        items: vec![],
        items_view: TurnItemsView::Full,
        status: TurnStatus::Completed,
        error: None,
        started_at: Some(100),
        completed_at: Some(110),
        duration_ms: Some(10_000),
    };
    let expected = "Took 10.00s · 45.6 tok/s";
    let mut live = ShellState::snapshot_fixture();
    live.record_active_turn_started(turn.id.clone());
    live.record_streamed_output("turn", "A short visible reply.");
    live.handle_notification(ServerNotification::TurnModelActivity(
        TurnModelActivityNotification {
            thread_id: live.thread_id.to_string(),
            turn_id: turn.id.clone(),
            activity: TurnModelActivity::Usage { output_tokens: 456 },
        },
    ));
    live.handle_notification(ServerNotification::TurnCompleted(
        TurnCompletedNotification {
            thread_id: live.thread_id.to_string(),
            turn: Turn {
                items: vec![],
                items_view: TurnItemsView::Summary,
                ..turn.clone()
            },
        },
    ));
    assert_eq!(live.transcript.back().unwrap().text, expected);

    let mut saved = ShellState::snapshot_fixture();
    saved.ingest_thread_history(vec![turn.clone()], /*timeline*/ None);
    assert_eq!(saved.transcript.back().unwrap().text, "Took 10.00s");

    // The timeline page starts at completion, but full turn data is also available.
    let mut resumed = ShellState::snapshot_fixture();
    let completion = ThreadTimelineEntry::TurnCompleted {
        position: 42,
        turn_id: turn.id.clone(),
        status: TurnStatus::Completed,
        error: None,
        started_at: turn.started_at,
        completed_at: turn.completed_at,
        duration_ms: None,
        output_tokens: Some(456),
    };
    resumed.ingest_thread_history(
        vec![turn],
        Some(ThreadTimeline {
            entries: vec![completion],
            has_older: true,
        }),
    );
    assert_eq!(resumed.transcript.back().unwrap().text, expected);
}

#[test]
fn partial_history_and_zero_duration_omit_unknown_rate() {
    let mut shell = ShellState::snapshot_fixture();
    shell.push_turn_separator("partial", Some(Duration::from_secs(2)));
    assert_eq!(shell.transcript.back().unwrap().text, "Took 2.00s");
    shell.turn_outputs.insert("zero".into(), 100);
    shell.push_turn_separator("zero", Some(Duration::ZERO));
    assert_eq!(shell.transcript.back().unwrap().text, "Took 0ms");
    shell.push_turn_separator("unknown", /*duration*/ None);
    assert_eq!(shell.transcript.back().unwrap().text, "");
}
