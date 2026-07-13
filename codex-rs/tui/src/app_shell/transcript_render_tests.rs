use super::*;
use crate::app_shell::ShellState;
use crate::app_shell::ToolBlockStatus;
use crate::app_shell::TranscriptKind;
use pretty_assertions::assert_eq;
use std::path::Path;

const WIDTH: u16 = 80;

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

    assert!(Arc::ptr_eq(first_lines, second_lines));
}

#[test]
fn unchanged_streaming_item_reuses_rendered_lines_until_the_next_delta() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_streaming_assistant_delta("A long in-progress response");
    let first_revision = shell.streaming_assistant_revision;
    let cwd = PathBuf::from(&shell.cwd);
    let mut cache = TranscriptRenderCache::default();

    let first = cache.layout(&shell, WIDTH, &cwd);
    let second = cache.layout(&shell, WIDTH, &cwd);
    assert!(Arc::ptr_eq(
        chunk_lines(&first, first_revision),
        chunk_lines(&second, first_revision)
    ));

    shell.push_streaming_assistant_delta(" with another delta");
    let third = cache.layout(&shell, WIDTH, &cwd);
    assert!(
        third
            .chunks
            .iter()
            .all(|chunk| chunk.revision != first_revision)
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
