use super::design::body_rect_after_title;
use super::design::centered_band_rect;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::title_rect;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use crate::wrapping::wrap_ranges_trim;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ModalHit {
    pub(super) line: usize,
    pub(super) column: usize,
}

pub(super) fn render_modal(screen: Rect, title: &str, lines: Vec<Line<'static>>, buf: &mut Buffer) {
    let panel = modal_panel_area(screen, &lines);
    let content = pane_content_rect(panel);
    let body = body_rect_after_title(content);
    let segments = wrapped_segments(&lines, body.width);
    let visible = visible_segment_indices(&lines, &segments, body.height);
    let lines = visible
        .iter()
        .filter_map(|index| segments.get(*index))
        .map(|segment| segment.content.clone())
        .collect::<Vec<_>>();

    buf.set_style(screen, Style::new().fg(palette::MUTED).bg(palette::DARK));
    Clear.render(panel, buf);
    fill_rect(buf, panel, palette::ELEVATED);
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette::FOCUS))
        .style(pane_style(palette::ELEVATED))
        .render(panel, buf);
    Paragraph::new(Line::from(vec![
        "◆ ".fg(palette::PURPLE),
        title.to_uppercase().fg(palette::TEXT).bold(),
    ]))
    .style(pane_style(palette::ELEVATED))
    .render(title_rect(content), buf);
    Paragraph::new(lines)
        .style(pane_style(palette::ELEVATED))
        .render(body, buf);
}

pub(super) fn modal_hit(
    screen: Rect,
    position: Position,
    lines: &[Line<'static>],
) -> Option<ModalHit> {
    let panel = modal_panel_area(screen, lines);
    let body = body_rect_after_title(pane_content_rect(panel));
    if !body.contains(position) {
        return None;
    }
    let segments = wrapped_segments(lines, body.width);
    let visible = visible_segment_indices(lines, &segments, body.height);
    let index = *visible.get(usize::from(position.y.saturating_sub(body.y)))?;
    segment_hit(segments.get(index)?, body, position)
}

pub(super) fn panel_line_at(
    panel: Rect,
    position: Position,
    lines: &[Line<'static>],
) -> Option<ModalHit> {
    let body = body_rect_after_title(pane_content_rect(panel));
    if !body.contains(position) {
        return None;
    }

    let segments = wrapped_segments(lines, body.width);
    let index = usize::from(position.y.saturating_sub(body.y));
    segment_hit(segments.get(index)?, body, position)
}

pub(super) fn modal_panel_area(screen: Rect, lines: &[Line<'static>]) -> Rect {
    let probe = centered_band_rect(screen, /*height*/ 5);
    let body_width = body_rect_after_title(pane_content_rect(probe)).width;
    let height = u16::try_from(wrapped_lines(lines.to_vec(), body_width).len())
        .unwrap_or(u16::MAX)
        .saturating_add(4);
    centered_band_rect(screen, height)
}

fn wrapped_lines(lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    word_wrap_lines(lines, RtOptions::new(usize::from(width.max(/*other*/ 1))))
}

#[derive(Debug, Clone)]
struct WrappedSegment {
    content: Line<'static>,
    logical_line: usize,
    source_column: usize,
}

fn wrapped_segments(lines: &[Line<'static>], width: u16) -> Vec<WrappedSegment> {
    let width = usize::from(width.max(/*other*/ 1));
    lines
        .iter()
        .enumerate()
        .flat_map(|(logical_line, line)| {
            let text = line_text(line);
            let mut ranges = wrap_ranges_trim(&text, width);
            if ranges.is_empty() {
                ranges.push(0..0);
            }
            wrapped_lines(vec![line.clone()], u16::try_from(width).unwrap_or(u16::MAX))
                .into_iter()
                .zip(ranges)
                .map(move |(content, range)| WrappedSegment {
                    content,
                    logical_line,
                    source_column: UnicodeWidthStr::width(&text[..range.start]),
                })
        })
        .collect()
}

fn visible_segment_indices(
    lines: &[Line<'static>],
    segments: &[WrappedSegment],
    height: u16,
) -> Vec<usize> {
    let visible = usize::from(height).min(segments.len());
    if visible == segments.len() {
        return (0..segments.len()).collect();
    }
    let focus_line = lines
        .iter()
        .position(|line| line_text(line).starts_with("> "));
    let Some(focus) = focus_line.and_then(|line| {
        segments
            .iter()
            .position(|segment| segment.logical_line == line)
    }) else {
        return (0..visible).collect();
    };
    if visible < 5 {
        let start = focus
            .saturating_sub(visible / 2)
            .min(segments.len().saturating_sub(visible));
        return (start..start.saturating_add(visible)).collect();
    }

    let leading = 2.min(visible);
    let trailing = 2.min(visible.saturating_sub(leading));
    let middle_visible = visible.saturating_sub(leading + trailing);
    let middle_end = segments.len().saturating_sub(trailing);
    let middle_start = focus
        .saturating_sub(middle_visible / 2)
        .clamp(leading, middle_end.saturating_sub(middle_visible));
    (0..leading)
        .chain(middle_start..middle_start.saturating_add(middle_visible))
        .chain(middle_end..segments.len())
        .collect()
}

fn segment_hit(segment: &WrappedSegment, body: Rect, position: Position) -> Option<ModalHit> {
    Some(ModalHit {
        line: segment.logical_line,
        column: segment
            .source_column
            .saturating_add(usize::from(position.x.saturating_sub(body.x))),
    })
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}
