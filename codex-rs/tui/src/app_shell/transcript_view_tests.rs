use super::*;
use crate::app_shell::TranscriptLine;
use pretty_assertions::assert_eq;

#[test]
fn output_card_renders_crlf_progress_and_tabs_as_terminal_text() {
    let rendered = tool_block_lines(
        TranscriptKind::Output,
        "left\tright\r\nprogress 10%\rprogress 100%\n",
        /*width*/ 48,
        ToolBlockStatus::Success,
        /*selected*/ false,
    )
    .into_iter()
    .map(|line| {
        line.line
            .spans
            .into_iter()
            .map(|span| span.content)
            .collect::<String>()
            .trim_end()
            .to_string()
    })
    .collect::<Vec<_>>()
    .join("\n");

    insta::assert_snapshot!(rendered);
}

#[test]
fn output_hit_testing_tracks_the_scrolled_render_viewport() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    for index in 0..8 {
        shell.push_output_with_status_for_item(
            format!("exec-{index}"),
            format!("command output {index}"),
            ToolBlockStatus::Running,
        );
    }
    shell.transcript_scroll = 4;
    let area = Rect::new(
        /*x*/ 7, /*y*/ 3, /*width*/ 54, /*height*/ 10,
    );
    let viewport = transcript_viewport(&shell, area);
    let visible_to = viewport
        .visible_from
        .saturating_add(viewport.visible_count)
        .min(viewport.layout.total_lines);
    let (logical_row, transcript_index) = (viewport.visible_from..visible_to)
        .find_map(|row| {
            let index = viewport.layout.transcript_index_at_row(row)?;
            (shell.transcript[index].kind == TranscriptKind::Output).then_some((row, index))
        })
        .expect("a scrolled output card should be visible");
    let output_indent = u16::try_from(OUTPUT_BLOCK_INDENT).expect("output indent fits in u16");
    let position = Position::new(
        viewport.text_body.x.saturating_add(output_indent),
        viewport.text_body.y.saturating_add(
            u16::try_from(logical_row.saturating_sub(viewport.visible_from))
                .expect("visible row fits in u16"),
        ),
    );

    assert!(viewport.scrollbar.is_some());
    assert_eq!(
        transcript_card_at(&shell, area, position),
        Some(TranscriptCardHit::ToolOutput { transcript_index })
    );
    assert_eq!(
        transcript_output_at(&shell, area, position),
        Some(transcript_index)
    );
    assert_eq!(
        transcript_output_at(
            &shell,
            area,
            Position::new(position.x.saturating_sub(1), position.y),
        ),
        None
    );
    assert_eq!(
        transcript_output_at(
            &shell,
            area,
            Position::new(viewport.text_body.right(), position.y),
        ),
        None
    );
}

#[test]
fn diff_hit_testing_tracks_the_scrolled_render_viewport_and_full_width() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    for index in 0..8 {
        shell.push_diff_with_status_for_item(
            format!("patch-{index}"),
            format!("1 files +1 -1\n  M src/file-{index}.rs"),
            ToolBlockStatus::Success,
        );
    }
    shell.transcript_scroll = 4;
    let area = Rect::new(
        /*x*/ 7, /*y*/ 3, /*width*/ 54, /*height*/ 10,
    );
    let viewport = transcript_viewport(&shell, area);
    let visible_to = viewport
        .visible_from
        .saturating_add(viewport.visible_count)
        .min(viewport.layout.total_lines);
    let (logical_row, transcript_index) = (viewport.visible_from..visible_to)
        .find_map(|row| {
            let index = viewport.layout.transcript_index_at_row(row)?;
            (shell.transcript[index].kind == TranscriptKind::Diff).then_some((row, index))
        })
        .expect("a scrolled diff card should be visible");
    let y = viewport.text_body.y.saturating_add(
        u16::try_from(logical_row.saturating_sub(viewport.visible_from))
            .expect("visible row fits in u16"),
    );

    assert!(viewport.scrollbar.is_some());
    for x in [viewport.text_body.x, viewport.text_body.right() - 1] {
        assert_eq!(
            transcript_card_at(&shell, area, Position::new(x, y)),
            Some(TranscriptCardHit::Diff { transcript_index })
        );
    }
    assert_eq!(
        transcript_card_at(&shell, area, Position::new(viewport.text_body.right(), y),),
        None
    );
    assert_eq!(
        transcript_output_at(&shell, area, Position::new(viewport.text_body.x, y)),
        None
    );
}

