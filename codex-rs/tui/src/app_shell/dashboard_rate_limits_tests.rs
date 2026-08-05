use super::*;
use pretty_assertions::assert_eq;

#[test]
fn usage_bar_uses_only_filled_and_empty_segments_and_clamps_to_ten_segments() {
    let cases = [
        (-10, "░░░░░░░░░░"),
        (0, "░░░░░░░░░░"),
        (1, "░░░░░░░░░░"),
        (10, "█░░░░░░░░░"),
        (41, "████░░░░░░"),
        (50, "█████░░░░░"),
        (82, "████████░░"),
        (95, "█████████░"),
        (99, "█████████░"),
        (100, "██████████"),
        (120, "██████████"),
    ];

    assert_eq!(
        cases.map(|(used_percent, _)| {
            Line::from(Vec::from(usage_bar_spans(used_percent, palette::purple()))).to_string()
        }),
        cases.map(|(_, expected_bar)| expected_bar.to_string()),
    );
}

#[test]
fn usage_bar_renders_exactly_ten_visible_chunks() {
    assert!((-10..=110).all(|used_percent| {
        Line::from(Vec::from(usage_bar_spans(used_percent, palette::purple()))).width()
            == USAGE_BAR_SEGMENTS
    }));
}

#[test]
fn usage_bar_styles_filled_and_empty_blocks() {
    assert_eq!(
        usage_bar_spans(/*used_percent*/ 82, palette::purple()),
        ["████████".fg(palette::purple()), "░░".fg(palette::border()),],
    );
}

#[test]
fn quota_bar_and_percentage_follow_usage_severity_thresholds() {
    let cases = [
        (49, palette::success()),
        (50, palette::cyan()),
        (74, palette::cyan()),
        (75, palette::purple()),
        (89, palette::purple()),
        (90, palette::error()),
    ];

    assert_eq!(
        cases.map(|(used_percent, _)| {
            let limit = RateLimitSnapshot {
                limit_id: Some("codex".to_string()),
                limit_name: Some("Codex".to_string()),
                primary: Some(codex_app_server_protocol::RateLimitWindow {
                    used_percent,
                    window_duration_mins: None,
                    resets_at: None,
                }),
                secondary: None,
                credits: None,
                individual_limit: None,
                plan_type: None,
                rate_limit_reached_type: None,
            };
            let line = rate_limit_lines(
                std::slice::from_ref(&limit),
                /*width*/ 100,
                /*current_time_at*/ 0,
            )
            .into_iter()
            .next()
            .expect("rate limit should render");
            let bar_color = line
                .spans
                .iter()
                .find(|span| span.content.contains('█'))
                .and_then(|span| span.style.fg);
            let percentage_color = line
                .spans
                .iter()
                .find(|span| span.content.contains('%'))
                .and_then(|span| span.style.fg);
            (bar_color, percentage_color)
        }),
        cases.map(|(_, expected_color)| (Some(expected_color), Some(expected_color))),
    );
}

#[test]
fn rows_put_bar_before_padded_percentage_type_and_reset_countdown() {
    const CURRENT_TIME_AT: i64 = 1_900_000_000;
    let limit = RateLimitSnapshot {
        limit_id: Some("gpt-5.3-codex-spark".to_string()),
        limit_name: Some("GPT-5.3-Codex-Spark".to_string()),
        primary: Some(codex_app_server_protocol::RateLimitWindow {
            used_percent: 82,
            window_duration_mins: Some(300),
            resets_at: Some(CURRENT_TIME_AT + 3 * 24 * 60 * 60 + 5 * 60 * 60),
        }),
        secondary: Some(codex_app_server_protocol::RateLimitWindow {
            used_percent: 5,
            window_duration_mins: Some(10_080),
            resets_at: Some(CURRENT_TIME_AT + 5 * 60 * 60),
        }),
        credits: None,
        individual_limit: None,
        plan_type: None,
        rate_limit_reached_type: None,
    };

    assert_eq!(
        rate_limit_lines(
            std::slice::from_ref(&limit),
            /*width*/ 100,
            CURRENT_TIME_AT
        )
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>(),
        vec![
            "████████░░ 82% GPT-5.3-Codex-Spark 3d 5h".to_string(),
            "░░░░░░░░░░ 5%  GPT-5.3-Codex-Spark 5h".to_string(),
        ],
    );
}

#[test]
fn single_digit_percentages_have_only_one_space_before_the_type() {
    let limit = RateLimitSnapshot {
        limit_id: Some("codex".to_string()),
        limit_name: None,
        primary: Some(codex_app_server_protocol::RateLimitWindow {
            used_percent: 5,
            window_duration_mins: None,
            resets_at: None,
        }),
        secondary: Some(codex_app_server_protocol::RateLimitWindow {
            used_percent: 8,
            window_duration_mins: None,
            resets_at: None,
        }),
        credits: None,
        individual_limit: None,
        plan_type: None,
        rate_limit_reached_type: None,
    };

    assert_eq!(
        rate_limit_lines(std::slice::from_ref(&limit), /*width*/ 100, 0)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>(),
        vec![
            "░░░░░░░░░░ 5% codex unknown".to_string(),
            "░░░░░░░░░░ 8% codex unknown".to_string(),
        ],
    );
}

#[test]
fn reset_countdown_is_truncated_to_hours_and_handles_missing_or_elapsed_resets() {
    const CURRENT_TIME_AT: i64 = 1_900_000_000;

    assert_eq!(
        [
            format_time_left(
                Some(CURRENT_TIME_AT + 3 * 24 * 60 * 60 + 5 * 60 * 60 + 59 * 60),
                CURRENT_TIME_AT,
            ),
            format_time_left(Some(CURRENT_TIME_AT + 59 * 60), CURRENT_TIME_AT),
            format_time_left(Some(CURRENT_TIME_AT - 60), CURRENT_TIME_AT),
            format_time_left(None, CURRENT_TIME_AT),
        ],
        [
            "3d 5h".to_string(),
            "0h".to_string(),
            "0h".to_string(),
            "unknown".to_string(),
        ],
    );
}

#[test]
fn quota_details_render_on_a_separate_indented_line() {
    let limit = RateLimitSnapshot {
        limit_id: Some("codex".to_string()),
        limit_name: None,
        primary: Some(codex_app_server_protocol::RateLimitWindow {
            used_percent: 50,
            window_duration_mins: Some(60),
            resets_at: Some(1_900_003_600),
        }),
        secondary: None,
        credits: None,
        individual_limit: Some(codex_app_server_protocol::SpendControlLimitSnapshot {
            limit: "$100.00".to_string(),
            used: "$25.00".to_string(),
            remaining_percent: 75,
            resets_at: 1_900_000_000,
        }),
        plan_type: None,
        rate_limit_reached_type: None,
    };

    assert_eq!(
        rate_limit_lines(
            std::slice::from_ref(&limit),
            /*width*/ 100,
            /*current_time_at*/ 1_900_000_000
        )
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>(),
        vec![
            "█████░░░░░ 50% codex 1h".to_string(),
            "  spend 75% left".to_string(),
        ],
    );
}
