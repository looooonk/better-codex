use super::*;
use crate::app_shell::ShellState;
use crate::app_shell::ToolBlockStatus;
use crate::app_shell::TranscriptKind;
use crate::app_theme::TuiAppTheme;
use pretty_assertions::assert_eq;
use std::path::Path;

const WIDTH: u16 = 80;

#[test]
fn grouped_cards_keep_their_indent_even_when_output_arrives_first() {
    let mut shell = ShellState::snapshot_fixture();
    shell.clear_visible_transcript();
    shell.push_assistant("Reply");
    shell.push_output_with_status_for_item("patch", "Change applied", ToolBlockStatus::Success);
    let cwd = PathBuf::from(&shell.cwd);
    let mut cache = TranscriptRenderCache::default();
    let before = cache.layout(&shell, WIDTH, &cwd);
    let output = &chunk_lines(&before, shell.transcript[1].render_revision)[0];
    assert_eq!(output.line.to_string().find('▌'), Some(2));

    shell.push_tool_with_status_for_item(
        "patch",
        "exec check",
        ToolBlockStatus::Success,
        Some(crate::app_shell::tool_groups::ToolAction::Command),
    );
    // The group first appears when its call arrives.
    let start = shell.tool_groups().next().unwrap().rows.start;
    shell.toggle_tool_group(start);
    for width in [WIDTH, 36] {
        let layout = cache.layout(&shell, width, &cwd);
        let rows = layout.visible_hyperlink_lines(/*visible_from*/ 0, layout.total_lines);
        let rail = |label: &str| {
            rows.iter()
                .find_map(|row| row.line.to_string().find(label))
                .expect("label should be visible")
        };
        assert_eq!(rows[0].line.to_string(), "Reply");
        let detail_indent = usize::from(TOOL_DETAIL_INDENT);
        assert_eq!(rail("⌄ ") + 2, detail_indent);
        assert_eq!(rail("▌ tool"), detail_indent);
        assert_eq!(rail("▌ output"), detail_indent + 2);
        assert!(
            rows.iter()
                .all(|row| row.line.width() <= usize::from(width))
        );
    }
}

#[test]
fn unchanged_items_reuse_rendered_lines() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.push_assistant("A completed markdown response with **formatting**.");
    let revision = shell.transcript[0].render_revision;
    let cwd = PathBuf::from(&shell.cwd);
    let mut cache = TranscriptRenderCache::default();

    let first = cache.layout(&shell, WIDTH, &cwd);
    let second = cache.layout(&shell, WIDTH, &cwd);
    let first_lines = &first
        .chunks
        .iter()
        .find(|chunk| chunk.revision == revision)
        .expect("completed item should be laid out")
        .lines;
    let second_lines = &second
        .chunks
        .iter()
        .find(|chunk| chunk.revision == revision)
        .expect("completed item should remain laid out")
        .lines;

    assert!(Arc::ptr_eq(&first, &second));
    assert!(Arc::ptr_eq(first_lines, second_lines));
}

#[test]
fn unchanged_streaming_item_reuses_rendered_lines_until_the_next_delta() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_streaming_assistant_delta("assistant-1", "A long in-progress response");
    let first_revision = shell.streaming_assistant_revision;
    let cwd = PathBuf::from(&shell.cwd);
    let mut cache = TranscriptRenderCache::default();

    let first = cache.layout(&shell, WIDTH, &cwd);
    let second = cache.layout(&shell, WIDTH, &cwd);
    assert!(Arc::ptr_eq(&first, &second));
    assert!(Arc::ptr_eq(
        chunk_lines(&first, first_revision),
        chunk_lines(&second, first_revision)
    ));

    shell.push_streaming_assistant_delta("assistant-1", " with another delta");
    let third = cache.layout(&shell, WIDTH, &cwd);
    assert!(!Arc::ptr_eq(&second, &third));
    assert!(
        third
            .chunks
            .iter()
            .all(|chunk| chunk.revision != first_revision)
    );
}

#[test]
fn complete_layout_cache_tracks_selection() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.push_assistant("Selectable response");
    let cwd = PathBuf::from(&shell.cwd);
    let mut cache = TranscriptRenderCache::default();

    let unselected = cache.layout(&shell, WIDTH, &cwd);
    shell.transcript_selection = Some(0);
    let selected = cache.layout(&shell, WIDTH, &cwd);
    let selected_again = cache.layout(&shell, WIDTH, &cwd);

    assert!(!Arc::ptr_eq(&unselected, &selected));
    assert!(Arc::ptr_eq(&selected, &selected_again));
}

#[test]
fn rendered_line_cache_tracks_app_theme() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.push_user("A prompt whose background follows the app theme.");
    shell.app_theme = TuiAppTheme::GruvboxDark;
    let revision = shell.transcript[0].render_revision;
    let cwd = PathBuf::from(&shell.cwd);
    let mut cache = TranscriptRenderCache::default();
    let gruvbox_surface = {
        let _theme = crate::app_theme::activate(TuiAppTheme::GruvboxDark);
        crate::app_theme::palette().surface
    };
    let catppuccin_surface = {
        let _theme = crate::app_theme::activate(TuiAppTheme::CatppuccinMocha);
        crate::app_theme::palette().surface
    };

    let gruvbox = cache.layout(&shell, WIDTH, &cwd);
    shell.app_theme = TuiAppTheme::CatppuccinMocha;
    let catppuccin = cache.layout(&shell, WIDTH, &cwd);

    assert!(!Arc::ptr_eq(&gruvbox, &catppuccin));
    assert!(!Arc::ptr_eq(
        chunk_lines(&gruvbox, revision),
        chunk_lines(&catppuccin, revision)
    ));
    assert_eq!(
        [
            chunk_lines(&gruvbox, revision)[1].line.style.bg,
            chunk_lines(&catppuccin, revision)[1].line.style.bg,
        ],
        [Some(gruvbox_surface), Some(catppuccin_surface),]
    );
}