#[test]
fn card_hover_uses_each_card_indent() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_diff_with_status_for_item(
        "patch-1",
        "1 files +1 -1\n  M src/lib.rs",
        ToolBlockStatus::Success,
    );
    shell.push_output_with_status_for_item("exec-1", "command output", ToolBlockStatus::Success);
    let area = Rect::new(
        /*x*/ 2, /*y*/ 3, /*width*/ 60, /*height*/ 14,
    );
    let viewport = transcript_viewport(&shell, area);

    for (transcript_index, indent) in [(0, 0), (1, OUTPUT_BLOCK_INDENT)] {
        let rows = viewport
            .layout
            .transcript_row_range(transcript_index)
            .expect("card should have rendered rows");
        let y = viewport.text_body.y.saturating_add(
            u16::try_from(rows.start.saturating_sub(viewport.visible_from))
                .expect("visible row fits in u16"),
        );
        let indent = u16::try_from(indent).expect("card indent fits in u16");
        let hover = Position::new(viewport.text_body.x.saturating_add(indent), y);
        let mut buf = Buffer::empty(area);

        render_transcript(&shell, area, Some(hover), &mut buf);

        assert_eq!(
            buf[(viewport.text_body.x.saturating_add(indent), y)]
                .style()
                .bg,
            Some(palette::border())
        );
        assert_eq!(
            buf[(viewport.text_body.right() - 1, y)].style().bg,
            Some(palette::border())
        );
        if indent > 0 {
            assert_ne!(
                buf[(viewport.text_body.x, y)].style().bg,
                Some(palette::border())
            );
        }
    }
}

#[test]
fn output_hit_testing_ignores_non_card_rows_and_empty_viewport_space() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_assistant("regular transcript row");
    shell.push_line(TranscriptLine::new(
        TranscriptKind::Output,
        "output without card status",
    ));
    let area = Rect::new(
        /*x*/ 2, /*y*/ 4, /*width*/ 60, /*height*/ 14,
    );
    let viewport = transcript_viewport(&shell, area);
    let output_indent = u16::try_from(OUTPUT_BLOCK_INDENT).expect("output indent fits in u16");
    let x = viewport.text_body.x.saturating_add(output_indent);

    for logical_row in 0..viewport.layout.total_lines {
        let y = viewport.text_body.y.saturating_add(
            u16::try_from(logical_row.saturating_sub(viewport.visible_from))
                .expect("visible row fits in u16"),
        );
        assert_eq!(
            transcript_output_at(&shell, area, Position::new(x, y)),
            None
        );
    }
    assert_eq!(
        transcript_output_at(
            &shell,
            area,
            Position::new(x, viewport.text_body.bottom().saturating_sub(1)),
        ),
        None
    );
}

#[test]
fn hyperlink_hit_testing_resolves_visible_markdown_destinations() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_assistant("Open [the docs](https://example.com/reference) for details.");
    let area = Rect::new(
        /*x*/ 7, /*y*/ 3, /*width*/ 54, /*height*/ 10,
    );
    let viewport = transcript_viewport(&shell, area);
    let (row, hyperlink) = (viewport.visible_from
        ..viewport.visible_from.saturating_add(viewport.visible_count))
        .find_map(|row| {
            let line = viewport.layout.row_at(row)?.line()?;
            line.hyperlinks.first().map(|hyperlink| (row, hyperlink))
        })
        .expect("the Markdown link should be visible");
    let y = viewport.text_body.y.saturating_add(
        u16::try_from(row.saturating_sub(viewport.visible_from)).expect("row should fit"),
    );
    let start = viewport
        .text_body
        .x
        .saturating_add(u16::try_from(hyperlink.columns.start).expect("column should fit"));
    let end = viewport
        .text_body
        .x
        .saturating_add(u16::try_from(hyperlink.columns.end).expect("column should fit"));

    assert_eq!(
        transcript_hyperlink_at(&shell, area, Position::new(start, y)),
        Some("https://example.com/reference".to_string())
    );
    assert_eq!(
        transcript_hyperlink_at(&shell, area, Position::new(end.saturating_sub(1), y)),
        Some("https://example.com/reference".to_string())
    );
    assert_eq!(
        transcript_hyperlink_at(&shell, area, Position::new(end, y)),
        None
    );
    assert_eq!(
        transcript_hyperlink_at(
            &shell,
            area,
            Position::new(start, viewport.text_body.y.saturating_sub(1)),
        ),
        None
    );
}

