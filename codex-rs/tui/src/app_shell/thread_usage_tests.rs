use super::*;
use pretty_assertions::assert_eq;

#[test]
fn formats_available_estimates_without_float_rounding() {
    let usage = ThreadUsage {
        thread_id: "thread-1".to_string(),
        estimated_usage_credits_micros: 5_200_000,
        estimated_usage_usd_micros: Some(1_824_000),
        groups: Vec::new(),
    };

    assert_eq!(
        thread_usage_line(&usage).map(|line| line.to_string()),
        Some("5.2 credits | ~$1.82".to_string())
    );
}

#[test]
fn omits_unavailable_estimates() {
    let usage = ThreadUsage {
        thread_id: "thread-1".to_string(),
        estimated_usage_credits_micros: 0,
        estimated_usage_usd_micros: None,
        groups: Vec::new(),
    };

    assert_eq!(thread_usage_line(&usage), None);
}

#[test]
fn preserves_positive_sub_cent_estimates() {
    assert_eq!(format_positive_micros(1), "<0.01");
    assert_eq!(format_positive_micros(MICROS_PER_UNIT), "1");
}