#[test]
fn output_delta_invalidates_only_the_changed_item() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.push_assistant("Stable history");
    shell.push_output_with_status_for_item(
        "exec-1",
        "Compiling dependency",
        ToolBlockStatus::Running,
    );
    let assistant_revision = shell.transcript[0].render_revision;
    let output_revision = shell.transcript[1].render_revision;
    let cwd = PathBuf::from(&shell.cwd);
    let mut cache = TranscriptRenderCache::default();
    let first = cache.layout(&shell, WIDTH, &cwd);

    shell.push_output_delta_with_status_for_item(
        "exec-1",
        "\nCompiling workspace",
        ToolBlockStatus::Running,
    );
    let second = cache.layout(&shell, WIDTH, &cwd);

    let first_assistant = chunk_lines(&first, assistant_revision);
    let second_assistant = chunk_lines(&second, assistant_revision);
    assert!(Arc::ptr_eq(first_assistant, second_assistant));
    assert!(
        second
            .chunks
            .iter()
            .all(|chunk| chunk.revision != output_revision)
    );
}

#[test]
fn visible_rows_are_bounded_to_the_viewport() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    for index in 0..20 {
        shell.push_assistant(format!(
            "response {index}: {}",
            "enough wrapped transcript content ".repeat(8)
        ));
    }
    let cwd = Path::new(&shell.cwd);
    let layout = TranscriptRenderCache::default().layout(&shell, /*width*/ 40, cwd);
    let visible_count = 7;
    let visible_from = layout.total_lines.saturating_sub(visible_count + 30);

    let visible = layout.visible_hyperlink_lines(visible_from, visible_count);
    let all = layout.visible_hyperlink_lines(/*visible_from*/ 0, layout.total_lines);

    assert_eq!(visible.len(), visible_count);
    assert_eq!(visible, all[visible_from..visible_from + visible_count]);
}

#[test]
fn logical_rows_resolve_to_stored_transcript_sources() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_assistant("stored response");
    shell.push_output_with_status_for_item(
        "exec-1",
        "first output line\nsecond output line",
        ToolBlockStatus::Running,
    );
    shell.push_streaming_assistant_delta("assistant-1", "streaming response");
    let cwd = Path::new(&shell.cwd);
    let layout = TranscriptRenderCache::default().layout(&shell, WIDTH, cwd);

    let mut expected = Vec::new();
    for chunk in &layout.chunks {
        if chunk.separator_before {
            expected.push(None);
        }
        expected.extend(std::iter::repeat_n(chunk.target, chunk.lines.len()));
    }
    let actual = (0..layout.total_lines)
        .map(|row| layout.target_at_row(row).map(|(target, _)| target))
        .collect::<Vec<_>>();

    assert_eq!(actual, expected);
    assert_eq!(layout.target_at_row(layout.total_lines), None);
    assert_eq!(
        actual
            .iter()
            .filter(|index| **index == Some(TranscriptTarget::Item(0)))
            .count(),
        1
    );
    assert_eq!(
        actual
            .iter()
            .filter(|index| **index == Some(TranscriptTarget::Item(1)))
            .count(),
        2
    );
    assert!(actual.ends_with(&[None, None]));
    assert_eq!(layout.transcript_row_range(0), Some(0..1));
    assert_eq!(layout.transcript_row_range(1), Some(2..4));
    assert_eq!(layout.transcript_row_range(2), None);

    let first = layout
        .row_at(0)
        .and_then(TranscriptLayoutRow::line)
        .expect("stored assistant row should be rendered");
    assert!(first.line.to_string().contains("stored response"));
    assert!(matches!(layout.row_at(1), Some(TranscriptLayoutRow::Blank)));
    let streaming = layout
        .row_at(layout.total_lines.saturating_sub(1))
        .and_then(TranscriptLayoutRow::line)
        .expect("streaming assistant row should be rendered");
    assert!(streaming.line.to_string().contains("streaming response"));
    assert!(layout.row_at(layout.total_lines).is_none());
}

#[test]
fn item_variants_stay_bounded_across_width_and_cwd_changes() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.streaming_assistant.clear();
    shell.push_assistant("cached response");
    let mut cache = TranscriptRenderCache::default();

    for index in 0..12 {
        let cwd = PathBuf::from(format!("/workspace/{index}"));
        cache.layout(&shell, /*width*/ 40 + index, &cwd);
    }

    assert_eq!(cache.items.len(), 1);
    assert_eq!(cache.layouts.len(), MAX_LAYOUT_VARIANTS);
    assert_eq!(
        cache
            .items
            .values()
            .next()
            .expect("cached transcript item")
            .variants
            .len(),
        MAX_RENDER_VARIANTS_PER_ITEM
    );
}

fn chunk_lines(layout: &TranscriptLayout, revision: u64) -> &Arc<[HyperlinkLine]> {
    &layout
        .chunks
        .iter()
        .find(|chunk| chunk.revision == revision)
        .expect("rendered transcript chunk")
        .lines
}

#[test]
fn transcript_line_equality_ignores_render_revision() {
    assert_eq!(
        crate::app_shell::TranscriptLine::new(TranscriptKind::Assistant, "same text"),
        crate::app_shell::TranscriptLine::new(TranscriptKind::Assistant, "same text")
    );
}