#[test]
fn text_hit_testing_resolves_scrolled_wide_graphemes_and_rejects_chrome() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    for index in 0..8 {
        shell.push_assistant(format!("message {index} A界e\u{301}Z"));
    }
    shell.transcript_scroll = 3;
    let area = Rect::new(
        /*x*/ 7, /*y*/ 3, /*width*/ 54, /*height*/ 10,
    );
    let viewport = transcript_viewport(&shell, area);
    let visible_end = viewport
        .visible_from
        .saturating_add(viewport.visible_count)
        .min(viewport.layout.total_lines);
    let (row, text) = (viewport.visible_from..visible_end)
        .find_map(|row| {
            let line = viewport.layout.row_at(row)?.line()?;
            let text = rendered_line_text(line);
            text.contains('界').then_some((row, text))
        })
        .expect("a wide grapheme row should be visible");
    let byte = text.find('界').expect("wide grapheme should exist");
    let column = text[..byte].width();
    let position = Position::new(
        viewport
            .text_body
            .x
            .saturating_add(u16::try_from(column + 1).expect("column should fit")),
        viewport.text_body.y.saturating_add(
            u16::try_from(row.saturating_sub(viewport.visible_from)).expect("row should fit"),
        ),
    );

    assert_eq!(
        transcript_text_hit_at(&shell, area, position),
        Some(VisualGraphemeHit::new(row, column, /*width*/ 2))
    );
    assert_eq!(
        transcript_text_hit_at(
            &shell,
            area,
            Position::new(viewport.text_body.x, viewport.text_body.y.saturating_sub(1)),
        ),
        None
    );
    assert_eq!(
        transcript_text_hit_at(&shell, area, Position::new(area.x, position.y),),
        None
    );
    assert_eq!(
        transcript_text_hit_at(
            &shell,
            area,
            Position::new(viewport.text_body.right(), position.y),
        ),
        None
    );

    let blank_y = viewport.text_body.y.saturating_add(
        u16::try_from(
            viewport
                .layout
                .total_lines
                .saturating_sub(viewport.visible_from),
        )
        .unwrap_or(u16::MAX),
    );
    if blank_y < viewport.text_body.bottom() {
        assert_eq!(
            transcript_text_hit_at(&shell, area, Position::new(viewport.text_body.x, blank_y),),
            None
        );
    }

    assert!(viewport.scrollbar.is_some());
    assert_eq!(
        transcript_text_hit_at(
            &shell,
            area,
            Position::new(viewport.body.right().saturating_sub(1), position.y),
        ),
        None
    );

    let mut short_shell = ShellState::snapshot_fixture();
    short_shell.transcript.clear();
    short_shell.clear_streaming_assistant();
    short_shell.push_assistant("only rendered row");
    let short_viewport = transcript_viewport(&short_shell, area);
    let empty_row = short_viewport.text_body.y.saturating_add(1);
    assert!(empty_row < short_viewport.text_body.bottom());
    assert_eq!(
        transcript_text_hit_at(
            &short_shell,
            area,
            Position::new(short_viewport.text_body.x, empty_row),
        ),
        None
    );
}

#[test]
fn selected_text_normalizes_reverse_drag_and_preserves_blank_rows() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_user("alpha 界 omega");
    shell.push_assistant("beta e\u{301} tail");
    let area = Rect::new(
        /*x*/ 4, /*y*/ 2, /*width*/ 64, /*height*/ 12,
    );
    let viewport = transcript_viewport(&shell, area);
    let user_text = rendered_line_text(
        viewport
            .layout
            .row_at(0)
            .and_then(TranscriptLayoutRow::line)
            .expect("user row should be rendered"),
    );
    let assistant_text = rendered_line_text(
        viewport
            .layout
            .row_at(2)
            .and_then(TranscriptLayoutRow::line)
            .expect("assistant row should be rendered"),
    );
    assert!(matches!(
        viewport.layout.row_at(1),
        Some(TranscriptLayoutRow::Blank)
    ));
    let user_column = user_text[..user_text.find('界').expect("wide grapheme")].width();
    let assistant_byte = assistant_text.find("e\u{301}").expect("combined grapheme");
    let assistant_column = assistant_text[..assistant_byte].width();
    let user_hit = grapheme_hit_at(&user_text, /*row*/ 0, user_column).expect("user hit");
    let assistant_hit =
        grapheme_hit_at(&assistant_text, /*row*/ 2, assistant_column).expect("assistant hit");
    let forward = NormalizedVisualRange::from_hits(user_hit, assistant_hit);
    let reverse = NormalizedVisualRange::from_hits(assistant_hit, user_hit);

    assert_eq!(forward, reverse);
    assert_eq!(
        transcript_selected_text(&shell, area, forward),
        Some("界 omega\n\n▎ CODEX  beta e\u{301}".to_string())
    );
}

