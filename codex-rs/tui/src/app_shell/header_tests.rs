use super::*;
use pretty_assertions::assert_eq;

fn view() -> HeaderView<'static> {
    HeaderView {
        cwd: "/workspace/better-codex",
        model: "gpt-5-codex",
        reasoning_effort: "high",
        service_tier: "priority",
        status: "ready",
        status_spinner_frame: None,
        turn_elapsed_seconds: None,
        dashboard_visible: true,
        reasoning_ripple: None,
    }
}

#[test]
fn control_hit_targets_match_visible_chips() {
    let area = Rect::new(0, 0, 100, 3);
    let layout = view().control_layout(area).expect("wide header");

    assert_eq!(
        view().control_at(area, layout.dashboard.as_position()),
        Some(HeaderControl::Dashboard)
    );
    assert_eq!(
        view().control_at(
            area,
            Position::new(
                layout.model.expect("model chip").x,
                layout.model.expect("model chip").y
            )
        ),
        Some(HeaderControl::Model)
    );
    assert_eq!(
        view().control_at(
            area,
            Position::new(
                layout.effort.expect("effort chip").x,
                layout.effort.expect("effort chip").y
            )
        ),
        Some(HeaderControl::ReasoningEffort)
    );
    assert_eq!(
        view().control_at(
            area,
            Position::new(
                layout.service_tier.expect("service tier chip").x,
                layout.service_tier.expect("service tier chip").y
            )
        ),
        Some(HeaderControl::ServiceTier)
    );
    assert_eq!(
        view().control_at(
            area,
            Position::new(
                layout.status.expect("wide status").x,
                layout.status.expect("wide status").y
            )
        ),
        None
    );
}

#[test]
fn effort_and_service_tier_chips_share_the_same_accent() {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 3,
    );
    let view = view();
    let layout = view.control_layout(area).expect("wide header");
    let mut buf = Buffer::empty(area);

    view.render(area, /*hovered*/ None, &mut buf);

    assert_eq!(
        (
            buf[layout.effort.expect("effort chip").as_position()]
                .style()
                .fg,
            buf[layout
                .service_tier
                .expect("service tier chip")
                .as_position()]
            .style()
            .fg,
        ),
        (Some(palette::purple()), Some(palette::purple()))
    );
}

#[test]
fn compact_header_keeps_mouse_controls_visible() {
    let layout = view()
        .control_layout(Rect::new(0, 0, 48, 3))
        .expect("compact controls");

    assert_eq!(
        (
            layout.compact_brand,
            layout.model.is_some(),
            layout.effort.is_some(),
            layout.service_tier.is_some(),
        ),
        (true, true, true, false)
    );
}

#[test]
fn ultra_narrow_header_keeps_the_dashboard_button() {
    for width in [20, 24, 28] {
        let area = Rect::new(0, 0, width, 3);
        let view = view();
        let mut buf = Buffer::empty(area);

        view.render(area, /*hovered*/ None, &mut buf);

        let rendered_brand = (area.x..area.right())
            .map(|x| buf[(x, area.y.saturating_add(1))].symbol())
            .collect::<String>();
        assert_eq!(rendered_brand.trim(), "Dashboard  ◆ BC", "width {width}");
        let layout = view.control_layout(area).expect("dashboard button");
        assert_eq!(
            view.control_at(area, layout.dashboard.as_position()),
            Some(HeaderControl::Dashboard),
            "width {width}"
        );
        assert_eq!(
            (
                layout.model,
                layout.effort,
                layout.service_tier,
                layout.status
            ),
            (None, None, None, None),
            "width {width}"
        );
    }
}

#[test]
fn dashboard_button_has_a_stable_two_way_hit_target() {
    let visible = view();
    let hidden = HeaderView {
        dashboard_visible: false,
        ..view()
    };
    for width in [40, 48, 100] {
        let area = Rect::new(0, 0, width, 3);
        let visible_layout = visible.control_layout(area).expect("visible button");
        let hidden_layout = hidden.control_layout(area).expect("hidden button");
        let position = visible_layout.dashboard.as_position();

        assert_eq!(visible_layout, hidden_layout, "width {width}");
        assert_eq!(
            (
                visible.control_at(area, position),
                hidden.control_at(area, position),
            ),
            (
                Some(HeaderControl::Dashboard),
                Some(HeaderControl::Dashboard),
            ),
            "width {width}"
        );
    }

    assert_eq!(
        visible
            .control_layout(Rect::new(0, 0, 50, 3))
            .expect("sidebar header")
            .dashboard,
        hidden
            .control_layout(Rect::new(0, 0, 100, 3))
            .expect("expanded header")
            .dashboard
    );
}

#[test]
fn dashboard_button_states_snapshot() {
    let visible = view();
    let hidden = HeaderView {
        dashboard_visible: false,
        ..view()
    };

    insta::assert_debug_snapshot!([
        ("visible", visible.dashboard_button(/*hovered*/ None),),
        (
            "visible hovered",
            visible.dashboard_button(Some(HeaderControl::Dashboard)),
        ),
        ("hidden", hidden.dashboard_button(/*hovered*/ None),),
        (
            "hidden hovered",
            hidden.dashboard_button(Some(HeaderControl::Dashboard)),
        ),
    ]);
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
fn active_turn_timer_remains_visible_in_compact_headers() {
    let view = HeaderView {
        status: "thinking",
        status_spinner_frame: Some(2),
        turn_elapsed_seconds: Some(5_147),
        ..view()
    };

    for width in [40, 48, 78, 100] {
        let area = Rect::new(0, 0, width, 3);
        let layout = view.control_layout(area).expect("timed header");
        let mut buf = Buffer::empty(area);
        view.render(area, /*hovered*/ None, &mut buf);
        let rendered = (area.x..area.right())
            .map(|x| buf[(x, area.y.saturating_add(1))].symbol())
            .collect::<String>();

        assert!(layout.status.is_some(), "width {width}: {rendered}");
        assert!(rendered.contains("1h 25m 47s"), "width {width}: {rendered}");
    }
}
