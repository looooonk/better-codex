use super::*;
use crate::app_server_session::ThreadTimeline;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadTimelineEntry;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnModelActivity;
use codex_app_server_protocol::TurnModelActivityNotification;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;
use std::time::Instant;

#[test]
fn live_and_saved_separators_exclude_tool_waits_from_reported_output_rate() {
    let home = tempfile::tempdir().unwrap();
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
    let expected = "Took 10.00s · 114.0 tok/s";
    let mut live = ShellState::snapshot_fixture();
    live.codex_home = home.path().to_owned();
    live.handle_notification(ServerNotification::TurnStarted(TurnStartedNotification {
        thread_id: live.thread_id.to_string(),
        turn: Turn {
            status: TurnStatus::InProgress,
            ..turn.clone()
        },
    }));
    let now = Instant::now();
    live.output_rate.pause(now - Duration::from_secs(7));
    live.output_rate.resume(now - Duration::from_secs(1));
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
    saved.codex_home = home.path().to_owned();
    saved.ingest_thread_history(vec![turn.clone()], /*timeline*/ None);
    assert_eq!(saved.transcript.back().unwrap().text, "Took 10.00s");

    // The timeline page starts at completion, but full turn data is also available.
    let mut resumed = ShellState::snapshot_fixture();
    resumed.codex_home = home.path().to_owned();
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
        vec![turn.clone()],
        Some(ThreadTimeline {
            entries: vec![completion.clone()],
            has_older: true,
        }),
    );
    assert_eq!(resumed.transcript.back().unwrap().text, expected);

    // A client that did not witness this turn has no reliable wait measurement.
    let other_home = tempfile::tempdir().unwrap();
    let mut unwitnessed = ShellState::snapshot_fixture();
    unwitnessed.codex_home = other_home.path().to_owned();
    unwitnessed.ingest_thread_history(
        vec![turn],
        Some(ThreadTimeline {
            entries: vec![completion],
            has_older: true,
        }),
    );
    assert_eq!(unwitnessed.transcript.back().unwrap().text, "Took 10.00s");
    assert_eq!(std::fs::read_dir(other_home.path()).unwrap().count(), 0);
}

#[test]
fn partial_history_and_zero_duration_omit_unknown_rate() {
    let mut shell = ShellState::snapshot_fixture();
    shell.push_turn_separator(
        "partial",
        Some(Duration::from_secs(2)),
        /*observed_wait*/ None,
    );
    assert_eq!(shell.transcript.back().unwrap().text, "Took 2.00s");
    shell.turn_outputs.insert("zero".into(), 100);
    shell.push_turn_separator("zero", Some(Duration::ZERO), /*observed_wait*/ None);
    assert_eq!(shell.transcript.back().unwrap().text, "Took 0ms");
    shell.push_turn_separator(
        "unknown", /*duration*/ None, /*observed_wait*/ None,
    );
    assert_eq!(shell.transcript.back().unwrap().text, "");
}

#[test]
fn ephemeral_turn_has_a_live_rate_without_leaving_timing_files() {
    let home = tempfile::tempdir().unwrap();
    let mut shell = ShellState::snapshot_fixture();
    shell.ephemeral = true;
    shell.codex_home = home.path().to_owned();
    shell.turn_outputs.insert("turn".into(), 400);
    shell.push_turn_separator(
        "turn",
        Some(Duration::from_secs(20)),
        Some(Duration::from_secs(12)),
    );
    assert_eq!(
        shell.transcript.back().unwrap().text,
        "Took 20.00s · 50.0 tok/s"
    );
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}

#[test]
fn mismatched_terminal_usage_and_zero_active_time_omit_the_rate() {
    let home = tempfile::tempdir().unwrap();
    let mut shell = ShellState::snapshot_fixture();
    shell.codex_home = home.path().to_owned();
    let duration = Some(Duration::from_secs(3));
    shell.turn_outputs.insert("turn".into(), 60);
    shell.push_turn_separator("turn", duration, Some(Duration::from_secs(1)));
    assert_eq!(
        shell.transcript.back().unwrap().text,
        "Took 3.00s · 30.0 tok/s"
    );
    shell.turn_outputs.insert("turn".into(), 70);
    shell.push_turn_separator("turn", duration, /*observed_wait*/ None);
    assert_eq!(shell.transcript.back().unwrap().text, "Took 3.00s");
    shell.turn_outputs.insert("turn".into(), 70);
    shell.push_turn_separator("turn", duration, duration);
    assert_eq!(shell.transcript.back().unwrap().text, "Took 3.00s");
}
