use super::*;
use pretty_assertions::assert_eq;

#[test]
fn transient_zero_thread_usage_preserves_positive_estimates() {
    let mut shell = ShellState::snapshot_fixture();
    shell.record_thread_usage(Some(ThreadUsage {
        thread_id: shell.thread_id.to_string(),
        estimated_usage_credits_micros: 5_200_000,
        estimated_usage_usd_micros: Some(1_824_000),
        groups: Vec::new(),
    }));
    shell.record_thread_usage(Some(ThreadUsage {
        thread_id: shell.thread_id.to_string(),
        estimated_usage_credits_micros: 0,
        estimated_usage_usd_micros: Some(0),
        groups: Vec::new(),
    }));

    assert_eq!(
        shell.thread_usage,
        Some(ThreadUsage {
            thread_id: shell.thread_id.to_string(),
            estimated_usage_credits_micros: 5_200_000,
            estimated_usage_usd_micros: Some(1_824_000),
            groups: Vec::new(),
        })
    );
}

#[test]
fn unavailable_thread_usage_clears_the_estimate() {
    let mut shell = ShellState::snapshot_fixture();
    shell.thread_usage = Some(ThreadUsage {
        thread_id: shell.thread_id.to_string(),
        estimated_usage_credits_micros: 1_000_000,
        estimated_usage_usd_micros: None,
        groups: Vec::new(),
    });

    shell.record_thread_usage(None);

    assert_eq!(shell.thread_usage, None);
}
