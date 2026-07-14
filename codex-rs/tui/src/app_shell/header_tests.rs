use super::*;

fn view() -> HeaderView<'static> {
    HeaderView {
        cwd: "/workspace/better-codex",
        model: "gpt-5-codex",
        reasoning_effort: "high",
        status: "ready",
        dashboard_visible: true,
    }
}

#[test]
fn control_hit_targets_match_visible_chips() {
    let area = Rect::new(0, 0, 100, 2);
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
                layout.status.expect("wide status").x,
                layout.status.expect("wide status").y
            )
        ),
        None
    );
}

#[test]
fn compact_header_keeps_mouse_controls_visible() {
    let layout = view()
        .control_layout(Rect::new(0, 0, 48, 2))
        .expect("compact controls");

    assert!(layout.compact_brand);
}

#[test]
fn controls_hide_when_even_compact_chips_cannot_fit() {
    assert_eq!(view().control_layout(Rect::new(0, 0, 28, 2)), None);
}

#[test]
fn hidden_dashboard_exposes_a_mouse_restore_control() {
    let view = HeaderView {
        dashboard_visible: false,
        ..view()
    };
    let area = Rect::new(0, 0, 48, 2);
    let layout = view.control_layout(area).expect("restore control");
    let dashboard = layout.dashboard.expect("dashboard control");

    assert_eq!(
        view.control_at(area, Position::new(dashboard.x, dashboard.y)),
        Some(HeaderControl::Dashboard)
    );
}
