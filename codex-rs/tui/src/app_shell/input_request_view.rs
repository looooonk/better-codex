use super::PendingApproval;
use super::PendingElicitation;
use super::PendingUserInput;
use super::design::body_rect_after_title;
use super::design::palette;
use super::design::pane_content_rect;
use crate::text_formatting::truncate_text;
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
    let segment = segments.get(usize::from(position.y.saturating_sub(body.y)))?;
    Some(RequestPanelHit {
        line: segment.logical_line,
        column: segment
            .source_column
            .saturating_add(usize::from(position.x.saturating_sub(body.x))),
    })
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
        .filter_map(|(index, segment)| (segment.logical_line == 0).then_some(index))
        .collect::<Vec<_>>();
    let actions = segments
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| (segment.logical_line == final_line).then_some(index))
        .collect::<Vec<_>>();

    let action_count = actions.len().min(
        visible
            .saturating_sub(usize::from(!title.is_empty()))
            .max(1),
    );
    let leading_count = visible.saturating_sub(action_count);
    let title_count = title.len().min(leading_count);
    let detail_count = leading_count.saturating_sub(title_count);
    let details = segments.iter().enumerate().filter_map(|(index, segment)| {
        (segment.logical_line != 0 && segment.logical_line != final_line).then_some(index)
    });

    title
        .into_iter()
        .take(title_count)
        .chain(details.take(detail_count))
        .chain(actions.into_iter().take(action_count))
        .collect()
}

fn wrapped_segments(lines: &[Line<'static>], width: u16) -> Vec<RequestPanelSegment> {
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
            word_wrap_lines(vec![line.clone()], RtOptions::new(width))
                .into_iter()
                .zip(ranges)
                .map(move |(content, range)| RequestPanelSegment {
                    content,
                    logical_line,
                    source_column: UnicodeWidthStr::width(&text[..range.start]),
                })
        })
        .collect()
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

pub(super) fn approval_lines(pending: &PendingApproval) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            "? ".fg(palette::WARNING).bold(),
            pending.title().to_string().fg(palette::TEXT).bold(),
        ]),
        Line::from(vec![
            "  ".into(),
            pending.detail().to_string().fg(palette::MUTED),
        ]),
        Line::from(vec![
            "  ".into(),
            " Approve ↵ ".fg(palette::DARK).bg(palette::SUCCESS).bold(),
            " ".into(),
            " Deny n ".fg(palette::TEXT).bg(palette::ERROR).bold(),
            " ".into(),
            " Edit e ".fg(palette::TEXT).bg(palette::ELEVATED).bold(),
            " ".into(),
            " Explain ? ".fg(palette::TEXT).bg(palette::ELEVATED).bold(),
        ]),
    ]
}

pub(super) fn user_input_lines(
    pending: &PendingUserInput,
    composer_text: &str,
    is_empty: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let (current, total) = pending.question_position();
    lines.push(Line::from(vec![
        "? ".cyan().bold(),
        format!("{} ({current}/{total})", pending.title()).bold(),
    ]));

    if let Some(question) = pending.current_question() {
        lines.push(Line::from(vec![
            "  ".into(),
            question.header.clone().bold(),
            ": ".dim(),
            question.question.clone().into(),
        ]));
    }

    let secret = pending
        .current_question()
        .is_some_and(|question| question.is_secret);
    let answer = if is_empty {
        "answer".dim()
    } else if secret {
        "[hidden]".dim()
    } else {
        composer_text.to_string().into()
    };
    let mut answer_line = vec!["> ".cyan().bold(), answer];
    if let Some(question) = pending.current_question()
        && let Some(options) = question.options.as_ref()
    {
        answer_line.push("  ".dim());
        answer_line.extend(
            options
                .iter()
                .take(3)
                .enumerate()
                .flat_map(|(index, option)| {
                    vec![
                        format!("{} ", index + 1).green().bold(),
                        option.label.clone().dim(),
                        "  ".dim(),
                    ]
                }),
        );
    }
    lines.push(Line::from(answer_line));
    lines
}

pub(super) fn elicitation_lines(pending: &PendingElicitation) -> Vec<Line<'static>> {
    let mut action_line = vec!["  ".into()];
    if pending.can_accept() {
        action_line.extend([
            " Accept ↵ ".fg(palette::DARK).bg(palette::SUCCESS).bold(),
            " ".into(),
        ]);
    }
    action_line.extend([
        " Decline d ".fg(palette::TEXT).bg(palette::ERROR).bold(),
        " ".into(),
        " Cancel c ".fg(palette::TEXT).bg(palette::ELEVATED).bold(),
    ]);

    vec![
        Line::from(vec!["? ".cyan().bold(), pending.title().to_string().bold()]),
        Line::from(vec![
            "  ".into(),
            truncate_text(pending.detail(), /*max_graphemes*/ 42).dim(),
        ]),
        Line::from(action_line),
    ]
}
