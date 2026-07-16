use super::ShellState;
use super::ToolBlockStatus;
use super::TranscriptKind;
use super::design::body_rect_after_title;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::selection_style;
use super::design::title_rect;
use super::diff_style::diff_stat_spans;
use super::transcript_render::TranscriptLayout;
use crate::line_truncation::line_width;
use crate::line_truncation::truncate_line_to_width;
use crate::markdown;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::mark_buffer_hyperlinks;
use crate::terminal_hyperlinks::prefix_hyperlink_lines;
use crate::terminal_hyperlinks::visible_lines;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

const TRANSCRIPT_SCROLLBAR_MIN_THUMB_HEIGHT: u16 = 2;
const OUTPUT_BLOCK_INDENT: usize = 2;
const OUTPUT_BLOCK_MAX_LINES: usize = 4;

pub(super) fn render_transcript(
    shell: &ShellState,
    area: Rect,
    hover_position: Option<Position>,
    buf: &mut Buffer,
) {
    let viewport = transcript_viewport(shell, area);
    let title = if let Some(selected) = shell.transcript_selection {
        format!(
            "CONVERSATION  SELECT {}/{}",
            selected.saturating_add(1),
            shell.transcript.len()
        )
    } else {
        "CONVERSATION".to_string()
    };
    let visible_hyperlink_lines = viewport
        .layout
        .visible_hyperlink_lines(viewport.visible_from, viewport.visible_count);
    let visible_lines = visible_lines(visible_hyperlink_lines.clone());
    Paragraph::new(Line::from(vec![
        "◆ ".set_style(Style::new().fg(palette::FOCUS)),
        title.set_style(Style::new().fg(palette::MUTED).bold()),
    ]))
    .style(pane_style(palette::BASE))
    .render(title_rect(viewport.content), buf);
    Paragraph::new(visible_lines)
        .style(pane_style(palette::BASE))
        .render(viewport.text_body, buf);
    mark_buffer_hyperlinks(
        buf,
        viewport.text_body,
        &visible_hyperlink_lines,
        /*scroll_rows*/ 0,
    );
    render_card_hover(shell, &viewport, hover_position, buf);
    if let Some(scrollbar) = viewport.scrollbar {
        render_transcript_scrollbar(buf, viewport.body, scrollbar);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptCardHit {
    ToolOutput { transcript_index: usize },
    Diff { transcript_index: usize },
}

impl TranscriptCardHit {
    pub(super) fn transcript_index(self) -> usize {
        match self {
            Self::ToolOutput { transcript_index } | Self::Diff { transcript_index } => {
                transcript_index
            }
        }
    }

    fn block_indent(self) -> usize {
        match self {
            Self::ToolOutput { .. } => OUTPUT_BLOCK_INDENT,
            Self::Diff { .. } => 0,
        }
    }
}

pub(super) fn transcript_card_at(
    shell: &ShellState,
    area: Rect,
    position: Position,
) -> Option<TranscriptCardHit> {
    let viewport = transcript_viewport(shell, area);
    transcript_card_hit_at(shell, &viewport, position)
}

pub(super) fn transcript_output_at(
    shell: &ShellState,
    area: Rect,
    position: Position,
) -> Option<usize> {
    match transcript_card_at(shell, area, position) {
        Some(TranscriptCardHit::ToolOutput { transcript_index }) => Some(transcript_index),
        Some(TranscriptCardHit::Diff { .. }) | None => None,
    }
}

fn transcript_card_hit_at(
    shell: &ShellState,
    viewport: &TranscriptViewport,
    position: Position,
) -> Option<TranscriptCardHit> {
    if !viewport.text_body.contains(position) {
        return None;
    }

    let logical_row = viewport
        .visible_from
        .saturating_add(usize::from(position.y.saturating_sub(viewport.text_body.y)));
    let transcript_index = viewport.layout.transcript_index_at_row(logical_row)?;
    let item = shell.transcript.get(transcript_index)?;
    item.tool_status?;
    let hit = match item.kind {
        TranscriptKind::Output => TranscriptCardHit::ToolOutput { transcript_index },
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
    };
    let block_indent = u16::try_from(hit.block_indent()).unwrap_or(u16::MAX);
    (position.x >= viewport.text_body.x.saturating_add(block_indent)).then_some(hit)
}

fn render_card_hover(
    shell: &ShellState,
    viewport: &TranscriptViewport,
    hover_position: Option<Position>,
    buf: &mut Buffer,
) {
    let Some(hit) =
        hover_position.and_then(|position| transcript_card_hit_at(shell, viewport, position))
    else {
        return;
    };
    let transcript_index = hit.transcript_index();
    let Some(rows) = viewport.layout.transcript_row_range(transcript_index) else {
        return;
    };
    let visible_end = viewport.visible_from.saturating_add(viewport.visible_count);
    let hover_start = rows.start.max(viewport.visible_from);
    let hover_end = rows.end.min(visible_end);
    if hover_start >= hover_end {
        return;
    }

    let block_indent = u16::try_from(hit.block_indent()).unwrap_or(u16::MAX);
    let y_offset =
        u16::try_from(hover_start.saturating_sub(viewport.visible_from)).unwrap_or(u16::MAX);
    let height = u16::try_from(hover_end.saturating_sub(hover_start)).unwrap_or(u16::MAX);
    let hover_area = Rect::new(
        viewport.text_body.x.saturating_add(block_indent),
        viewport.text_body.y.saturating_add(y_offset),
        viewport.text_body.width.saturating_sub(block_indent),
        height,
    );
    buf.set_style(hover_area, Style::new().bg(palette::BORDER));
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
    let content = pane_content_rect(area);
    let body = body_rect_after_title(content);
    let cwd = std::path::Path::new(&shell.cwd);
    let mut text_body = body;
    let visible_count = usize::from(body.height);
    let mut layout = shell
        .transcript_render_cache
        .borrow_mut()
        .layout(shell, text_body.width, cwd);
    let mut max_scroll = layout.total_lines.saturating_sub(visible_count);
    if max_scroll > 0 && body.width > 2 {
        text_body.width = text_body.width.saturating_sub(2);
        layout = shell
            .transcript_render_cache
            .borrow_mut()
            .layout(shell, text_body.width, cwd);
        max_scroll = layout.total_lines.saturating_sub(visible_count);
    }
    shell.transcript_scroll_max.set(max_scroll);
    let scroll = shell.transcript_scroll.min(max_scroll);
    let visible_from = layout
        .total_lines
        .saturating_sub(visible_count.saturating_add(scroll));
    let scrollbar = transcript_scrollbar_metrics(
        layout.total_lines,
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
                .set_style(Style::new().fg(palette::FOCUS));
        } else {
            cell.set_symbol("│")
                .set_style(Style::new().fg(palette::BORDER));
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
    if kind == TranscriptKind::Separator {
        return vec![HyperlinkLine::new(
            Line::from("─".repeat(usize::from(width))).style(Style::new().fg(palette::BORDER)),
        )];
    }
    if let Some(status) = tool_status
        && matches!(
            kind,
            TranscriptKind::Tool | TranscriptKind::Diff | TranscriptKind::Output
        )
    {
        return tool_block_lines(kind, text, width, status, selected);
    }

    let width = usize::from(width).max(12);
    let label = kind.label();
    let style = match kind {
        TranscriptKind::System => LineStyle::Dim,
        TranscriptKind::User => LineStyle::Cyan,
        TranscriptKind::Assistant => LineStyle::Magenta,
        TranscriptKind::Plan => LineStyle::Green,
        TranscriptKind::Tool => LineStyle::Cyan,
        TranscriptKind::Diff => LineStyle::Green,
        TranscriptKind::Output => LineStyle::Dim,
        TranscriptKind::Separator => LineStyle::Dim,
        TranscriptKind::Status => LineStyle::Dim,
        TranscriptKind::Audit => LineStyle::Cyan,
        TranscriptKind::Error => LineStyle::Red,
    };

    let prefix_width = label.len() + 4;
    let body_width = width.saturating_sub(prefix_width).max(1);
    let initial_prefix = style.label_prefix(label, selected);
    let subsequent_prefix = " ".repeat(prefix_width).into();

    let mut rendered_lines = if matches!(kind, TranscriptKind::Assistant | TranscriptKind::Plan) {
        let rendered =
            markdown::render_markdown_agent_with_links_and_cwd(text, Some(body_width), Some(cwd))
                .into_iter()
                .map(|line| line.style(style.line_style()))
                .collect();
        prefix_hyperlink_lines(rendered, initial_prefix, subsequent_prefix)
    } else {
        let options = textwrap::Options::new(body_width);
        let wrapped_lines: Vec<HyperlinkLine> = textwrap::wrap(text, options)
            .into_iter()
            .map(|wrapped| {
                HyperlinkLine::new(
                    Line::from(style.text(wrapped.into_owned())).style(style.line_style()),
                )
            })
            .collect();
        prefix_hyperlink_lines(wrapped_lines, initial_prefix, subsequent_prefix)
    };

    if selected {
        rendered_lines = rendered_lines
            .into_iter()
            .map(|line| line.style(selection_style()))
            .collect();
    }
    rendered_lines
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
    let block_background = match kind {
        TranscriptKind::Output => palette::DARK,
        TranscriptKind::Tool | TranscriptKind::Diff => palette::SURFACE,
        TranscriptKind::System
        | TranscriptKind::User
        | TranscriptKind::Assistant
        | TranscriptKind::Plan
        | TranscriptKind::Separator
        | TranscriptKind::Status
        | TranscriptKind::Audit
        | TranscriptKind::Error => palette::SURFACE,
    };
    let label = kind.label();
    let label_width = label.width();
    let label_prefix_width = label_width + 3;
    let content_width = block_width.saturating_sub(label_prefix_width).max(1);
    let normalized_text = text.replace('\r', "\n").replace('\t', "    ");
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
            let wrapped_width = wrapped.width();
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
            let occupied_width = block_indent + label_prefix_width + wrapped_width;
            if occupied_width < width {
                spans.push(Span::styled(
                    " ".repeat(width - occupied_width),
                    Style::new().bg(block_background),
                ));
            }
            let mut line = Line::from(spans);
            for span in line.spans.iter_mut().skip(usize::from(block_indent > 0)) {
                span.style = span.style.patch(Style::new().bg(block_background));
            }
            if line_width(&line) > width {
                line = truncate_line_to_width(line, width);
            }
            let rendered_width = line_width(&line);
            if rendered_width < width {
                line.spans.push(Span::styled(
                    " ".repeat(width - rendered_width),
                    Style::new().bg(block_background),
                ));
            }
            HyperlinkLine::new(line)
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
            Self::Running => Style::new().fg(palette::CYAN).bg(palette::SURFACE),
            Self::Success => Style::new().fg(palette::SUCCESS).bg(palette::SURFACE),
            Self::Fail => Style::new().fg(palette::ERROR).bg(palette::SURFACE),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum LineStyle {
    Cyan,
    Dim,
    Green,
    Magenta,
    Red,
}

impl LineStyle {
    fn label_prefix(self, text: &str, selected: bool) -> Span<'static> {
        if selected {
            self.label(format!("▶ {}  ", text.to_uppercase()))
        } else {
            self.label(format!("▎ {}  ", text.to_uppercase()))
        }
    }

    fn label(self, text: String) -> Span<'static> {
        Span::styled(text, Style::new().fg(self.color()).bold())
    }

    fn text(self, text: String) -> Span<'static> {
        Span::styled(text, Style::new().fg(self.text_color()))
    }

    fn line_style(self) -> Style {
        Style::new().fg(self.text_color())
    }

    fn color(self) -> Color {
        match self {
            Self::Cyan => palette::CYAN,
            Self::Dim => palette::MUTED,
            Self::Green => palette::SUCCESS,
            Self::Magenta => palette::PURPLE,
            Self::Red => palette::ERROR,
        }
    }

    fn text_color(self) -> Color {
        match self {
            Self::Dim => palette::MUTED,
            Self::Green => palette::SUCCESS,
            Self::Red => palette::ERROR,
            Self::Cyan | Self::Magenta => palette::TEXT,
        }
    }
}

#[cfg(test)]
#[path = "transcript_view_tests.rs"]
mod tests;
