#[path = "transcript_semantic_copy.rs"]
mod semantic_copy;
pub(super) use semantic_copy::selected_markdown as transcript_selected_copy_text;

use super::ShellState;
use super::ToolBlockStatus;
use super::TranscriptKind;
use super::design::body_rect_after_title;
use super::design::markdown_styles;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::selection_style;
use super::design::text_selection_style;
use super::design::title_rect;
use super::diff_style::diff_stat_spans;
use super::terminal_output;
use super::text_selection::NormalizedVisualRange;
use super::text_selection::VisualGraphemeHit;
use super::text_selection::grapheme_hit_at;
use super::text_selection::graphemes_in_columns;
use super::text_selection::trim_synthetic_right_padding;
use super::transcript_render::TranscriptLayout;
use super::transcript_render::TranscriptLayoutRow;
use super::transcript_render::TranscriptTarget;
use crate::line_truncation::truncate_line_to_width;
use crate::markdown;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::mark_buffer_hyperlinks;
use crate::terminal_hyperlinks::prefix_hyperlink_lines;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::ops::Range;
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

const TRANSCRIPT_SCROLLBAR_MIN_THUMB_HEIGHT: u16 = 2;
const TURN_SEPARATOR_INDENT: usize = 9;
const OUTPUT_BLOCK_INDENT: usize = 2;
const OUTPUT_BLOCK_MAX_LINES: usize = 4;

