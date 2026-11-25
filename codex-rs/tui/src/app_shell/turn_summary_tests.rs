use super::*;
use crate::app_server_session::ThreadTimeline;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadTimelineEntry;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::ThreadTokenUsageUpdatedNotification;
use codex_app_server_protocol::TokenUsageBreakdown;
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
    live.output_rate.begin_request(now - Duration::from_secs(7));
    live.output_rate.end_request(now - Duration::from_secs(3));
    live.record_streamed_output("turn", "A short visible reply.");
    let usage = TokenUsageBreakdown {
        output_tokens: 456,
        reasoning_output_tokens: 450,
        input_tokens: 50,
        cached_input_tokens: 0,
        cache_write_input_tokens: 0,
        total_tokens: 506,
    };
    let update = ThreadTokenUsageUpdatedNotification {
        thread_id: live.thread_id.to_string(),
        turn_id: turn.id.clone(),
        token_usage: ThreadTokenUsage {
            total: TokenUsageBreakdown {
                output_tokens: live.token_usage.output_tokens + usage.output_tokens,
                ..usage.clone()
            },
            last: usage,
            model_context_window: None,
        },
    };
    live.handle_notification(ServerNotification::ThreadTokenUsageUpdated(update.clone()));
    live.handle_notification(ServerNotification::ThreadTokenUsageUpdated(update));
    assert_eq!(live.active_turn_tps(), Some(114));
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
        &codex_app_server_protocol::TurnStatus::Completed,
        Some(Duration::from_secs(2)),
        /*observed_model_time*/ None,
    );
    assert_eq!(shell.transcript.back().unwrap().text, "Took 2.00s");
    shell.turn_outputs.insert("zero".into(), 100);
    shell.push_turn_separator(
        "zero",
        &codex_app_server_protocol::TurnStatus::Completed,
        Some(Duration::ZERO),
        /*observed_model_time*/ None,
    );
    assert_eq!(shell.transcript.back().unwrap().text, "Took 0ms");
    shell.push_turn_separator(
        "unknown",
        &codex_app_server_protocol::TurnStatus::Completed,
        /*duration*/ None,
        /*observed_model_time*/ None,
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
        &codex_app_server_protocol::TurnStatus::Completed,
        Some(Duration::from_secs(20)),
        Some(Duration::from_secs(8)),
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
    shell.push_turn_separator(
        "turn",
        &codex_app_server_protocol::TurnStatus::Completed,
        duration,
        Some(Duration::from_secs(2)),
    );
    assert_eq!(
        shell.transcript.back().unwrap().text,
        "Took 3.00s · 30.0 tok/s"
    );
    shell.turn_outputs.insert("turn".into(), 70);
    shell.push_turn_separator(
        "turn",
        &codex_app_server_protocol::TurnStatus::Completed,
        duration,
        /*observed_model_time*/ None,
    );
    assert_eq!(shell.transcript.back().unwrap().text, "Took 3.00s");
    shell.turn_outputs.insert("turn".into(), 70);
    shell.push_turn_separator(
        "turn",
        &codex_app_server_protocol::TurnStatus::Completed,
        duration,
        Some(Duration::ZERO),
    );
    assert_eq!(shell.transcript.back().unwrap().text, "Took 3.00s");
}

#[test]
fn legacy_timing_files_reproduce_their_original_rate_on_resume() {
    let home = tempfile::tempdir().unwrap();
    let mut shell = ShellState::snapshot_fixture();
    shell.codex_home = home.path().to_owned();
    let elapsed = Duration::from_secs(10);
    let directory = home
        .path()
        .join("better-codex/turn-timings")
        .join(shell.thread_id.to_string());
    std::fs::create_dir_all(&directory).unwrap();
    // The legacy filename is the SHA-256 digest of the turn ID.
    use sha2::Digest;
    let digest = sha2::Sha256::digest(b"turn");
    let path = directory.join(format!("{digest:x}.json"));
    let legacy = serde_json::json!({
        "elapsed": elapsed,
        "output_tokens": 400,
        "tool_wait": Duration::from_secs(6),
    });
    std::fs::write(path, legacy.to_string()).unwrap();
    shell.turn_outputs.insert("turn".into(), 400);
    shell.push_turn_separator(
        "turn",
        &TurnStatus::Completed,
        Some(elapsed),
        /*observed_model_time*/ None,
    );
    assert_eq!(
        shell.transcript.back().unwrap().text,
        "Took 10.00s · 100.0 tok/s"
    );
}
