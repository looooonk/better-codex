use super::*;
use pretty_assertions::assert_eq;

fn view() -> HeaderView<'static> {
    HeaderView {
        cwd: "/workspace/better-codex",
        status: "ready",
        status_spinner_frame: None,
        turn_metrics: None,
    }
}

#[test]
fn running_status_spinner_rotates_without_changing_width() {
    let frames = (0..STATUS_SPINNER_FRAMES.len())
        .map(|status_spinner_frame| HeaderView {
            status: "thinking",
            status_spinner_frame: Some(status_spinner_frame),
            ..view()
        })
        .map(|view| view.status_line().to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        frames,
        vec![
            "◐ thinking".to_string(),
            "◓ thinking".to_string(),
            "◑ thinking".to_string(),
            "◒ thinking".to_string(),
        ]
    );
    assert!(
        frames
            .iter()
            .all(|frame| unicode_width::UnicodeWidthStr::width(frame.as_str()) == 10)
    );
}

#[test]
fn ready_status_keeps_static_indicator() {
    assert_eq!(view().status_line().to_string(), "● ready");
}

#[test]
fn turn_elapsed_time_keeps_seconds_at_every_scale() {
    assert_eq!(
        [0, 59, 60, 3_599, 3_600, 5_147].map(format_turn_elapsed),
        [
            "0s".to_string(),
            "59s".to_string(),
            "1m 0s".to_string(),
            "59m 59s".to_string(),
            "1h 0m 0s".to_string(),
            "1h 25m 47s".to_string(),
        ]
    );
}

#[test]
fn active_turn_header_shows_rate_and_keeps_timer_in_compact_headers() {
    let view = HeaderView {
        status: "thinking",
        status_spinner_frame: Some(2),
        turn_metrics: Some(TurnMetrics {
            elapsed_seconds: 5_147,
            tokens_per_second: 42,
        }),
        ..view()
    };

    let mut rows = Vec::new();
    for width in [40, 48, 78, 110] {
        let area = Rect::new(0, 0, width, 3);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        let rendered = (area.x..area.right())
            .map(|x| buf[(x, area.y.saturating_add(1))].symbol())
            .collect::<String>();

        assert!(rendered.contains("1h 25m 47s"), "width {width}: {rendered}");
        assert!(rendered.contains("42 tok/s"), "width {width}: {rendered}");
        rows.push(rendered.trim_end().to_string());
    }
    insta::assert_snapshot!(rows.join("\n"));
}
