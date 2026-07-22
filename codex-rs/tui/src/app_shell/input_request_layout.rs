use super::PendingApproval;
use super::PendingElicitation;
use super::composer::ComposerState;
use super::design::body_rect_after_title;
use super::design::palette;
use super::design::pane_content_rect;
use super::input_request_view::approval_lines;
use super::input_request_view::elicitation_lines;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use crate::wrapping::wrap_ranges_trim;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RequestPanelHit {
    pub(super) line: usize,
    pub(super) column: usize,
}

#[derive(Debug, Clone)]
struct RequestPanelSegment {
    content: Line<'static>,
    logical_line: usize,
    source_column: usize,
    display_prefix_width: usize,
}

pub(super) fn request_panel_visual_line_count(lines: &[Line<'static>], width: u16) -> usize {
    wrapped_segments(lines, width).len()
}

pub(super) fn visible_request_panel_lines(
    lines: &[Line<'static>],
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    visible_segments(lines, width, height)
        .into_iter()
        .map(|segment| segment.content)
        .collect()
}

pub(super) fn visible_approval_panel_lines(
    pending: &PendingApproval,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    visible_approval_segments(pending, width, height)
        .into_iter()
        .map(|segment| segment.content)
        .collect()
}

pub(super) fn visible_elicitation_panel_lines(
    pending: &PendingElicitation,
    composer: &ComposerState,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    visible_elicitation_segments(pending, composer, width, height)
        .into_iter()
        .map(|segment| segment.content)
        .collect()
}

pub(super) fn request_panel_hit(
    panel: Rect,
    position: Position,
    lines: &[Line<'static>],
) -> Option<RequestPanelHit> {
    let body = body_rect_after_title(pane_content_rect(panel));
    if !body.contains(position) {
        return None;
    }
    let segments = visible_segments(lines, body.width, body.height);
    request_panel_hit_from_segments(body, position, &segments)
}

pub(super) fn approval_panel_hit(
    panel: Rect,
    position: Position,
    pending: &PendingApproval,
) -> Option<RequestPanelHit> {
    let body = body_rect_after_title(pane_content_rect(panel));
    if !body.contains(position) {
        return None;
    }
    let segments = visible_approval_segments(pending, body.width, body.height);
    request_panel_hit_from_segments(body, position, &segments)
}

pub(super) fn elicitation_panel_hit(
    panel: Rect,
    position: Position,
    pending: &PendingElicitation,
    composer: &ComposerState,
) -> Option<RequestPanelHit> {
    let body = body_rect_after_title(pane_content_rect(panel));
    if !body.contains(position) {
        return None;
    }
    let segments = visible_elicitation_segments(pending, composer, body.width, body.height);
    request_panel_hit_from_segments(body, position, &segments)
}

fn request_panel_hit_from_segments(
    body: Rect,
    position: Position,
    segments: &[RequestPanelSegment],
) -> Option<RequestPanelHit> {
    let segment = segments.get(usize::from(position.y.saturating_sub(body.y)))?;
    if segment.logical_line == usize::MAX {
        return None;
    }
    let display_column = usize::from(position.x.saturating_sub(body.x));
    let source_offset = display_column.checked_sub(segment.display_prefix_width)?;
    Some(RequestPanelHit {
        line: segment.logical_line,
        column: segment.source_column.saturating_add(source_offset),
    })
}

fn visible_approval_segments(
    pending: &PendingApproval,
    width: u16,
    height: u16,
) -> Vec<RequestPanelSegment> {
    let lines = approval_lines(pending);
    let segments = wrapped_segments(&lines, width);
    let visible = usize::from(height).min(segments.len());
    if visible == segments.len() || visible == 0 {
        pending.set_scroll_max(0);
        return segments.into_iter().take(visible).collect();
    }

    let preferred_pinned_line = pending.details().len().saturating_add(1);
    let final_line = lines.len().saturating_sub(1);
    let pinned_start = pinned_suffix_start(&segments, preferred_pinned_line, final_line, visible);
    let (selected, max_start) = visible_scrolled_segments(
        &segments,
        visible,
        pinned_start,
        pending.scroll_offset(),
        " (j/k)",
    );
    pending.set_scroll_max(max_start);
    selected
}

fn visible_elicitation_segments(
    pending: &PendingElicitation,
    composer: &ComposerState,
    width: u16,
    height: u16,
) -> Vec<RequestPanelSegment> {
    let lines = elicitation_lines(pending, composer, width);
    if pending.url().is_none() {
        pending.set_scroll_max(0);
        return visible_segments(&lines, width, height);
    }
    let segments = wrapped_segments(&lines, width);
    let visible = usize::from(height).min(segments.len());
    if visible == segments.len() || visible == 0 {
        pending.set_scroll_max(0);
        return segments.into_iter().take(visible).collect();
    }

    let final_line = lines.len().saturating_sub(1);
    let pinned_start =
        pinned_suffix_start(&segments, /*preferred_line*/ 2, final_line, visible);
    let (selected, max_start) = visible_scrolled_segments(
        &segments,
        visible,
        pinned_start,
        pending.scroll_offset(),
        " (j/k or arrows)",
    );
    pending.set_scroll_max(max_start);
    selected
}

fn pinned_suffix_start(
    segments: &[RequestPanelSegment],
    preferred_line: usize,
    final_line: usize,
    visible: usize,
) -> usize {
    let preferred_start = segments
        .iter()
        .position(|segment| segment.logical_line >= preferred_line)
        .unwrap_or(segments.len());
    let final_start = segments
        .iter()
        .position(|segment| segment.logical_line == final_line)
        .unwrap_or(segments.len());
    if segments.len().saturating_sub(preferred_start) <= visible.saturating_sub(2) {
        preferred_start
    } else {
        final_start
    }
}

fn visible_scrolled_segments(
    segments: &[RequestPanelSegment],
    visible: usize,
    pinned_start: usize,
    scroll_offset: usize,
    scroll_hint: &'static str,
) -> (Vec<RequestPanelSegment>, usize) {
    let (scrollable, pinned) = segments.split_at(pinned_start);
    let viewport = visible.saturating_sub(pinned.len()).saturating_sub(1);
    let max_start = scrollable.len().saturating_sub(viewport);
    let start = scroll_offset.min(max_start);
    let end = start.saturating_add(viewport).min(scrollable.len());
    let hidden_above = start > 0;
    let marker = overflow_marker(hidden_above, end < scrollable.len(), scroll_hint);
    let mut selected = Vec::with_capacity(visible);
    if hidden_above {
        selected.push(marker.clone());
    }
    selected.extend_from_slice(&scrollable[start..end]);
    if !hidden_above {
        selected.push(marker);
    }
    selected.extend_from_slice(pinned);
    selected.truncate(visible);
    (selected, max_start)
}

fn overflow_marker(
    hidden_above: bool,
    hidden_below: bool,
    scroll_hint: &'static str,
) -> RequestPanelSegment {
    let direction = match (hidden_above, hidden_below) {
        (true, true) => "↕",
        (true, false) => "↑",
        (false, true) => "↓",
        (false, false) => " ",
    };
    RequestPanelSegment {
        content: Line::from(vec![
            "  ".into(),
            format!("{direction} more").fg(palette::WARNING).bold(),
            scroll_hint.fg(palette::MUTED),
        ]),
        logical_line: usize::MAX,
        source_column: 0,
        display_prefix_width: 0,
    }
}

fn visible_segments(lines: &[Line<'static>], width: u16, height: u16) -> Vec<RequestPanelSegment> {
    let segments = wrapped_segments(lines, width);
    visible_segment_indices(&segments, lines.len(), height)
        .into_iter()
        .filter_map(|index| segments.get(index).cloned())
        .collect()
}

fn visible_segment_indices(
    segments: &[RequestPanelSegment],
    logical_line_count: usize,
    height: u16,
) -> Vec<usize> {
    let visible = usize::from(height).min(segments.len());
    if visible == segments.len() {
        return (0..segments.len()).collect();
    }
    if visible == 0 || logical_line_count == 0 {
        return Vec::new();
    }

    let final_line = logical_line_count.saturating_sub(1);
    if final_line == 0 {
        return (0..visible).collect();
    }
    let title = segments
        .iter()
        .enumerate()
        .find_map(|(index, segment)| (segment.logical_line == 0).then_some(index));
    let actions = segments
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| (segment.logical_line == final_line).then_some(index))
        .collect::<Vec<_>>();

    let action_count = actions
        .len()
        .min(visible.saturating_sub(usize::from(title.is_some())).max(1));
    let leading_count = visible.saturating_sub(action_count);
    let title_count = usize::from(title.is_some()).min(leading_count);
    let detail_count = leading_count.saturating_sub(title_count);
    let mut previous_logical_line = None;
    let mut selected = title
        .into_iter()
        .take(title_count)
        .chain(
            segments
                .iter()
                .enumerate()
                .filter_map(|(index, segment)| {
                    let first_segment = previous_logical_line != Some(segment.logical_line);
                    previous_logical_line = Some(segment.logical_line);
                    (first_segment
                        && segment.logical_line != 0
                        && segment.logical_line != final_line)
                        .then_some(index)
                })
                .take(detail_count),
        )
        .chain(actions.into_iter().take(action_count))
        .collect::<Vec<_>>();
    if selected.len() < visible {
        for index in 0..segments.len() {
            if selected.len() >= visible {
                break;
            }
            if !selected.contains(&index) {
                selected.push(index);
            }
        }
    }
    selected.sort_unstable();
    selected
}

fn wrapped_segments(lines: &[Line<'static>], width: u16) -> Vec<RequestPanelSegment> {
    let width = usize::from(width.max(/*other*/ 1));
    lines
        .iter()
        .enumerate()
        .flat_map(|(logical_line, line)| {
            let text = line_text(line);
            let continuation_indent = continuation_indent(line);
            let display_prefix_width = UnicodeWidthStr::width(continuation_indent.as_str());
            let range_options =
                textwrap::Options::new(width).subsequent_indent(&continuation_indent);
            let mut ranges = wrap_ranges_trim(&text, range_options);
            if ranges.is_empty() {
                ranges.push(0..0);
            }
            word_wrap_lines(
                vec![line.clone()],
                RtOptions::new(width).subsequent_indent(Line::from(continuation_indent)),
            )
            .into_iter()
            .zip(ranges)
            .enumerate()
            .map(
                move |(segment_index, (content, range))| RequestPanelSegment {
                    content,
                    logical_line,
                    source_column: UnicodeWidthStr::width(&text[..range.start]),
                    display_prefix_width: if segment_index == 0 {
                        0
                    } else {
                        display_prefix_width
                    },
                },
            )
        })
        .collect()
}

fn continuation_indent(line: &Line<'_>) -> String {
    line.spans
        .first()
        .map(|span| span.content.as_ref())
        .filter(|prefix| !prefix.is_empty() && prefix.chars().all(char::is_whitespace))
        .unwrap_or_default()
        .to_string()
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[cfg(test)]
#[path = "input_request_view_tests.rs"]
mod tests;
