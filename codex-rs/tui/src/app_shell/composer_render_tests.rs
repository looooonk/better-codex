use super::*;
use pretty_assertions::assert_eq;
use ratatui::style::Modifier;

fn input_area(body_width: u16, body_height: u16) -> Rect {
    Rect::new(
        /*x*/ 10,
        /*y*/ 5,
        body_width.saturating_add(2),
        body_height.saturating_add(3),
    )
}

fn position_in_body(area: Rect, column: u16, row: u16) -> Position {
    let body = body_rect_after_title(pane_content_rect(area));
    Position::new(body.x.saturating_add(column), body.y.saturating_add(row))
}

fn text_hit(grapheme_range: Range<usize>, caret_range: Range<usize>) -> Option<ComposerTextHit> {
    Some(ComposerTextHit {
        grapheme_range,
        caret_range,
    })
}

#[test]
fn unselected_wrapper_preserves_the_existing_rendering() {
    let text = "/goal preserve wrapping and command styles";

    assert_eq!(
        wrapped_composer_lines(text, /*is_empty*/ false, text.len(), /*width*/ 14,),
        wrapped_composer_lines_with_selection(
            text,
            /*is_empty*/ false,
            text.len(),
            /*width*/ 14,
            /*selection*/ None,
        )
    );
}

#[test]
fn selection_styles_source_graphemes_without_styling_indents() {
    let text = "/goal 界e\u{301} tail";
    let selection_end = text.find(" tail").expect("tail should be present");
    let lines = wrapped_composer_lines_with_selection(
        text,
        /*is_empty*/ false,
        text.len(),
        /*width*/ 10,
        Some(1..selection_end),
    );

    let rendered_styles = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| (span.content.to_string(), span.style))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let selection_style = text_selection_style();
    let selected_spans = lines
        .iter()
        .flat_map(|line| line.spans.iter().skip(1))
        .filter(|span| span.style.bg == selection_style.bg)
        .collect::<Vec<_>>();

    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|line| {
        line.spans
            .first()
            .is_some_and(|indent| indent.style.bg != selection_style.bg)
    }));
    assert!(
        selected_spans
            .iter()
            .all(|span| span.style.fg == selection_style.fg)
    );
    assert!(
        selected_spans
            .iter()
            .find(|span| span.content.contains("goal"))
            .expect("selected slash command segment")
            .style
            .add_modifier
            .contains(Modifier::BOLD)
    );
    insta::assert_debug_snapshot!("composer_selected_command_styles", rendered_styles);
}