#[test]
fn selected_text_excludes_visual_continuation_prefixes() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_assistant(
        "Released the alpha.9 source and started CD.\n\n\
         - Version bumped to 0.1.0-alpha.9.\n\
         - Commit pushed: 307e72ba1",
    );
    let area = Rect::new(
        /*x*/ 4, /*y*/ 2, /*width*/ 64, /*height*/ 12,
    );
    let viewport = transcript_viewport(&shell, area);
    let first = rendered_line_text(
        viewport
            .layout
            .row_at(0)
            .and_then(TranscriptLayoutRow::line)
            .expect("first assistant row"),
    );
    let last_row = viewport.layout.total_lines.saturating_sub(1);
    let last_line = viewport
        .layout
        .row_at(last_row)
        .and_then(TranscriptLayoutRow::line)
        .expect("last assistant row");
    let last = rendered_line_text(last_line);
    let anchor = grapheme_hit_at(&first, /*row*/ 0, /*column*/ 0).expect("first label hit");
    let last = trim_synthetic_right_padding(&last);
    let focus = grapheme_hit_at(last, last_row, last.width().saturating_sub(1))
        .expect("last assistant text hit");

    assert_eq!(
        transcript_selected_text(
            &shell,
            area,
            NormalizedVisualRange::from_hits(anchor, focus),
        ),
        Some(
            "▎ CODEX  Released the alpha.9 source and started CD.\n\n\
             - Version bumped to 0.1.0-alpha.9.\n\
             - Commit pushed: 307e72ba1"
                .to_string()
        )
    );

    let last_y = viewport.text_body.y.saturating_add(
        u16::try_from(last_row.saturating_sub(viewport.visible_from)).expect("last row fits"),
    );
    let prefix_end = viewport
        .text_body
        .x
        .saturating_add(u16::try_from(last_line.synthetic_prefix_width).expect("prefix fits"));
    assert_eq!(
        transcript_text_hit_at(
            &shell,
            area,
            Position::new(prefix_end.saturating_sub(1), last_y),
        ),
        None
    );
    assert_eq!(
        transcript_text_hit_at(&shell, area, Position::new(prefix_end, last_y)),
        Some(VisualGraphemeHit::new(
            last_row,
            last_line.synthetic_prefix_width,
            /*width*/ 1,
        ))
    );
}

#[test]
fn selected_text_trims_full_width_card_padding() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_output_with_status_for_item("exec-1", "first\n  second", ToolBlockStatus::Success);
    let area = Rect::new(
        /*x*/ 2, /*y*/ 2, /*width*/ 60, /*height*/ 12,
    );
    let viewport = transcript_viewport(&shell, area);
    let first = rendered_line_text(
        viewport
            .layout
            .row_at(0)
            .and_then(TranscriptLayoutRow::line)
            .expect("first output row"),
    );
    let second = rendered_line_text(
        viewport
            .layout
            .row_at(1)
            .and_then(TranscriptLayoutRow::line)
            .expect("second output row"),
    );
    assert!(first.ends_with(' '));
    assert!(second.ends_with(' '));
    let first_column = first[..first.find("first").expect("first output text")].width();
    let second = trim_synthetic_right_padding(&second);
    let last_column = second.width().saturating_sub(1);
    let anchor = grapheme_hit_at(&first, /*row*/ 0, first_column).expect("first output hit");
    let focus = grapheme_hit_at(second, /*row*/ 1, last_column).expect("second output hit");
    let selection = NormalizedVisualRange::from_hits(anchor, focus);
    let selected = transcript_selected_text(&shell, area, selection).expect("selected text");

    assert_eq!(selected, "first\n  second");
    assert!(
        !selected
            .lines()
            .next()
            .expect("first selected row")
            .ends_with(' ')
    );
}

