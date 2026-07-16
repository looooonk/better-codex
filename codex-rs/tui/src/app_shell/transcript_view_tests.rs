use super::*;
use crate::app_shell::TranscriptLine;
use pretty_assertions::assert_eq;

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
            Some(palette::BORDER)
        );
        assert_eq!(
            buf[(viewport.text_body.right() - 1, y)].style().bg,
            Some(palette::BORDER)
        );
        if indent > 0 {
            assert_ne!(
                buf[(viewport.text_body.x, y)].style().bg,
                Some(palette::BORDER)
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
