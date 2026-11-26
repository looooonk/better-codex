//! Cached, viewport-bounded layout for the app-shell transcript.

use super::ShellState;
use super::ToolBlockStatus;
use super::TranscriptKind;
use super::transcript_view::TranscriptVisuals;
use super::transcript_view::render_transcript_line_with_visualizations;
use crate::app_theme::TuiAppTheme;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::prefix_hyperlink_lines;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

const MAX_RENDER_VARIANTS_PER_ITEM: usize = 4;
const MAX_LAYOUT_VARIANTS: usize = 4;
pub(super) const TOOL_DETAIL_INDENT: u16 = 5;
const TOOL_MARKER_WIDTH: u16 = 2; // "▸ " or "⌄ "

fn tool_detail_indent(width: u16) -> u16 {
    // Leave room for the card label and value on very narrow terminals.
    TOOL_DETAIL_INDENT.min(width.saturating_sub(12))
}

pub(super) fn tool_label_indent(width: u16) -> u16 {
    tool_detail_indent(width).max(TOOL_MARKER_WIDTH)
}

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
    math_cell: Option<crate::equations::CellSize>,
    math_metrics: std::cell::RefCell<crate::equations::Measurements>,
    items: HashMap<u64, CachedTranscriptItem>,
    revisions: Vec<u64>,
    layouts: VecDeque<CachedTranscriptLayout>,
    visualizations: super::inline_visualizations::VisualizationState,
}

