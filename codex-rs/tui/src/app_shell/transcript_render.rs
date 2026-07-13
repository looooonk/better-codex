//! Cached, viewport-bounded layout for the app-shell transcript.

use super::ShellState;
use super::ToolBlockStatus;
use super::TranscriptKind;
use super::render::render_transcript_line;
use crate::terminal_hyperlinks::HyperlinkLine;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

const MAX_RENDER_VARIANTS_PER_ITEM: usize = 4;

/// Width-aware rendered transcript state retained across terminal frames.
///
/// Completed items are keyed by their stable render revision. Each item keeps a
/// small set of variants so the normal content and scrollbar widths can coexist
/// without letting resize or working-directory changes grow the cache without
/// bound.
#[derive(Default)]
pub(super) struct TranscriptRenderCache {
    items: HashMap<u64, CachedTranscriptItem>,
}

impl TranscriptRenderCache {
    pub(super) fn layout(
        &mut self,
        shell: &ShellState,
        width: u16,
        cwd: &Path,
    ) -> TranscriptLayout {
        let mut previous_items = std::mem::take(&mut self.items);
        let mut current_items = HashMap::with_capacity(
            shell.transcript.len()
                + usize::from(!shell.streaming_plan.is_empty())
                + usize::from(!shell.streaming_assistant.is_empty()),
        );
        let mut chunks = Vec::with_capacity(current_items.capacity());
        let mut previous_kind = None;

        for (index, item) in shell.transcript.iter().enumerate() {
            let selected = shell.transcript_selection == Some(index);
            push_cached_chunk(
                &mut chunks,
                &mut previous_kind,
                &mut previous_items,
                &mut current_items,
                CachedSource {
                    revision: item.render_revision,
                    kind: item.kind,
                    text: &item.text,
                    tool_status: item.tool_status,
                },
                width,
                cwd,
                selected,
            );
        }

        if !shell.streaming_plan.is_empty() {
            push_cached_chunk(
                &mut chunks,
                &mut previous_kind,
                &mut previous_items,
                &mut current_items,
                CachedSource {
                    revision: shell.streaming_plan_revision,
                    kind: TranscriptKind::Plan,
                    text: &shell.streaming_plan,
                    tool_status: None,
                },
                width,
                cwd,
                /*selected*/ false,
            );
        }
        if !shell.streaming_assistant.is_empty() {
            push_cached_chunk(
                &mut chunks,
                &mut previous_kind,
                &mut previous_items,
                &mut current_items,
                CachedSource {
                    revision: shell.streaming_assistant_revision,
                    kind: TranscriptKind::Assistant,
                    text: &shell.streaming_assistant,
                    tool_status: None,
                },
                width,
                cwd,
                /*selected*/ false,
            );
        }

        self.items = current_items;
        TranscriptLayout::new(chunks)
    }

    pub(super) fn clear(&mut self) {
        self.items.clear();
    }
}

struct CachedSource<'a> {
    revision: u64,
    kind: TranscriptKind,
    text: &'a str,
    tool_status: Option<ToolBlockStatus>,
}

fn push_cached_chunk(
    chunks: &mut Vec<TranscriptChunk>,
    previous_kind: &mut Option<TranscriptKind>,
    previous_items: &mut HashMap<u64, CachedTranscriptItem>,
    current_items: &mut HashMap<u64, CachedTranscriptItem>,
    source: CachedSource<'_>,
    width: u16,
    cwd: &Path,
    selected: bool,
) {
    let separator_before = should_separate_transcript_item(*previous_kind, source.kind);
    let mut cached = previous_items.remove(&source.revision).unwrap_or_default();
    let lines = cached.lines(&source, width, cwd, selected);
    chunks.push(TranscriptChunk {
        revision: source.revision,
        separator_before,
        lines,
    });
    current_items.insert(source.revision, cached);
    *previous_kind = Some(source.kind);
}

fn should_separate_transcript_item(
    previous_kind: Option<TranscriptKind>,
    current_kind: TranscriptKind,
) -> bool {
    let Some(previous_kind) = previous_kind else {
        return false;
    };
    if previous_kind == TranscriptKind::System {
        return false;
    }
    matches!(
        current_kind,
        TranscriptKind::User
            | TranscriptKind::Assistant
            | TranscriptKind::Tool
            | TranscriptKind::Diff
            | TranscriptKind::Output
    )
}

#[derive(Default)]
struct CachedTranscriptItem {
    variants: VecDeque<CachedRenderVariant>,
}

impl CachedTranscriptItem {
    fn lines(
        &mut self,
        source: &CachedSource<'_>,
        width: u16,
        cwd: &Path,
        selected: bool,
    ) -> Arc<[HyperlinkLine]> {
        if let Some(index) = self.variants.iter().position(|variant| {
            variant.width == width && variant.cwd.as_path() == cwd && variant.selected == selected
        }) && let Some(variant) = self.variants.remove(index)
        {
            let lines = Arc::clone(&variant.lines);
            self.variants.push_back(variant);
            return lines;
        }

        let lines: Arc<[HyperlinkLine]> = render_transcript_line(
            source.kind,
            source.text,
            source.tool_status,
            width,
            cwd,
            selected,
        )
        .into();
        self.variants.push_back(CachedRenderVariant {
            width,
            cwd: cwd.to_path_buf(),
            selected,
            lines: Arc::clone(&lines),
        });
        while self.variants.len() > MAX_RENDER_VARIANTS_PER_ITEM {
            self.variants.pop_front();
        }
        lines
    }
}

struct CachedRenderVariant {
    width: u16,
    cwd: PathBuf,
    selected: bool,
    lines: Arc<[HyperlinkLine]>,
}

struct TranscriptChunk {
    #[cfg_attr(not(test), allow(dead_code))]
    revision: u64,
    separator_before: bool,
    lines: Arc<[HyperlinkLine]>,
}

pub(super) struct TranscriptLayout {
    chunks: Vec<TranscriptChunk>,
    pub(super) total_lines: usize,
}

impl TranscriptLayout {
    fn new(chunks: Vec<TranscriptChunk>) -> Self {
        let total_lines = chunks
            .iter()
            .map(|chunk| chunk.lines.len() + usize::from(chunk.separator_before))
            .sum();
        Self {
            chunks,
            total_lines,
        }
    }

    /// Clone only the rows that intersect the requested terminal viewport.
    pub(super) fn visible_hyperlink_lines(
        &self,
        visible_from: usize,
        visible_count: usize,
    ) -> Vec<HyperlinkLine> {
        if visible_count == 0 {
            return Vec::new();
        }

        let visible_to = visible_from.saturating_add(visible_count);
        let mut logical_row = 0usize;
        let mut visible = Vec::with_capacity(visible_count);
        for chunk in &self.chunks {
            if chunk.separator_before {
                if (visible_from..visible_to).contains(&logical_row) {
                    visible.push(HyperlinkLine::new(Default::default()));
                }
                logical_row = logical_row.saturating_add(1);
            }

            let chunk_start = logical_row;
            let chunk_end = chunk_start.saturating_add(chunk.lines.len());
            if chunk_end > visible_from && chunk_start < visible_to {
                let take_from = visible_from.saturating_sub(chunk_start);
                let take_to = visible_to
                    .saturating_sub(chunk_start)
                    .min(chunk.lines.len());
                visible.extend(chunk.lines[take_from..take_to].iter().cloned());
            }
            logical_row = chunk_end;
            if logical_row >= visible_to {
                break;
            }
        }
        visible
    }
}

#[cfg(test)]
#[path = "transcript_render_tests.rs"]
mod tests;