#[test]
fn text_selection_patches_only_selected_grapheme_cells() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_assistant("select 界e\u{301} text");
    let area = Rect::new(
        /*x*/ 3, /*y*/ 2, /*width*/ 60, /*height*/ 12,
    );
    let viewport = transcript_viewport(&shell, area);
    let line = viewport
        .layout
        .row_at(0)
        .and_then(TranscriptLayoutRow::line)
        .expect("assistant row");
    let text = rendered_line_text(line);
    let start = text[..text.find('界').expect("selected wide grapheme")].width();
    let anchor = grapheme_hit_at(&text, /*row*/ 0, start + 1).expect("selection anchor");
    let focus = grapheme_hit_at(&text, /*row*/ 0, start + 2).expect("selection focus");
    let selection = NormalizedVisualRange::from_hits(anchor, focus);
    let mut buf = Buffer::empty(area);

    render_transcript(&shell, area, /*hover_position*/ None, &mut buf);
    render_transcript_text_selection(&shell, area, selection, &mut buf);

    let y = viewport.text_body.y;
    let selected_cells = (start..start + 3)
        .map(|column| {
            let x = viewport
                .text_body
                .x
                .saturating_add(u16::try_from(column).expect("column should fit"));
            let cell = &buf[(x, y)];
            (cell.symbol().to_string(), cell.style())
        })
        .collect::<Vec<_>>();
    assert!(selected_cells.iter().all(|(_, style)| {
        style.fg == Some(palette::dark()) && style.bg == Some(palette::focus())
    }));
    let before = viewport
        .text_body
        .x
        .saturating_add(u16::try_from(start.saturating_sub(1)).expect("column should fit"));
    assert_ne!(buf[(before, y)].style().bg, Some(palette::focus()));
    insta::assert_debug_snapshot!("transcript_text_selection_cells", selected_cells);
}

#[test]
fn text_selection_does_not_patch_visual_continuation_prefixes() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_assistant("first line\n\nsecond line");
    let area = Rect::new(
        /*x*/ 3, /*y*/ 2, /*width*/ 60, /*height*/ 12,
    );
    let viewport = transcript_viewport(&shell, area);
    let first = rendered_line_text(
        viewport
            .layout
            .row_at(0)
            .and_then(TranscriptLayoutRow::line)
            .expect("first assistant row"),
    );
    let last_row = viewport.layout.total_lines.saturating_sub(1);
    let last = rendered_line_text(
        viewport
            .layout
            .row_at(last_row)
            .and_then(TranscriptLayoutRow::line)
            .expect("last assistant row"),
    );
    let anchor = grapheme_hit_at(&first, /*row*/ 0, /*column*/ 0).expect("first label hit");
    let focus = grapheme_hit_at(&last, last_row, last.width().saturating_sub(1))
        .expect("last assistant hit");
    let selection = NormalizedVisualRange::from_hits(anchor, focus);
    let mut buf = Buffer::empty(area);

    render_transcript(&shell, area, /*hover_position*/ None, &mut buf);
    render_transcript_text_selection(&shell, area, selection, &mut buf);

    let rendered_rows = (0..viewport.layout.total_lines)
        .map(|row| {
            let line = viewport
                .layout
                .row_at(row)
                .and_then(TranscriptLayoutRow::line)
                .expect("assistant row");
            let text = rendered_line_text(line);
            let y = viewport
                .text_body
                .y
                .saturating_add(u16::try_from(row).expect("row fits"));
            let selection_mask = (0..text.width())
                .map(|column| {
                    let x = viewport
                        .text_body
                        .x
                        .saturating_add(u16::try_from(column).expect("column fits"));
                    let style = buf[(x, y)].style();
                    if style.fg == Some(palette::dark()) && style.bg == Some(palette::focus()) {
                        '^'
                    } else {
                        '.'
                    }
                })
                .collect::<String>();
            (text, selection_mask)
        })
        .collect::<Vec<_>>();

    insta::assert_debug_snapshot!(
        "transcript_text_selection_skips_visual_prefixes",
        rendered_rows
    );
}