impl TranscriptRenderCache {
    pub(super) fn layout(
        &mut self,
        shell: &ShellState,
        width: u16,
        cwd: &Path,
    ) -> Arc<TranscriptLayout> {
        let cell = shell.graphics.borrow().cell;
        if cell != self.math_cell {
            self.clear();
            self.math_cell = cell;
            if cell.is_none() {
                self.math_metrics = Default::default();
            }
        }
        // All widths share one transcript. Drop obsolete layouts before allocating
        // their replacements so long streaming replies do not retain previous revisions.
        if !self.revisions.iter().copied().eq(render_revisions(shell)) {
            self.layouts.clear();
            self.revisions.clear();
            self.revisions.extend(render_revisions(shell));
        }
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

        let mut has_visualization = |revision, text: &str| {
            self.items
                .entry(revision)
                .or_insert_with(|| CachedTranscriptItem::new(TranscriptKind::Assistant, text))
                .has_visualization
        };
        let has_visualizations = shell.transcript.iter().any(|item| {
            item.kind == TranscriptKind::Assistant
                && has_visualization(item.render_revision, &item.text)
        }) || (!shell.streaming_assistant.is_empty()
            && has_visualization(
                shell.streaming_assistant_revision,
                &shell.streaming_assistant,
            ));
        let visualization_context = has_visualizations
            .then(|| self.visualizations.context(shell))
            .flatten();
        let mut previous_items = std::mem::take(&mut self.items);
        let mut current_items = HashMap::with_capacity(
            shell.transcript.len().min(super::MAX_TRANSCRIPT_LINES)
                + usize::from(!shell.streaming_assistant.is_empty()),
        );
        let mut chunks = Vec::new();
        let mut previous_kind = None;
        let context = CachedRenderContext {
            width,
            indent: 0,
            cwd,
            app_theme: shell.app_theme,
            visuals: TranscriptVisuals {
                inline: visualization_context,
                math: cell.map(|cell| (cell, &self.math_metrics)),
            },
        };

        let detail_indent = tool_detail_indent(width);
        let header_indent = tool_label_indent(width) - TOOL_MARKER_WIDTH;
        let mut groups = shell.tool_groups().peekable();
        for (index, item) in shell.transcript.iter().enumerate() {
            if groups.peek().is_some_and(|group| group.rows.end <= index) {
                groups.next();
            }
            let anchor_source = || {
                item.item_id.as_ref().map_or(
                    TranscriptAnchorSource::Revision(item.render_revision),
                    |id| TranscriptAnchorSource::Item {
                        kind: item.kind,
                        item_id: id.clone(),
                    },
                )
            };
            let group = groups.peek().filter(|group| group.rows.contains(&index));
            if let Some(group) = group {
                if index == group.rows.start {
                    let _theme = crate::app_theme::activate(shell.app_theme);
                    // The disclosure marker sits in the gutter beside the activity text.
                    let header = group.header(
                        width - header_indent,
                        shell.transcript_selection == Some(index),
                    );
                    chunks.push(TranscriptChunk {
                        target: Some(TranscriptTarget::ToolGroup(index)),
                        indent: header_indent,
                        revision: item.render_revision,
                        anchor_source: anchor_source(),
                        separator_before: should_separate_transcript_item(
                            previous_kind,
                            TranscriptKind::Tool,
                        ),
                        lines: indent_lines(vec![header], header_indent),
                    });
                    previous_kind = Some(TranscriptKind::Tool);
                }
                if !group.expanded {
                    continue;
                }
            }
            let selected = shell.transcript_selection == Some(index);
            push_cached_chunk(
                &mut chunks,
                &mut previous_kind,
                &mut previous_items,
                &mut current_items,
                CachedSource {
                    transcript_index: Some(index),
                    revision: item.render_revision,
                    anchor_source: anchor_source(),
                    kind: item.kind,
                    text: &item.text,
                    approvals: &item.approvals,
                    tool_status: item.tool_status,
                    selected,
                },
                CachedRenderContext {
                    indent: if group.is_some() { detail_indent } else { 0 },
                    ..context
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
                    anchor_source: shell.streaming_assistant_item_id.as_ref().map_or(
                        TranscriptAnchorSource::StreamingAssistant,
                        |item_id| TranscriptAnchorSource::Item {
                            kind: TranscriptKind::Assistant,
                            item_id: item_id.clone(),
                        },
                    ),
                    kind: TranscriptKind::Assistant,
                    text: &shell.streaming_assistant,
                    approvals: &[],
                    tool_status: None,
                    selected: false,
                },
                context,
            );
        }

        self.items = current_items;
        let layout = Arc::new(TranscriptLayout::new(chunks, previous_kind));
        if has_visualizations {
            return layout;
        }
        self.layouts.push_back(CachedTranscriptLayout {
            width,
            cwd: cwd.to_path_buf(),
            selected: shell.transcript_selection,
            app_theme: shell.app_theme,
            layout: Arc::clone(&layout),
        });
        while self.layouts.len() > MAX_LAYOUT_VARIANTS {
            self.layouts.pop_front();
        }
        layout
    }

    pub(super) fn clear(&mut self) {
        self.revisions.clear();
        self.items.clear();
        self.layouts.clear();
    }
}

struct CachedTranscriptLayout {
    width: u16,
    cwd: PathBuf,
    selected: Option<usize>,
    app_theme: TuiAppTheme,
    layout: Arc<TranscriptLayout>,
}

impl CachedTranscriptLayout {
    fn matches(&self, shell: &ShellState, width: u16, cwd: &Path) -> bool {
        self.width == width
            && self.cwd == cwd
            && self.selected == shell.transcript_selection
            && self.app_theme == shell.app_theme
    }
}

fn render_revisions(shell: &ShellState) -> impl Iterator<Item = u64> + '_ {
    shell
        .transcript
        .iter()
        .map(|item| item.render_revision)
        .chain(
            (!shell.streaming_assistant.is_empty()).then_some(shell.streaming_assistant_revision),
        )
}

struct CachedSource<'a> {
    transcript_index: Option<usize>,
    revision: u64,
    anchor_source: TranscriptAnchorSource,
    kind: TranscriptKind,
    text: &'a str,
    approvals: &'a [super::guardian_activity::ToolApproval],
    tool_status: Option<ToolBlockStatus>,
    selected: bool,
}

