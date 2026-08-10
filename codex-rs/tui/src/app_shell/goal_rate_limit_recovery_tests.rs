use super::*;
use codex_app_server_protocol::CreditsSnapshot;
use codex_app_server_protocol::RateLimitReachedType;
use codex_app_server_protocol::RateLimitWindow;
use codex_app_server_protocol::SpendControlLimitSnapshot;
use std::collections::HashMap;

fn available_limits() -> RateLimitSnapshot {
    RateLimitSnapshot {
        limit_id: Some("codex".to_string()),
        limit_name: Some("Codex".to_string()),
        primary: Some(RateLimitWindow {
            used_percent: 20,
            window_duration_mins: Some(300),
            resets_at: Some(1_900_000_000),
        }),
        secondary: Some(RateLimitWindow {
            used_percent: 40,
            window_duration_mins: Some(10_080),
            resets_at: Some(1_900_000_000),
        }),
        credits: Some(CreditsSnapshot {
            has_credits: true,
            unlimited: false,
            balance: Some("10.00".to_string()),
        }),
        individual_limit: Some(SpendControlLimitSnapshot {
            limit: "$100.00".to_string(),
            used: "$25.00".to_string(),
            remaining_percent: 75,
            resets_at: 1_900_000_000,
        }),
        plan_type: None,
        rate_limit_reached_type: None,
    }
}

fn available_response() -> GetAccountRateLimitsResponse {
    GetAccountRateLimitsResponse {
        rate_limits: available_limits(),
        rate_limits_by_limit_id: None,
        rate_limit_reset_credits: None,
    }
}

#[test]
fn available_canonical_limits_allow_goal_recovery() {
    assert!(rate_limits_allow_goal_resume(
        &available_response(),
        "gpt-5-codex"
    ));
}

#[test]
fn exhausted_or_depleted_limits_keep_the_goal_stopped() {
    let cases = [
        RateLimitSnapshot {
            primary: Some(RateLimitWindow {
                used_percent: 100,
                ..available_limits()
                    .primary
                    .expect("primary limit should exist")
            }),
            ..available_limits()
        },
        RateLimitSnapshot {
            rate_limit_reached_type: Some(RateLimitReachedType::RateLimitReached),
            ..available_limits()
        },
        RateLimitSnapshot {
            credits: Some(CreditsSnapshot {
                has_credits: false,
                unlimited: false,
                balance: None,
            }),
            ..available_limits()
        },
        RateLimitSnapshot {
            individual_limit: Some(SpendControlLimitSnapshot {
                remaining_percent: 0,
                ..available_limits()
                    .individual_limit
                    .expect("individual limit should exist")
            }),
            ..available_limits()
        },
        RateLimitSnapshot {
            primary: None,
            secondary: None,
            ..available_limits()
        },
    ];

    assert!(
        cases
            .iter()
            .all(|limits| !rate_limit_snapshot_allows_goal_resume(limits))
    );
}

#[test]
fn exhausted_current_model_limit_keeps_the_goal_stopped() {
    let model = "gpt-5-codex";
    let mut response = available_response();
    response.rate_limits_by_limit_id = Some(HashMap::from([(
        model.to_string(),
        RateLimitSnapshot {
            primary: Some(RateLimitWindow {
                used_percent: 100,
                ..available_limits()
                    .primary
                    .expect("primary limit should exist")
            }),
            ..available_limits()
        },
    )]));

    assert!(!rate_limits_allow_goal_resume(&response, model));
}
