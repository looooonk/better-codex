use super::*;
use crate::app_server_session::ThreadTimeline;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadTimelineEntry;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;

#[test]
fn live_and_saved_separators_average_all_visible_output_over_the_full_turn() {
    // Each "one " contributes one estimated token. Twenty tokens of commentary and ten
    // of plan text over ten seconds should give 3 tok/s regardless of chunk timing.
    let reply = ThreadItem::untimed(ThreadItemPayload::AgentMessage {
        id: "reply".into(),
        text: "one ".repeat(20),
        phase: None,
        memory_citation: None,
        delivery: None,
        questions: None,
    });
    let plan = ThreadItem::untimed(ThreadItemPayload::Plan {
        id: "plan".into(),
        text: "one ".repeat(10),
    });
    let turn = Turn {
        id: "turn".into(),
        root_turn_id: None,
        items: vec![reply, plan.clone()],
        items_view: TurnItemsView::Full,
        status: TurnStatus::Completed,
        error: None,
        started_at: Some(100),
        completed_at: Some(110),
        duration_ms: Some(10_000),
    };
    let expected = "Took 10.00s · 3.0 tok/s";
    let mut live = ShellState::snapshot_fixture();
    live.record_active_turn_started(turn.id.clone());
    live.record_streamed_output("turn", "reply", &"one ".repeat(20));
    live.record_streamed_output("turn", "plan", "The provisional plan differs.");
    live.ingest_completed_item_for_turn(
        "turn",
        plan.clone(),
        super::super::CompletedItemOrigin::Live,
        /*rewind_anchor*/ None,
    );
    live.handle_notification(ServerNotification::TurnCompleted(
        TurnCompletedNotification {
            thread_id: live.thread_id.to_string(),
            turn: Turn {
                items: vec![plan],
                items_view: TurnItemsView::Summary,
                ..turn.clone()
            },
        },
    ));
    assert_eq!(live.transcript.back().unwrap().text, expected);

    let mut saved = ShellState::snapshot_fixture();
    saved.ingest_thread_history(vec![turn.clone()], /*timeline*/ None);
    assert_eq!(saved.transcript.back().unwrap().text, expected);

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
    shell
        .turn_outputs
        .insert("zero".into(), TurnOutput::default());
    shell.push_turn_separator("zero", Some(Duration::ZERO));
    assert_eq!(shell.transcript.back().unwrap().text, "Took 0ms");
    shell.push_turn_separator("unknown", /*duration*/ None);
    assert_eq!(shell.transcript.back().unwrap().text, "");
}