#[derive(Clone, Copy)]
struct CachedRenderContext<'a> {
    width: u16,
    indent: u16,
    cwd: &'a Path,
    app_theme: TuiAppTheme,
    visuals: TranscriptVisuals<'a>,
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
    let mut cached = previous_items
        .remove(&source.revision)
        .unwrap_or_else(|| CachedTranscriptItem::new(source.kind, source.text));
    let lines = cached.lines(&source, context);
    chunks.push(TranscriptChunk {
        target: source.transcript_index.map(TranscriptTarget::Item),
        indent: context.indent,
        revision: source.revision,
        anchor_source: source.anchor_source,
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
        return current_kind == TranscriptKind::Separator;
    };
    // Prompts and turn dividers share a single blank row with each neighbor.
    if matches!(
        current_kind,
        TranscriptKind::User | TranscriptKind::Separator
    ) || matches!(
        previous_kind,
        TranscriptKind::User | TranscriptKind::Separator
    ) {
        return true;
    }
    if previous_kind == TranscriptKind::System {
        return false;
    }
    matches!(
        current_kind,
        TranscriptKind::Assistant
            | TranscriptKind::Tool
            | TranscriptKind::Diff
            | TranscriptKind::Output
    )
}

struct CachedTranscriptItem {
    has_visualization: bool,
    variants: VecDeque<CachedRenderVariant>,
}

impl CachedTranscriptItem {
    fn new(kind: TranscriptKind, text: &str) -> Self {
        Self {
            has_visualization: kind == TranscriptKind::Assistant
                && crate::inline_visualization::contains_inline_visualization(text),
            variants: VecDeque::new(),
        }
    }

