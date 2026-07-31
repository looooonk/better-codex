//! Cached, viewport-bounded layout for the app-shell transcript.

use super::ShellState;
use super::ToolBlockStatus;
use super::TranscriptKind;
use super::transcript_view::render_transcript_line;
use crate::terminal_hyperlinks::HyperlinkLine;
use codex_config::types::TuiAppTheme;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

const MAX_RENDER_VARIANTS_PER_ITEM: usize = 4;
const MAX_LAYOUT_VARIANTS: usize = 4;

/// Width-aware rendered transcript state retained across terminal frames.
///
/// Completed items are keyed by their stable render revision. Each item keeps a
/// small set of variants so the normal content and scrollbar widths can coexist.
/// Complete layouts are also retained by their ordered revisions, width,
/// working directory, selection, and app theme so unchanged frames avoid
/// rebuilding every chunk. Both caches are bounded to prevent resize,
/// directory, or theme changes from growing them without limit.
#[derive(Default)]
pub(super) struct TranscriptRenderCache {
    items: HashMap<u64, CachedTranscriptItem>,
    layouts: VecDeque<CachedTranscriptLayout>,
}

impl TranscriptRenderCache {
    pub(super) fn layout(
        &mut self,
        shell: &ShellState,
        width: u16,
        cwd: &Path,
    ) -> Arc<TranscriptLayout> {
        if let Some(index) = self
            .layouts
            .iter()
            .position(|cached| cached.matches(shell, width, cwd))
            && let Some(cached) = self.layouts.remove(index)
        {
            let layout = Arc::clone(&cached.layout);
            self.layouts.push_back(cached);
            return layout;
        }

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
                    transcript_index: Some(index),
                    revision: item.render_revision,
                    kind: item.kind,
                    text: &item.text,
                    tool_status: item.tool_status,
                    selected,
                },
                CachedRenderContext {
                    width,
                    cwd,
                    app_theme: shell.app_theme,
                },
            );
        }

        if !shell.streaming_plan.is_empty() {
            push_cached_chunk(
                &mut chunks,
                &mut previous_kind,
                &mut previous_items,
                &mut current_items,
                CachedSource {
                    transcript_index: None,
                    revision: shell.streaming_plan_revision,
                    kind: TranscriptKind::Plan,
                    text: &shell.streaming_plan,
                    tool_status: None,
                    selected: false,
                },
                CachedRenderContext {
                    width,
                    cwd,
                    app_theme: shell.app_theme,
                },
            );
        }
        if !shell.streaming_assistant.is_empty() {
            push_cached_chunk(
                &mut chunks,
                &mut previous_kind,
                &mut previous_items,
                &mut current_items,
                CachedSource {
                    transcript_index: None,
                    revision: shell.streaming_assistant_revision,
                    kind: TranscriptKind::Assistant,
                    text: &shell.streaming_assistant,
                    tool_status: None,
                    selected: false,
                },
                CachedRenderContext {
                    width,
                    cwd,
                    app_theme: shell.app_theme,
                },
            );
        }

        self.items = current_items;
        let layout = Arc::new(TranscriptLayout::new(chunks));
        self.layouts.push_back(CachedTranscriptLayout {
            width,
            cwd: cwd.to_path_buf(),
            selected: shell.transcript_selection,
            app_theme: shell.app_theme,
            revisions: render_revisions(shell).collect(),
            layout: Arc::clone(&layout),
        });
        while self.layouts.len() > MAX_LAYOUT_VARIANTS {
            self.layouts.pop_front();
        }
        layout
    }

    pub(super) fn clear(&mut self) {
        self.items.clear();
        self.layouts.clear();
    }
}

struct CachedTranscriptLayout {
    width: u16,
    cwd: PathBuf,
    selected: Option<usize>,
    app_theme: TuiAppTheme,
    revisions: Vec<u64>,
    layout: Arc<TranscriptLayout>,
}

impl CachedTranscriptLayout {
    fn matches(&self, shell: &ShellState, width: u16, cwd: &Path) -> bool {
        self.width == width
            && self.cwd == cwd
            && self.selected == shell.transcript_selection
            && self.app_theme == shell.app_theme
            && self.revisions.iter().copied().eq(render_revisions(shell))
    }
}

fn render_revisions(shell: &ShellState) -> impl Iterator<Item = u64> + '_ {
    shell
        .transcript
        .iter()
        .map(|item| item.render_revision)
        .chain((!shell.streaming_plan.is_empty()).then_some(shell.streaming_plan_revision))
        .chain(
            (!shell.streaming_assistant.is_empty()).then_some(shell.streaming_assistant_revision),
        )
}

struct CachedSource<'a> {
    transcript_index: Option<usize>,
    revision: u64,
    kind: TranscriptKind,
    text: &'a str,
    tool_status: Option<ToolBlockStatus>,
    selected: bool,
}

