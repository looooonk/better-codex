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
        dashboard_visible: true,
    }
}

#[test]
fn control_hit_targets_match_visible_chips() {
    let area = Rect::new(0, 0, 100, 3);
    let layout = view().control_layout(area).expect("wide header");

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
        (Some(palette::PURPLE), Some(palette::PURPLE))
    );
}

#[test]
fn compact_header_keeps_mouse_controls_visible() {
    let layout = view()
        .control_layout(Rect::new(0, 0, 48, 3))
        .expect("compact controls");

    assert!(layout.compact_brand);
}

#[test]
fn ultra_narrow_header_uses_compact_brand_without_controls() {
    for width in [20, 24, 28] {
        let area = Rect::new(0, 0, width, 3);
        let view = view();
        let mut buf = Buffer::empty(area);

        view.render(area, /*hovered*/ None, &mut buf);

        let rendered_brand = (area.x..area.right())
            .map(|x| buf[(x, area.y.saturating_add(1))].symbol())
            .collect::<String>();
        assert_eq!(rendered_brand.trim(), "◆ BC", "width {width}");
        assert_eq!(view.control_layout(area), None, "width {width}");
        for x in area.x..area.right() {
            assert_eq!(
                view.control_at(area, Position::new(x, area.y)),
                None,
                "width {width}, x {x}"
            );
        }
    }
}

#[test]
fn hidden_dashboard_exposes_a_mouse_restore_control() {
    let view = HeaderView {
        dashboard_visible: false,
        ..view()
    };
    let area = Rect::new(0, 0, 48, 3);
    let layout = view.control_layout(area).expect("restore control");
    let dashboard = layout.dashboard.expect("dashboard control");

    assert_eq!(
        view.control_at(area, Position::new(dashboard.x, dashboard.y)),
        Some(HeaderControl::Dashboard)
    );
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
