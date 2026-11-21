use super::super::output_rate::OutputRate;
use super::*;
use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::PlanDeltaNotification;
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
fn rate_counts_live_text_and_plans_only_for_the_active_turn() {
    let mut shell = ShellState::snapshot_fixture();
    let thread_id = shell.thread_id.to_string();
    shell.record_active_turn_started("turn-1".into());
    let message = "Here is the next message. ";
    let plan = "Plan the next step.";
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
        thread_id,
        turn_id: "turn-1".into(),
        item_id: "plan".into(),
        delta: plan.into(),
    }));
    shell.record_active_turn_started("turn-1".into());
    let now = Instant::now();
    let mut expected = OutputRate::default();
    expected.record(now, message);
    expected.record(now, plan);
    assert_eq!(
        shell.active_turn_metrics().unwrap().tokens_per_second,
        expected.tokens_per_second(now)
    );

    shell.record_active_turn_started("turn-2".into());
    assert_eq!(shell.active_turn_metrics().unwrap().tokens_per_second, 0);
    shell.clear_active_turn();
    assert_eq!(shell.active_turn_metrics(), None);
}