    fn lines(
        &mut self,
        source: &CachedSource<'_>,
        context: CachedRenderContext<'_>,
    ) -> Arc<[HyperlinkLine]> {
        let CachedRenderContext {
            width,
            indent,
            cwd,
            app_theme,
            visuals,
        } = context;
        let selected = source.selected;
        if let Some(index) = self.variants.iter().position(|variant| {
            variant.width == width
                && variant.indent == indent
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
        let mut lines = render_transcript_line_with_visualizations(
            source.kind,
            source.text,
            source.tool_status,
            width - indent,
            cwd,
            selected,
            visuals,
        );
        for approval in source.approvals {
            lines.extend(approval.lines(source.text, width - indent));
        }
        let lines = indent_lines(lines, indent);
        if self.has_visualization {
            return lines;
        }
        self.variants.push_back(CachedRenderVariant {
            width,
            indent,
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
    indent: u16,
    cwd: PathBuf,
    selected: bool,
    app_theme: TuiAppTheme,
    lines: Arc<[HyperlinkLine]>,
}

fn indent_lines(mut lines: Vec<HyperlinkLine>, indent: u16) -> Arc<[HyperlinkLine]> {
    if indent > 0 {
        let margin = ratatui::text::Span::from(" ".repeat(usize::from(indent)));
        lines = prefix_hyperlink_lines(lines, margin.clone(), margin);
        if let Some(first) = lines.first_mut() {
            first.synthetic_prefix_width += usize::from(indent);
        }
    }
    lines.into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptTarget {
    Item(usize),
    ToolGroup(usize),
}

struct TranscriptChunk {
    target: Option<TranscriptTarget>,
    indent: u16,
    #[cfg_attr(not(test), allow(dead_code))]
    revision: u64,
    anchor_source: TranscriptAnchorSource,
    separator_before: bool,
    lines: Arc<[HyperlinkLine]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TranscriptAnchorSource {
    Item {
        kind: TranscriptKind,
        item_id: String,
    },
    Revision(u64),
    StreamingAssistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptViewportAnchor {
    source: TranscriptAnchorSource,
    group_header: bool,
    line_offset: usize,
    pub(super) width: u16,
}

pub(super) struct TranscriptLayout {
    chunks: Vec<TranscriptChunk>,
    pub(super) total_lines: usize,
    /// Row for live activity after the output and its blank separator.
    pub(super) activity_row: usize,
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
    fn new(chunks: Vec<TranscriptChunk>, last_kind: Option<TranscriptKind>) -> Self {
        // The next chunk normally owns the gap. Reserve it here when there is no next chunk.
        let trailing_rows = usize::from(matches!(
            last_kind,
            Some(TranscriptKind::User | TranscriptKind::Separator)
        ));
        let output_rows: usize = chunks
            .iter()
            .map(|chunk| chunk.lines.len() + usize::from(chunk.separator_before))
            .sum();
        Self {
            chunks,
            total_lines: output_rows + trailing_rows,
            activity_row: output_rows + usize::from(output_rows > 0),
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
        if logical_row < self.total_lines && (visible_from..visible_to).contains(&logical_row) {
            visible.push(HyperlinkLine::new(Default::default()));
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
        (row < self.total_lines).then_some(TranscriptLayoutRow::Blank)
    }

    /// Return the target and left margin of the chunk that owns a rendered row.
    ///
    /// Separator rows and streaming-only chunks have no stored transcript
    /// source, so they intentionally return `None`.
    pub(super) fn target_at_row(&self, row: usize) -> Option<(TranscriptTarget, u16)> {
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
                return chunk.target.map(|target| (target, chunk.indent));
            }
            logical_row = chunk_end;
            if logical_row > row {
                break;
            }
        }
        None
    }

    pub(super) fn transcript_row_range(&self, index: usize) -> Option<std::ops::Range<usize>> {
        self.target_rows(TranscriptTarget::Item(index))
            .or_else(|| self.target_rows(TranscriptTarget::ToolGroup(index)))
    }

    pub(super) fn target_rows(&self, target: TranscriptTarget) -> Option<std::ops::Range<usize>> {
        let mut logical_row = 0usize;
        for chunk in &self.chunks {
            logical_row = logical_row.saturating_add(usize::from(chunk.separator_before));
            let chunk_end = logical_row.saturating_add(chunk.lines.len());
            if chunk.target == Some(target) {
                return Some(logical_row..chunk_end);
            }
            logical_row = chunk_end;
        }
        None
    }

    pub(super) fn viewport_anchor_at_or_after(
        &self,
        row: usize,
        width: u16,
    ) -> Option<TranscriptViewportAnchor> {
        let mut logical_row = 0usize;
        for chunk in &self.chunks {
            let chunk_start = logical_row;
            let chunk_end = chunk_start
                .saturating_add(usize::from(chunk.separator_before))
                .saturating_add(chunk.lines.len());
            if chunk_end > row {
                return Some(TranscriptViewportAnchor {
                    source: chunk.anchor_source.clone(),
                    group_header: matches!(chunk.target, Some(TranscriptTarget::ToolGroup(_))),
                    line_offset: row
                        .saturating_sub(chunk_start)
                        .min(chunk_end.saturating_sub(chunk_start).saturating_sub(1)),
                    width,
                });
            }
            logical_row = chunk_end;
        }
        None
    }

    pub(super) fn row_for_viewport_anchor(
        &self,
        anchor: &TranscriptViewportAnchor,
    ) -> Option<usize> {
        let mut logical_row = 0usize;
        for chunk in &self.chunks {
            let chunk_start = logical_row;
            let chunk_end = chunk_start
                .saturating_add(usize::from(chunk.separator_before))
                .saturating_add(chunk.lines.len());
            if chunk.anchor_source == anchor.source
                && matches!(chunk.target, Some(TranscriptTarget::ToolGroup(_)))
                    == anchor.group_header
            {
                return Some(
                    chunk_start.saturating_add(
                        anchor
                            .line_offset
                            .min(chunk_end.saturating_sub(chunk_start).saturating_sub(1)),
                    ),
                );
            }
            logical_row = chunk_end;
        }
        None
    }
}

#[cfg(test)]
#[path = "transcript_render_tests.rs"]
mod tests;