#[test]
fn selected_shell_sigil_keeps_its_command_emphasis() {
    let lines = wrapped_composer_lines_with_selection(
        "!echo hello",
        /*is_empty*/ false,
        /*cursor*/ 0,
        /*width*/ 20,
        Some(0..1),
    );
    let sigil = lines[0]
        .spans
        .iter()
        .find(|span| span.content == "!")
        .expect("shell sigil span");

    assert_eq!(
        (sigil.style.fg, sigil.style.bg),
        (text_selection_style().fg, text_selection_style().bg)
    );
    assert!(sigil.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn selection_uses_global_offsets_after_logical_newlines() {
    let text = "first\nsecond";
    let lines = wrapped_composer_lines_with_selection(
        text,
        /*is_empty*/ false,
        text.len(),
        /*width*/ 20,
        Some("first\n".len()..text.len()),
    );

    assert_eq!(lines.len(), 2);
    assert!(
        lines[0]
            .spans
            .iter()
            .all(|span| span.style.bg != text_selection_style().bg)
    );
    assert_eq!(
        lines[1]
            .spans
            .iter()
            .find(|span| span.content == "second")
            .map(|span| span.style),
        Some(text_selection_style())
    );
}

#[test]
fn inside_hit_resolves_indent_eol_spaces_wide_and_combining_graphemes() {
    let area = input_area(/*body_width*/ 18, /*body_height*/ 4);
    let text = "A界e\u{301} Z";
    let hit = |column| {
        composer_text_hit_inside(
            area,
            text,
            text.len(),
            position_in_body(area, column, /*row*/ 0),
        )
    };

    assert_eq!(
        [
            hit(0),
            hit(1),
            hit(2),
            hit(3),
            hit(4),
            hit(5),
            hit(6),
            hit(8),
            hit(17),
        ],
        [
            text_hit(0..0, 0..0),
            text_hit(0..0, 0..0),
            text_hit(0..1, 0..0),
            text_hit(1..4, 1..1),
            text_hit(1..4, 4..4),
            text_hit(4..7, 4..4),
            text_hit(7..8, 7..7),
            text_hit(9..9, 9..9),
            text_hit(9..9, 9..9),
        ]
    );

    let body = body_rect_after_title(pane_content_rect(area));
    assert_eq!(
        [
            composer_text_hit_inside(
                area,
                text,
                text.len(),
                Position::new(body.x.saturating_sub(1), body.y),
            ),
            composer_text_hit_inside(
                area,
                text,
                text.len(),
                Position::new(body.x, body.y.saturating_sub(1)),
            ),
            composer_text_hit_inside(area, text, text.len(), Position::new(body.right(), body.y),),
        ],
        [None, None, None]
    );
}

#[test]
fn hit_ranges_are_global_across_soft_wraps_and_blank_logical_lines() {
    let area = input_area(/*body_width*/ 8, /*body_height*/ 6);
    let text = "abcdefghi\n\n界";
    let hit = |column, row| {
        composer_text_hit_inside(area, text, text.len(), position_in_body(area, column, row))
    };

    assert_eq!(
        [
            hit(2, 0),
            hit(0, 1),
            hit(2, 1),
            hit(5, 1),
            hit(0, 2),
            hit(7, 2),
            hit(2, 3),
            hit(3, 3),
        ],
        [
            text_hit(0..1, 0..0),
            text_hit(6..6, 6..6),
            text_hit(6..7, 6..6),
            text_hit(9..9, 9..9),
            text_hit(10..10, 10..10),
            text_hit(10..10, 10..10),
            text_hit(11..14, 11..11),
            text_hit(11..14, 14..14),
        ]
    );
}

#[test]
fn display_expanded_tab_spaces_remain_individually_hittable() {
    let area = input_area(/*body_width*/ 20, /*body_height*/ 3);
    let text = "a       b";

    assert_eq!(
        composer_text_hit_inside(
            area,
            text,
            text.len(),
            position_in_body(area, /*column*/ 6, /*row*/ 0),
        ),
        text_hit(4..5, 4..4)
    );
}

#[test]
fn viewport_hits_follow_the_same_cursor_scrolling_as_cursor_rendering() {
    let area = input_area(/*body_width*/ 10, /*body_height*/ 3);
    let text = "r0\nr1\nr2\nr3\nr4\nr5";
    let body = body_rect_after_title(pane_content_rect(area));
    let cursor_position = composer_cursor_position(area, text, text.len())
        .expect("cursor should be in the visible viewport");

    assert_eq!(
        cursor_position,
        position_in_body(area, /*column*/ 4, /*row*/ 2)
    );
    assert_eq!(
        [
            composer_text_hit_inside(
                area,
                text,
                text.len(),
                position_in_body(area, /*column*/ 0, /*row*/ 0),
            ),
            composer_text_hit_inside(area, text, text.len(), cursor_position),
            composer_text_hit_clamped_to_visible_viewport(
                area,
                text,
                text.len(),
                Position::new(/*x*/ 0, /*y*/ 0),
            ),
            composer_text_hit_clamped_to_visible_viewport(
                area,
                text,
                text.len(),
                Position::new(u16::MAX, u16::MAX),
            ),
        ],
        [
            text_hit(9..9, 9..9),
            text_hit(17..17, 17..17),
            text_hit(9..9, 9..9),
            text_hit(17..17, 17..17),
        ]
    );

    assert!(body.contains(cursor_position));
    assert_eq!(
        composer_text_hit_clamped_to_visible_viewport(
            area,
            text,
            /*cursor*/ 0,
            Position::new(u16::MAX, u16::MAX),
        ),
        text_hit(8..8, 8..8)
    );
}

#[test]
fn scroll_clipped_soft_wraps_keep_global_hit_ranges() {
    let area = input_area(/*body_width*/ 8, /*body_height*/ 2);
    let text = "abcdefghijklmnopqrstu";

    assert_eq!(
        [
            composer_text_hit_inside(
                area,
                text,
                text.len(),
                position_in_body(area, /*column*/ 2, /*row*/ 0),
            ),
            composer_text_hit_clamped_to_visible_viewport(
                area,
                text,
                text.len(),
                Position::new(/*x*/ 0, /*y*/ 0),
            ),
            composer_text_hit_clamped_to_visible_viewport(
                area,
                text,
                text.len(),
                Position::new(u16::MAX, u16::MAX),
            ),
        ],
        [
            text_hit(12..13, 12..12),
            text_hit(12..12, 12..12),
            text_hit(text.len()..text.len(), text.len()..text.len()),
        ]
    );
    assert_eq!(
        composer_cursor_position(area, text, text.len()),
        Some(position_in_body(area, /*column*/ 5, /*row*/ 1))
    );
}

#[test]
fn clicks_in_blank_body_rows_resolve_against_the_last_visible_line() {
    let area = input_area(/*body_width*/ 12, /*body_height*/ 4);
    let text = "short";

    assert_eq!(
        composer_text_hit_inside(
            area,
            text,
            text.len(),
            position_in_body(area, /*column*/ 11, /*row*/ 3),
        ),
        text_hit(text.len()..text.len(), text.len()..text.len())
    );
}

#[test]
fn empty_or_collapsed_bodies_have_no_text_hits() {
    let area = Rect::new(
        /*x*/ 4, /*y*/ 2, /*width*/ 0, /*height*/ 0,
    );
    let position = Position::new(/*x*/ 4, /*y*/ 2);

    assert_eq!(
        (
            composer_text_hit_inside(area, "", /*cursor*/ 0, position),
            composer_text_hit_clamped_to_visible_viewport(area, "", /*cursor*/ 0, position),
        ),
        (None, None)
    );
}
