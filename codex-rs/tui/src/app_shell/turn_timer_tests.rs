use super::super::output_rate::OutputRate;
use super::*;
use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::PlanDeltaNotification;
use codex_app_server_protocol::ReasoningTextDeltaNotification;
use codex_app_server_protocol::ServerNotification;
use pretty_assertions::assert_eq;
use std::time::Duration;

#[test]
fn rate_combines_fragmented_tokens_and_expires_idle_samples() {
    let start = Instant::now();
    let later = start + Duration::from_secs(2);
    let window = Duration::from_secs(3);
    let text = "The HeaderView shows 120 tokens. 한국어도 표시합니다. ";
    let mut whole = OutputRate::default();
    whole.record(start, text);
    let mut fragmented = OutputRate::default();
    for c in text.chars() {
        fragmented.record(start, c.encode_utf8(&mut [0; 4]));
    }
    let first_rate = whole.tokens_per_second(start);
    assert!(first_rate > 0);
    assert_eq!(fragmented.tokens_per_second(start), first_rate);

    let second = "Here is some more output.";
    whole.record(later, second);
    assert_eq!(whole.tokens_per_second(start + window), 2);
    assert_eq!(whole.tokens_per_second(later + window), 0);

    whole.record(later + window, text);
    assert_eq!(whole.tokens_per_second(later + window), first_rate);
}

#[test]
fn one_second_estimates_track_reference_token_counts() {
    // o200k_base counts for each complete string. Allow some estimation error.
    let samples = [
        (
            "The status line updates as the assistant writes. Time spent waiting for tools produces no visible output.",
            19,
        ),
        (
            "fn main() {\n    let values = vec![12, 34, 56];\n    println!(\"{values:?}\");\n}",
            25,
        ),
        ("응답이 도착하면 헤더에 초당 토큰 수가 표시됩니다.", 18),
    ];
    for (text, reference) in samples {
        let start = Instant::now();
        let mut rate = OutputRate::default();
        rate.record(start, text);
        let estimate = rate.tokens_per_second(start + Duration::from_secs(1));
        assert!(
            estimate.abs_diff(reference) <= 3,
            "{text}: {estimate} vs {reference}"
        );
    }
}

#[test]
fn rate_counts_streamed_output_only_for_the_active_turn() {
    let mut shell = ShellState::snapshot_fixture();
    let thread_id = shell.thread_id.to_string();
    shell.record_active_turn_started("turn-1".into());
    let message = "Here is the next message. ";
    let plan = "Plan the next step.";
    let reasoning = "Compare the two durations.";
    for (turn, thread) in [
        ("old", thread_id.as_str()),
        ("turn-1", "other"),
        ("turn-1", thread_id.as_str()),
    ] {
        shell.handle_notification(ServerNotification::AgentMessageDelta(
            AgentMessageDeltaNotification {
                thread_id: thread.into(),
                turn_id: turn.into(),
                item_id: "message".into(),
                delta: message.into(),
            },
        ));
    }
    shell.handle_notification(ServerNotification::PlanDelta(PlanDeltaNotification {
        thread_id: thread_id.clone(),
        turn_id: "turn-1".into(),
        item_id: "plan".into(),
        delta: plan.into(),
    }));
    shell.handle_notification(ServerNotification::ReasoningTextDelta(
        ReasoningTextDeltaNotification {
            thread_id,
            turn_id: "turn-1".into(),
            item_id: "reasoning".into(),
            delta: reasoning.into(),
            content_index: 0,
        },
    ));
    shell.record_active_turn_started("turn-1".into());
    let now = Instant::now();
    let mut expected = OutputRate::default();
    expected.record(now, message);
    expected.record(now, plan);
    expected.record(now, reasoning);
    assert_eq!(
        shell.active_turn_metrics().unwrap().tokens_per_second,
        expected.tokens_per_second(now)
    );

    shell.record_active_turn_started("turn-2".into());
    assert_eq!(shell.active_turn_metrics().unwrap().tokens_per_second, 0);
    shell.clear_active_turn();
    assert_eq!(shell.active_turn_metrics(), None);
}

#[test]
fn tool_wait_holds_three_seconds_of_model_activity() {
    let start = Instant::now();
    let mut rate = OutputRate::default();
    rate.add_tokens(start, 60.0);
    rate.add_tokens(start + Duration::from_secs(1), 30.0);
    let pause = start + Duration::from_secs(2);
    rate.pause(pause);
    rate.pause(pause + Duration::from_secs(1));
    let resume = pause + Duration::from_secs(20);
    assert_eq!(rate.tokens_per_second(resume), 45);
    rate.resume(resume);
    rate.add_tokens(resume, 30.0);
    assert_eq!(rate.tokens_per_second(resume), 60);
    assert_eq!(rate.tokens_per_second(resume + Duration::from_secs(1)), 20);
}

#[test]
fn full_turn_wait_includes_distinct_pauses_and_an_unfinished_wait_only_once() {
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    let mut rate = OutputRate::default();
    rate.pause(at(0));
    rate.resume(at(1));
    assert_eq!(rate.total_tool_wait(at(2)), None);

    let mut rate = OutputRate::default();
    rate.observe_turn_start();
    assert_eq!(rate.total_tool_wait(at(1)), Some(Duration::ZERO));
    rate.pause(at(2));
    rate.pause(at(3));
    rate.resume(at(7));
    rate.resume(at(8));
    rate.observe_turn_start();
    rate.pause(at(10));
    assert_eq!(rate.total_tool_wait(at(13)), Some(Duration::from_secs(8)));
    rate.invalidate_turn_wait();
    rate.resume(at(14));
    assert_eq!(rate.total_tool_wait(at(15)), None);
}