#[derive(Clone, Copy)]
struct CachedRenderContext<'a> {
    width: u16,
    cwd: &'a Path,
    app_theme: TuiAppTheme,
}

fn push_cached_chunk(
    chunks: &mut Vec<TranscriptChunk>,
    previous_kind: &mut Option<TranscriptKind>,
    previous_items: &mut HashMap<u64, CachedTranscriptItem>,
    current_items: &mut HashMap<u64, CachedTranscriptItem>,
    source: CachedSource<'_>,
    context: CachedRenderContext<'_>,
) {
    let separator_before = should_separate_transcript_item(*previous_kind, source.kind);
    let mut cached = previous_items.remove(&source.revision).unwrap_or_default();
    let lines = cached.lines(
        &source,
        context.width,
        context.cwd,
        source.selected,
        context.app_theme,
    );
    chunks.push(TranscriptChunk {
        transcript_index: source.transcript_index,
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
    if previous_kind == TranscriptKind::Audit || current_kind == TranscriptKind::Audit {
        return true;
    }
    if matches!(
        previous_kind,
        TranscriptKind::System | TranscriptKind::Separator
    ) || current_kind == TranscriptKind::Separator
    {
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
        app_theme: TuiAppTheme,
    ) -> Arc<[HyperlinkLine]> {
        if let Some(index) = self.variants.iter().position(|variant| {
            variant.width == width
                && variant.cwd.as_path() == cwd
                && variant.selected == selected
                && variant.app_theme == app_theme
        }) && let Some(variant) = self.variants.remove(index)
        {
            let lines = Arc::clone(&variant.lines);
            self.variants.push_back(variant);
            return lines;
        }

        let _active_theme = crate::app_theme::activate(app_theme);
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
            app_theme,
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
    app_theme: TuiAppTheme,
    lines: Arc<[HyperlinkLine]>,
}

struct TranscriptChunk {
    transcript_index: Option<usize>,
    #[cfg_attr(not(test), allow(dead_code))]
    revision: u64,
    separator_before: bool,
    lines: Arc<[HyperlinkLine]>,
}

pub(super) struct TranscriptLayout {
    chunks: Vec<TranscriptChunk>,
    pub(super) total_lines: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum TranscriptLayoutRow<'a> {
    Blank,
    Rendered(&'a HyperlinkLine),
}

impl<'a> TranscriptLayoutRow<'a> {
    pub(super) fn line(self) -> Option<&'a HyperlinkLine> {
        match self {
            Self::Blank => None,
            Self::Rendered(line) => Some(line),
        }
    }
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

    /// Return the rendered line at a logical transcript row.
    ///
    /// Deliberate separator rows are represented by [`TranscriptLayoutRow::Blank`]. Completed and
    /// streaming chunks both return their rendered [`HyperlinkLine`].
    pub(super) fn row_at(&self, row: usize) -> Option<TranscriptLayoutRow<'_>> {
        let mut logical_row = 0usize;
        for chunk in &self.chunks {
            if chunk.separator_before {
                if logical_row == row {
                    return Some(TranscriptLayoutRow::Blank);
                }
                logical_row = logical_row.saturating_add(1);
            }

            let chunk_end = logical_row.saturating_add(chunk.lines.len());
            if (logical_row..chunk_end).contains(&row) {
                return chunk
                    .lines
                    .get(row.saturating_sub(logical_row))
                    .map(TranscriptLayoutRow::Rendered);
            }
            logical_row = chunk_end;
            if logical_row > row {
                break;
            }
        }
        None
    }

    /// Return the stored transcript item that owns a rendered logical row.
    ///
    /// Separator rows and streaming-only chunks have no stored transcript
    /// source, so they intentionally return `None`.
    pub(super) fn transcript_index_at_row(&self, row: usize) -> Option<usize> {
        let mut logical_row = 0usize;
        for chunk in &self.chunks {
            if chunk.separator_before {
                if logical_row == row {
                    return None;
                }
                logical_row = logical_row.saturating_add(1);
            }

            let chunk_end = logical_row.saturating_add(chunk.lines.len());
            if (logical_row..chunk_end).contains(&row) {
                return chunk.transcript_index;
            }
            logical_row = chunk_end;
            if logical_row > row {
                break;
            }
        }
        None
    }

    pub(super) fn transcript_row_range(
        &self,
        transcript_index: usize,
    ) -> Option<std::ops::Range<usize>> {
        let mut logical_row = 0usize;
        for chunk in &self.chunks {
            logical_row = logical_row.saturating_add(usize::from(chunk.separator_before));
            let chunk_end = logical_row.saturating_add(chunk.lines.len());
            if chunk.transcript_index == Some(transcript_index) {
                return Some(logical_row..chunk_end);
            }
            logical_row = chunk_end;
        }
        None
    }
}

#[cfg(test)]
#[path = "transcript_render_tests.rs"]
mod tests;
