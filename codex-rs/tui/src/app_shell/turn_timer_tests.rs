use super::super::output_rate::OutputRate;
use super::*;
use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::PlanDeltaNotification;
use codex_app_server_protocol::ReasoningTextDeltaNotification;
use codex_app_server_protocol::ServerNotification;
use pretty_assertions::assert_eq;
use std::time::Duration;

#[test]
fn buffered_rate_is_independent_of_packet_fragmentation() {
    let start = Instant::now();
    let text = "The HeaderView shows 120 tokens. 한국어도 표시합니다. ";
    let mut whole = OutputRate::default();
    whole.add_tokens(start, 1.0);
    let mut fragmented = OutputRate::default();
    fragmented.add_tokens(start, 1.0);
    let second = start + Duration::from_secs(1);
    let packet = start + Duration::from_millis(50);
    whole.record(packet, text);
    for c in text.chars() {
        fragmented.record(packet, c.encode_utf8(&mut [0; 4]));
    }
    assert_eq!(whole.tokens_per_second(), None);
    // Coalescing within one packet produces the same estimate at the next observation.
    whole.record(second, "more");
    fragmented.record(second, "more");
    assert!(whole.tokens_per_second().is_some_and(|rate| rate > 0));
    assert_eq!(fragmented.tokens_per_second(), whole.tokens_per_second());
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
        rate.add_tokens(start, 1.0);
        rate.record(start + Duration::from_secs(1), text);
        let estimate = rate.tokens_per_second().unwrap();
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
    let start = Instant::now();
    shell.output_rate.add_tokens(start, 1.0);
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
    expected.add_tokens(start, 1.0);
    expected.record(now, message);
    expected.record(now, plan);
    expected.record(now, reasoning);
    let finish = start + Duration::from_secs(1);
    expected.add_tokens(finish, 1.0);
    shell.output_rate.add_tokens(finish, 1.0);
    assert_eq!(shell.active_turn_tps(), expected.tokens_per_second());

    shell.record_active_turn_started("turn-2".into());
    assert_eq!(shell.active_turn_tps(), None);
    shell.clear_active_turn();
    assert_eq!(shell.active_turn_tps(), None);
}

#[test]
fn tool_execution_and_follow_up_prefill_do_not_age_the_live_rate() {
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    let mut rate = OutputRate::default();
    rate.begin_request(at(0));
    rate.add_tokens(at(2), 100.0); // buffered first packet
    rate.add_tokens(at(3), 30.0);
    assert_eq!(rate.tokens_per_second(), Some(30));
    rate.end_request(at(4));
    rate.end_request(at(5));
    rate.record_usage(at(24), 130);
    assert_eq!(rate.tokens_per_second(), Some(30));
    rate.begin_request(at(25));
    rate.add_tokens(at(50), 2.0);
    assert_eq!(rate.tokens_per_second(), Some(30));
    rate.add_tokens(at(51), 30.0);
    assert_eq!(rate.tokens_per_second(), Some(30));
    rate.add_tokens(at(52), 12.0);
    assert!((12..30).contains(&rate.tokens_per_second().unwrap()));
}

#[test]
fn hidden_reasoning_uses_reported_output_and_model_time_excludes_non_request_work() {
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    let mut rate = OutputRate::default();
    rate.observe_turn_start();
    rate.begin_request(at(2));
    rate.end_request(at(4));
    rate.record_usage(at(24), 100);
    assert_eq!(rate.tokens_per_second(), Some(50));
    rate.begin_request(at(25));
    rate.add_tokens(at(28), 8.0);
    assert_eq!(rate.tokens_per_second(), Some(50));
    rate.record_usage(at(29), 100);
    assert_eq!(rate.model_time(at(46), 200), Some(Duration::from_secs(6)));
    assert_eq!(rate.model_time(at(46), 300), None);
    rate.invalidate_timing();
    rate.observe_turn_start();
    assert_eq!(rate.model_time(at(46), 200), None);
}

#[test]
fn delayed_usage_after_follow_up_start_does_not_close_the_new_request() {
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    let mut rate = OutputRate::default();
    rate.observe_turn_start();
    rate.begin_request(at(0));
    rate.end_request(at(2));
    rate.begin_request(at(10));
    rate.record_usage(at(11), 100);
    // Interrupting here has only the earlier completed response's usage.
    assert_eq!(rate.model_time(at(12), 100), Some(Duration::from_secs(2)));
    rate.record_usage(at(14), 200);
    assert_eq!(rate.model_time(at(20), 300), Some(Duration::from_secs(6)));
}

#[test]
fn attaching_mid_turn_does_not_publish_a_partial_turn_rate() {
    let start = Instant::now();
    let mut rate = OutputRate::default();
    rate.begin_request(start);
    rate.record_usage(start + Duration::from_secs(1), 100);
    assert_eq!(rate.model_time(start + Duration::from_secs(2), 100), None);
}
