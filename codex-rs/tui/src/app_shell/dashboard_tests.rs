use super::*;
use crate::token_usage::TokenUsage;
use pretty_assertions::assert_eq;

#[test]
fn token_counts_use_compact_units_through_trillions() {
    let values = [
        999,
        1_000,
        1_234_567,
        1_700_000_000,
        1_100_000_000_000,
        -1_700_000_000,
    ];

    assert_eq!(
        values.map(format_token_count),
        ["999", "1k", "1.2m", "1.7b", "1.1t", "-1.7b"]
    );
}

#[test]
fn token_panel_renders_billions_and_trillions() {
    let mut shell = ShellState::snapshot_fixture();
    shell.token_usage = TokenUsage {
        input_tokens: 1_700_000_000,
        output_tokens: 1_100_000_000_000,
        ..TokenUsage::default()
    };
    let panel = dashboard_panel(
        &shell,
        /*width*/ 100,
        /*height*/ 100,
        DashboardPanelKind::Tokens,
    )
    .expect("tokens panel");

    insta::assert_snapshot!(
        panel
            .render_lines(/*width*/ 100)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn token_panel_includes_estimated_thread_usage() {
    let mut shell = ShellState::snapshot_fixture();
    shell.thread_usage = Some(codex_app_server_protocol::ThreadUsage {
        thread_id: shell.thread_id.to_string(),
        estimated_usage_credits_micros: 5_200_000,
        estimated_usage_usd_micros: Some(1_824_000),
        groups: Vec::new(),
    });
    let panel = dashboard_panel(
        &shell,
        /*width*/ 100,
        /*height*/ 100,
        DashboardPanelKind::Tokens,
    )
    .expect("tokens panel");

    insta::assert_snapshot!(
        "token_panel_with_thread_usage",
        panel
            .render_lines(/*width*/ 100)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