pub(super) fn render_transcript(
    shell: &ShellState,
    area: Rect,
    hover_position: Option<Position>,
    buf: &mut Buffer,
) {
    let viewport = transcript_viewport(shell, area);
    if viewport.layout.total_lines == 0 && shell.active_turn_id.is_none() {
        return;
    }
    if let Some(selected) = shell.transcript_selection {
        let mut hint = format!("SELECT {}/{}", selected + 1, shell.transcript.len());
        if shell.tool_group_start(selected).is_some() {
            hint.push_str("  Space FOLD/UNFOLD");
        }
        if shell.tool_groups().next().is_some() {
            hint.push_str("  Tab TOOLS");
        }
        if shell.selected_transcript_is_rewindable() {
            hint.push_str("  e BRANCH EDIT");
        }
        Paragraph::new(hint.fg(palette::muted()).bold()).render(title_rect(viewport.content), buf);
    }
    let visible_hyperlink_lines = viewport
        .layout
        .visible_hyperlink_lines(viewport.visible_from, viewport.visible_count);
    buf.set_style(viewport.text_body, pane_style(palette::base()));
    for (row, line) in viewport.text_body.rows().zip(&visible_hyperlink_lines) {
        (&line.line).render(row, buf);
    }
    mark_buffer_hyperlinks(
        buf,
        viewport.text_body,
        &visible_hyperlink_lines,
        /*scroll_rows*/ 0,
    );
    shell.render_turn_activity(
        viewport.text_body,
        viewport.layout.activity_row,
        viewport.visible_from,
        buf,
    );
    render_card_hover(shell, &viewport, hover_position, buf);
    if let Some(scrollbar) = viewport.scrollbar {
        render_transcript_scrollbar(buf, viewport.body, scrollbar);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptCardHit {
    ToolGroup { transcript_index: usize },
    ToolOutput { transcript_index: usize },
    Diff { transcript_index: usize },
}

impl TranscriptCardHit {
    fn target(self) -> TranscriptTarget {
        match self {
            Self::ToolGroup { transcript_index } => TranscriptTarget::ToolGroup(transcript_index),
            Self::ToolOutput { transcript_index } | Self::Diff { transcript_index } => {
                TranscriptTarget::Item(transcript_index)
            }
        }
    }
}

pub(super) fn transcript_card_at(
    shell: &ShellState,
    area: Rect,
    position: Position,
) -> Option<TranscriptCardHit> {
    let viewport = transcript_viewport(shell, area);
    transcript_card_hit_at(shell, &viewport, position).map(|(hit, _)| hit)
}

/// Keep the clicked header in place when opening a group adds more than a screen of rows.
pub(super) fn toggle_tool_group_at(shell: &mut ShellState, area: Rect, index: usize) {
    let before = transcript_viewport(shell, area);
    let target = TranscriptTarget::ToolGroup(index);
    let Some(rows) = before.layout.target_rows(target) else {
        return;
    };
    let offset = rows.start.saturating_sub(before.visible_from);
    shell.toggle_tool_group(index);
    let after = transcript_viewport(shell, area);
    if let Some(rows) = after.layout.target_rows(target) {
        let top = rows.start.saturating_sub(offset);
        shell.transcript_scroll = shell.transcript_scroll_max.get().saturating_sub(top);
        shell.clear_transcript_viewport_anchor();
    }
}

pub(super) fn transcript_output_at(
    shell: &ShellState,
    area: Rect,
    position: Position,
) -> Option<usize> {
    match transcript_card_at(shell, area, position) {
        Some(TranscriptCardHit::ToolOutput { transcript_index }) => Some(transcript_index),
        Some(TranscriptCardHit::Diff { .. } | TranscriptCardHit::ToolGroup { .. }) | None => None,
    }
}

/// Resolve a terminal position inside the rendered transcript body to its web destination.
pub(super) fn transcript_hyperlink_at(
    shell: &ShellState,
    area: Rect,
    position: Position,
) -> Option<crate::terminal_hyperlinks::ClickTarget> {
    let viewport = transcript_viewport(shell, area);
    if !viewport.text_body.contains(position) {
        return None;
    }

    let row = viewport
        .visible_from
        .saturating_add(usize::from(position.y.saturating_sub(viewport.text_body.y)));
    let line = viewport.layout.row_at(row)?.line()?;
    let column = usize::from(position.x.saturating_sub(viewport.text_body.x));
    line.hyperlinks
        .iter()
        .find(|hyperlink| hyperlink.columns.contains(&column))
        .and_then(crate::terminal_hyperlinks::TerminalHyperlink::click_target)
}

/// Resolve a terminal position inside the rendered transcript body to its complete grapheme.
pub(super) fn transcript_text_hit_at(
    shell: &ShellState,
    area: Rect,
    position: Position,
) -> Option<VisualGraphemeHit> {
    let viewport = transcript_viewport(shell, area);
    if !viewport.text_body.contains(position) {
        return None;
    }

    let row = viewport
        .visible_from
        .saturating_add(usize::from(position.y.saturating_sub(viewport.text_body.y)));
    let line = viewport.layout.row_at(row)?.line()?;
    let text = rendered_line_text(line);
    let text = trim_synthetic_right_padding(&text);
    let column = usize::from(position.x.saturating_sub(viewport.text_body.x));
    if column < line.synthetic_prefix_width {
        return None;
    }
    grapheme_hit_at(text, row, column)
}

/// Extract the visible rendered characters covered by a transcript selection.
///
/// The returned text contains rendered labels and Markdown rather than source markup, excludes
/// hyperlink control sequences, and removes alignment prefixes and right-edge card padding before
/// joining logical rows.
pub(super) fn transcript_selected_text(
    shell: &ShellState,
    area: Rect,
    selection: NormalizedVisualRange,
) -> Option<String> {
    let viewport = transcript_viewport(shell, area);
    if selection.start().row() >= viewport.layout.total_lines
        || selection.end().row() >= viewport.layout.total_lines
    {
        return None;
    }

    let mut selected_rows = Vec::new();
    for row in selection.start().row()..=selection.end().row() {
        let (text, synthetic_prefix_width) = match viewport.layout.row_at(row)? {
            TranscriptLayoutRow::Blank => (String::new(), 0),
            TranscriptLayoutRow::Rendered(line) => {
                (rendered_line_text(line), line.synthetic_prefix_width)
            }
        };
        let text = trim_synthetic_right_padding(&text);
        let columns =
            selectable_columns_on_row(selection, row, text.width(), synthetic_prefix_width)?;
        selected_rows.push(graphemes_in_columns(text, columns));
    }
    Some(selected_rows.join("\n"))
}

/// Patch the selected transcript cells without invalidating the cached text layout.
pub(super) fn render_transcript_text_selection(
    shell: &ShellState,
    area: Rect,
    selection: NormalizedVisualRange,
    buf: &mut Buffer,
) {
    let viewport = transcript_viewport(shell, area);
    if selection.start().row() >= viewport.layout.total_lines
        || selection.end().row() >= viewport.layout.total_lines
        || viewport.visible_count == 0
    {
        return;
    }

    let visible_end = viewport
        .visible_from
        .saturating_add(viewport.visible_count)
        .min(viewport.layout.total_lines);
    let first_row = selection.start().row().max(viewport.visible_from);
    let last_row_exclusive = selection.end().row().saturating_add(1).min(visible_end);
    for row in first_row..last_row_exclusive {
        let Some(TranscriptLayoutRow::Rendered(line)) = viewport.layout.row_at(row) else {
            continue;
        };
        let text = rendered_line_text(line);
        let text = trim_synthetic_right_padding(&text);
        let Some(columns) =
            selectable_columns_on_row(selection, row, text.width(), line.synthetic_prefix_width)
        else {
            continue;
        };
        let start = columns.start.min(usize::from(viewport.text_body.width));
        let end = columns.end.min(usize::from(viewport.text_body.width));
        if start >= end {
            continue;
        }

        let x_offset = u16::try_from(start).unwrap_or(u16::MAX);
        let y_offset = u16::try_from(row.saturating_sub(viewport.visible_from)).unwrap_or(u16::MAX);
        let width = u16::try_from(end.saturating_sub(start)).unwrap_or(u16::MAX);
        buf.set_style(
            Rect::new(
                viewport.text_body.x.saturating_add(x_offset),
                viewport.text_body.y.saturating_add(y_offset),
                width,
                /*height*/ 1,
            ),
            text_selection_style(),
        );
    }
}

fn rendered_line_text(line: &crate::terminal_hyperlinks::HyperlinkLine) -> String {
    line.line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn selectable_columns_on_row(
    selection: NormalizedVisualRange,
    row: usize,
    line_width: usize,
    synthetic_prefix_width: usize,
) -> Option<Range<usize>> {
    let columns = selection.columns_on_row(row, line_width)?;
    let start = columns.start.max(synthetic_prefix_width).min(columns.end);
    Some(start..columns.end)
}

fn transcript_card_hit_at(
    shell: &ShellState,
    viewport: &TranscriptViewport,
    position: Position,
) -> Option<(TranscriptCardHit, u16)> {
    if !viewport.text_body.contains(position) {
        return None;
    }

    let logical_row = viewport
        .visible_from
        .saturating_add(usize::from(position.y.saturating_sub(viewport.text_body.y)));
    let (target, mut indent) = viewport.layout.target_at_row(logical_row)?;
    let hit = match target {
        TranscriptTarget::ToolGroup(transcript_index) => {
            TranscriptCardHit::ToolGroup { transcript_index }
        }
        TranscriptTarget::Item(transcript_index) => {
            let item = shell.transcript.get(transcript_index)?;
            item.tool_status?;
            match item.kind {
                TranscriptKind::Output => {
                    indent += OUTPUT_BLOCK_INDENT as u16;
                    TranscriptCardHit::ToolOutput { transcript_index }
                }
                TranscriptKind::Diff => TranscriptCardHit::Diff { transcript_index },
                TranscriptKind::System
                | TranscriptKind::User
                | TranscriptKind::Assistant
                | TranscriptKind::Plan
                | TranscriptKind::Tool
                | TranscriptKind::Separator
                | TranscriptKind::Status
                | TranscriptKind::Audit
                | TranscriptKind::Error => return None,
            }
        }
    };
    (position.x >= viewport.text_body.x.saturating_add(indent)).then_some((hit, indent))
}

fn render_card_hover(
    shell: &ShellState,
    viewport: &TranscriptViewport,
    hover_position: Option<Position>,
    buf: &mut Buffer,
) {
    let Some((hit, block_indent)) =
        hover_position.and_then(|position| transcript_card_hit_at(shell, viewport, position))
    else {
        return;
    };
    let Some(rows) = viewport.layout.target_rows(hit.target()) else {
        return;
    };
    let visible_end = viewport.visible_from.saturating_add(viewport.visible_count);
    let hover_start = rows.start.max(viewport.visible_from);
    let hover_end = rows.end.min(visible_end);
    if hover_start >= hover_end {
        return;
    }

    let y_offset =
        u16::try_from(hover_start.saturating_sub(viewport.visible_from)).unwrap_or(u16::MAX);
    let height = u16::try_from(hover_end.saturating_sub(hover_start)).unwrap_or(u16::MAX);
    let hover_area = Rect::new(
        viewport.text_body.x.saturating_add(block_indent),
        viewport.text_body.y.saturating_add(y_offset),
        viewport.text_body.width.saturating_sub(block_indent),
        height,
    );
    buf.set_style(hover_area, Style::new().bg(palette::border()));
}

struct TranscriptViewport {
    content: Rect,
    body: Rect,
    text_body: Rect,
    layout: Arc<TranscriptLayout>,
    visible_from: usize,
    visible_count: usize,
    scrollbar: Option<TranscriptScrollbarMetrics>,
}

fn transcript_viewport(shell: &ShellState, area: Rect) -> TranscriptViewport {
    let previous_anchor = shell.transcript_viewport_anchor.borrow().clone();
    let previous_scroll = if shell.transcript_scroll == 0 {
        0
    } else {
        shell
            .transcript_effective_scroll
            .get()
            .unwrap_or(shell.transcript_scroll)
    };
    let inner = pane_content_rect(area);
    let left = 2.min(inner.width.saturating_sub(1));
    let top = 1.min(inner.height.saturating_sub(1));
    let content = Rect::new(
        inner.x + left,
        inner.y + top,
        inner.width - left,
        inner.height - top,
    );
    let body = if shell.transcript_selection.is_some() {
        body_rect_after_title(content)
    } else {
        content
    };
    let cwd = std::path::Path::new(&shell.cwd);
    let mut text_body = body;
    let visible_count = usize::from(body.height);
    let mut layout = shell
        .transcript_render_cache
        .borrow_mut()
        .layout(shell, text_body.width, cwd);
    // The live indicator scrolls with the text but is never selectable history.
    let scroll_height = |layout: &TranscriptLayout| {
        if shell.active_turn_id.is_some() {
            layout.activity_row + 1
        } else {
            layout.total_lines
        }
    };
    let mut max_scroll = scroll_height(&layout).saturating_sub(visible_count);
    if max_scroll > 0 && body.width > 2 {
        text_body.width = text_body.width.saturating_sub(2);
        layout = shell
            .transcript_render_cache
            .borrow_mut()
            .layout(shell, text_body.width, cwd);
        max_scroll = scroll_height(&layout).saturating_sub(visible_count);
    }
    shell.transcript_scroll_max.set(max_scroll);
    let scroll = previous_scroll.min(max_scroll);
    let mut visible_from =
        scroll_height(&layout).saturating_sub(visible_count.saturating_add(scroll));
    if previous_scroll > 0
        && let Some(anchor) = &previous_anchor
        && let Some(anchor_row) = layout.row_for_viewport_anchor(anchor)
    {
        visible_from = anchor_row.min(max_scroll);
    }
    let resized = previous_anchor
        .as_ref()
        .is_some_and(|anchor| anchor.width != text_body.width);
    if (resized || shell.transcript_selection_needs_reveal.replace(false))
        && visible_count > 0
        && let Some(selected) = shell.transcript_selection
        && let Some(rows) = layout.transcript_row_range(selected)
    {
        // Reveal prompt text even when only its shaded margin is on screen.
        let rows = if shell.transcript[selected].kind == TranscriptKind::User {
            rows.start + 1..rows.end - 1
        } else {
            rows
        };
        let visible_end = visible_from.saturating_add(visible_count);
        if rows.end <= visible_from {
            visible_from = rows.start.min(max_scroll);
        } else if rows.start >= visible_end {
            visible_from = rows
                .start
                .saturating_add(1)
                .saturating_sub(visible_count)
                .min(max_scroll);
        }
    }
    let effective_scroll = max_scroll.saturating_sub(visible_from);
    shell
        .transcript_effective_scroll
        .set(Some(effective_scroll));
    *shell.transcript_viewport_anchor.borrow_mut() = (effective_scroll > 0)
        .then(|| layout.viewport_anchor_at_or_after(visible_from, text_body.width))
        .flatten();
    let scrollbar = transcript_scrollbar_metrics(
        scroll_height(&layout),
        body.height,
        visible_from,
        TRANSCRIPT_SCROLLBAR_MIN_THUMB_HEIGHT,
    );
    TranscriptViewport {
        content,
        body,
        text_body,
        layout,
        visible_from,
        visible_count,
        scrollbar,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TranscriptScrollbarMetrics {
    pub(super) thumb_top: u16,
    pub(super) thumb_height: u16,
}

pub(super) fn transcript_scrollbar_metrics(
    total_lines: usize,
    visible_count: u16,
    visible_from: usize,
    min_thumb_height: u16,
) -> Option<TranscriptScrollbarMetrics> {
    let visible_count_usize = usize::from(visible_count);
    if visible_count == 0 || total_lines <= visible_count_usize {
        return None;
    }

    let track_height = visible_count;
    let min_thumb_height = min_thumb_height.min(track_height).max(1);
    let raw_thumb_height = visible_count_usize
        .saturating_mul(visible_count_usize)
        .div_ceil(total_lines)
        .try_into()
        .unwrap_or(u16::MAX);
    let thumb_height = raw_thumb_height.clamp(min_thumb_height, track_height);
    let thumb_travel = track_height.saturating_sub(thumb_height);
    let max_visible_from = total_lines.saturating_sub(visible_count_usize);
    let thumb_top = if thumb_travel == 0 || max_visible_from == 0 {
        0
    } else {
        let rounded_offset = visible_from
            .min(max_visible_from)
            .saturating_mul(usize::from(thumb_travel))
            .saturating_add(max_visible_from / 2)
            / max_visible_from;
        rounded_offset.try_into().unwrap_or(thumb_travel)
    };

    Some(TranscriptScrollbarMetrics {
        thumb_top,
        thumb_height,
    })
}

fn render_transcript_scrollbar(
    buf: &mut Buffer,
    body: Rect,
    scrollbar: TranscriptScrollbarMetrics,
) {
    let x = body.right().saturating_sub(1);
    let thumb_start = body.y.saturating_add(scrollbar.thumb_top);
    let thumb_end = thumb_start.saturating_add(scrollbar.thumb_height);
    for y in body.y..body.bottom() {
        let Some(cell) = buf.cell_mut((x, y)) else {
            continue;
        };
        if (thumb_start..thumb_end).contains(&y) {
            cell.set_symbol("┃")
                .set_style(Style::new().fg(palette::focus()));
        } else {
            cell.set_symbol("│")
                .set_style(Style::new().fg(palette::border()));
        }
    }
}

pub(super) fn render_transcript_line(
    kind: TranscriptKind,
    text: &str,
    tool_status: Option<ToolBlockStatus>,
    width: u16,
    cwd: &std::path::Path,
    selected: bool,
) -> Vec<HyperlinkLine> {
    render_transcript_line_with_visualizations(
        kind,
        text,
        tool_status,
        width,
        cwd,
        selected,
        /*inline_visualization_context*/ None,
    )
}

pub(super) fn render_transcript_line_with_visualizations(
    kind: TranscriptKind,
    text: &str,
    tool_status: Option<ToolBlockStatus>,
    width: u16,
    cwd: &std::path::Path,
    selected: bool,
    inline_visualization_context: Option<&crate::inline_visualization::InlineVisualizationContext>,
) -> Vec<HyperlinkLine> {
    if kind == TranscriptKind::Separator {
        return vec![HyperlinkLine::new(turn_separator_line(
            text,
            usize::from(width),
        ))];
    }
    if let Some(status) = tool_status
        && matches!(
            kind,
            TranscriptKind::Tool | TranscriptKind::Diff | TranscriptKind::Output
        )
    {
        return tool_block_lines(kind, text, width, status, selected);
    }

    let text_color = match kind {
        TranscriptKind::System
        | TranscriptKind::Output
        | TranscriptKind::Separator
        | TranscriptKind::Status => palette::muted(),
        TranscriptKind::Diff | TranscriptKind::Plan => palette::success(),
        TranscriptKind::Error => palette::error(),
        TranscriptKind::Assistant
        | TranscriptKind::User
        | TranscriptKind::Audit
        | TranscriptKind::Tool => palette::text(),
    };
    let prefix = match kind {
        TranscriptKind::User => "> ".bold(),
        TranscriptKind::Assistant => Span::default(),
        _ => {
            let marker = if selected { "▶" } else { "▎" };
            let color = if matches!(kind, TranscriptKind::Tool | TranscriptKind::Audit) {
                palette::cyan()
            } else {
                text_color
            };
            format!("{marker} {}  ", kind.label().to_uppercase())
                .fg(color)
                .bold()
        }
    };
    let prefix_width = prefix.width();
    let body_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let mut lines = if matches!(kind, TranscriptKind::Assistant | TranscriptKind::Plan) {
        let rendered = markdown::render_markdown_agent_with_styles(
            text,
            Some(body_width),
            Some(cwd),
            inline_visualization_context,
            crate::markdown_render::ListSpacing::AfterMultiline,
            markdown_styles(),
        );
        crate::terminal_hyperlinks::adaptive_wrap_hyperlink_lines(
            &rendered,
            crate::wrapping::RtOptions::new(body_width),
        )
    } else {
        // Imported prompts may end with a newline. The prompt block supplies its own margin.
        let text = if kind == TranscriptKind::User {
            text.trim_end()
        } else {
            text
        };
        textwrap::wrap(text, textwrap::Options::new(body_width))
            .into_iter()
            .map(|wrapped| HyperlinkLine::from(wrapped.into_owned()))
            .collect()
    };
    if prefix_width > 0 {
        lines = prefix_hyperlink_lines(lines, prefix, " ".repeat(prefix_width).into());
    }
    if kind == TranscriptKind::User {
        if let Some(first) = lines.first_mut() {
            first.synthetic_prefix_width = prefix_width;
        }
        // Keep the shaded margins with the message as it wraps and scrolls.
        lines.insert(0, " ".into());
        lines.push(" ".into());
    }
    let style = if selected {
        selection_style()
    } else if kind == TranscriptKind::User {
        pane_style(palette::surface())
    } else {
        Style::new().fg(text_color)
    };
    lines.into_iter().map(|line| line.style(style)).collect()
}

fn turn_separator_line(label: &str, width: usize) -> Line<'static> {
    let indent = TURN_SEPARATOR_INDENT;
    let border = palette::border();
    if label.is_empty() || width <= indent {
        return Line::from("─".repeat(width).fg(border));
    }

    let mut line = Line::from(vec![
        "─".repeat(indent - 1).fg(border),
        " ".into(),
        label.to_owned().fg(palette::muted()),
    ]);
    let used = indent + label.width();
    if used > width {
        return crate::line_truncation::truncate_line_with_ellipsis_if_overflow(line, width);
    }
    if width > used {
        line.spans.push(" ".into());
        line.spans.push("─".repeat(width - used - 1).fg(border));
    }
    line
}

fn tool_block_lines(
    kind: TranscriptKind,
    text: &str,
    width: u16,
    status: ToolBlockStatus,
    selected: bool,
) -> Vec<HyperlinkLine> {
    let width = usize::from(width).max(12);
    let block_indent = if kind == TranscriptKind::Output {
        OUTPUT_BLOCK_INDENT.min(width.saturating_sub(1))
    } else {
        0
    };
    let block_width = width.saturating_sub(block_indent).max(1);
    let label = kind.label();
    let label_width = label.width();
    let label_prefix_width = label_width + 3;
    let content_width = block_width.saturating_sub(label_prefix_width).max(1);
    let normalized_text = terminal_output::normalize(text);
    let visible_text = codex_ansi_escape::ansi_escape(&normalized_text);
    let visible_text_lines = if visible_text.lines.is_empty() {
        vec![String::new()]
    } else {
        visible_text
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    let mut wrapped = Vec::new();
    for text in visible_text_lines {
        let line_wrapped = textwrap::wrap(&text, textwrap::Options::new(content_width));
        if line_wrapped.is_empty() {
            wrapped.push(String::new());
        } else {
            wrapped.extend(line_wrapped.into_iter().map(std::borrow::Cow::into_owned));
        }
    }
    if kind == TranscriptKind::Output && wrapped.len() > OUTPUT_BLOCK_MAX_LINES {
        let hidden_lines = wrapped.len().saturating_sub(OUTPUT_BLOCK_MAX_LINES - 1);
        let mut tail = wrapped.split_off(hidden_lines);
        let noun = if hidden_lines == 1 { "line" } else { "lines" };
        wrapped = vec![format!("... {hidden_lines} earlier output {noun}")];
        wrapped.append(&mut tail);
    }
    let mut rendered_lines = wrapped
        .into_iter()
        .enumerate()
        .map(|(index, wrapped)| {
            let label_span = if index == 0 {
                format!("{label} ").bold()
            } else {
                " ".repeat(label_width + 1).dim()
            };
            let mut spans = Vec::new();
            if block_indent > 0 {
                spans.push(" ".repeat(block_indent).into());
            }
            spans.extend([
                Span::styled("▌", status.accent_style()),
                " ".into(),
                label_span,
            ]);
            if kind == TranscriptKind::Diff {
                spans.extend(diff_stat_spans(wrapped));
            } else {
                spans.push(wrapped.into());
            }
            let line = truncate_line_to_width(Line::from(spans), width);
            let mut line = HyperlinkLine::new(line);
            if index > 0 {
                line.synthetic_prefix_width = block_indent + label_prefix_width;
            }
            line
        })
        .collect::<Vec<_>>();

    if selected {
        rendered_lines = rendered_lines
            .into_iter()
            .map(|line| line.style(selection_style()))
            .collect();
    }
    rendered_lines
}

impl ToolBlockStatus {
    fn accent_style(self) -> Style {
        match self {
            Self::Running => Style::new().fg(palette::cyan()),
            Self::Success => Style::new().fg(palette::success()),
            Self::Fail => Style::new().fg(palette::error()),
            Self::Interrupted | Self::Unconfirmed => Style::new().fg(palette::warning()),
        }
    }
}

#[cfg(test)]
#[path = "transcript_view_tests.rs"]
mod tests;
