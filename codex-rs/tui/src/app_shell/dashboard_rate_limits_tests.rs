use super::*;
use pretty_assertions::assert_eq;

#[test]
fn usage_bar_uses_fractional_block_elements_and_clamps_to_ten_segments() {
    let cases = [
        (-10, "░░░░░░░░░░"),
        (0, "░░░░░░░░░░"),
        (1, "▏░░░░░░░░░"),
        (10, "█░░░░░░░░░"),
        (41, "████▏░░░░░"),
        (50, "█████░░░░░"),
        (82, "████████▎░"),
        (95, "█████████▌"),
        (99, "█████████▉"),
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
fn usage_bar_styles_filled_fractional_and_empty_blocks() {
    assert_eq!(
        usage_bar_spans(/*used_percent*/ 82, palette::purple()),
        [
            "████████".fg(palette::purple()),
            "▎".fg(palette::purple()),
            "░".fg(palette::border()),
        ],
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
            let line = rate_limit_lines(&limit, /*width*/ 100)
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
fn narrow_layout_omits_bars_but_keeps_every_quota_percentage() {
    let limit = RateLimitSnapshot {
        limit_id: Some("codex".to_string()),
        limit_name: Some("Codex".to_string()),
        primary: Some(codex_app_server_protocol::RateLimitWindow {
            used_percent: 82,
            window_duration_mins: Some(300),
            resets_at: None,
        }),
        secondary: Some(codex_app_server_protocol::RateLimitWindow {
            used_percent: 18,
            window_duration_mins: Some(10_080),
            resets_at: None,
        }),
        credits: None,
        individual_limit: None,
        plan_type: None,
        rate_limit_reached_type: None,
    };

    assert_eq!(
        rate_limit_lines(&limit, /*width*/ 42)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>(),
        vec!["Codex 82% 5h | 18% 7d".to_string()],
    );
    assert_eq!(
        rate_limit_lines(&limit, /*width*/ 43)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>(),
        vec!["Codex ████████▎░ 82% 5h | █▊░░░░░░░░ 18% 7d".to_string()],
    );
    assert_eq!(
        rate_limit_lines(&limit, /*width*/ 16)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>(),
        vec!["Codex 82% 5h".to_string(), "  18% 7d".to_string(),],
    );
}

#[test]
fn quota_details_wrap_only_below_the_exact_inline_width() {
    let limit = RateLimitSnapshot {
        limit_id: Some("codex".to_string()),
        limit_name: Some("Codex".to_string()),
        primary: Some(codex_app_server_protocol::RateLimitWindow {
            used_percent: 50,
            window_duration_mins: Some(60),
            resets_at: None,
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
        rate_limit_lines(&limit, /*width*/ 40)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>(),
        vec!["Codex █████░░░░░ 50% 1h | spend 75% left".to_string()],
    );
    assert_eq!(
        rate_limit_lines(&limit, /*width*/ 39)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>(),
        vec![
            "Codex █████░░░░░ 50% 1h".to_string(),
            "  spend 75% left".to_string(),
        ],
    );
}
