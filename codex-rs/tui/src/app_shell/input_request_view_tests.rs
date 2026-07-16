use super::*;
use pretty_assertions::assert_eq;
use ratatui::layout::Position;
use ratatui::layout::Rect;

fn request_lines() -> Vec<Line<'static>> {
    vec![
        Line::from("? Run command?"),
        Line::from(vec!["  ".into(), "A command that needs approval".into()]),
        Line::from(vec![
            "  ".into(),
            " Approve ↵ ".into(),
            " ".into(),
            " Deny n ".into(),
            " ".into(),
            " Edit e ".into(),
            " ".into(),
            " Explain ? ".into(),
        ]),
    ]
}

#[test]
fn wrapped_continuations_keep_structural_indent() {
    let segments = wrapped_segments(&request_lines(), /*width*/ 38);
    let action_segments = segments
        .iter()
        .filter(|segment| segment.logical_line == 2)
        .collect::<Vec<_>>();

    assert_eq!(action_segments.len(), 2);
    assert_eq!(line_text(&action_segments[1].content), "  Explain ?");
    assert_eq!(action_segments[1].display_prefix_width, 2);
}

#[test]
fn continuation_hit_maps_after_synthetic_indent() {
    let lines = request_lines();
    let panel = Rect::new(0, 0, 40, 8);
    let body = body_rect_after_title(pane_content_rect(panel));
    let segments = visible_segments(&lines, body.width, body.height);
    let continuation_row = segments
        .iter()
        .position(|segment| line_text(&segment.content).contains("Explain"))
        .expect("action continuation should be visible");
    let action_text = line_text(&lines[2]);
    let explain_byte = action_text
        .find("Explain")
        .expect("source action line should contain Explain");
    let expected_column = UnicodeWidthStr::width(&action_text[..explain_byte]);
    let y = body.y + u16::try_from(continuation_row).expect("row fits in terminal coordinates");

    assert_eq!(
        request_panel_hit(panel, Position::new(body.x + 2, y), &lines),
        Some(RequestPanelHit {
            line: 2,
            column: expected_column,
        })
    );
    assert_eq!(
        request_panel_hit(panel, Position::new(body.x + 1, y), &lines),
        None
    );
}
