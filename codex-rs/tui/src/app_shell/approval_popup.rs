use super::ApprovalAction;
use super::PendingApproval;
use super::design::palette;
use super::input_request_layout::RequestPanelSegment;
use super::input_request_layout::request_panel_visual_line_count;
use super::input_request_layout::visible_approval_segments;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Widget;
use std::ops::Range;

pub(super) struct ApprovalRows {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) choices: Range<usize>,
}

impl ApprovalRows {
    pub(super) fn new(pending: &PendingApproval) -> Self {
        let mut lines = vec![
            Line::from(
                pending
                    .reason()
                    .unwrap_or("Allow this action?")
                    .to_owned()
                    .fg(palette::text())
                    .bold(),
            ),
            Line::default(),
        ];
        let start = lines.len();
        lines.extend(pending.options().map(|(index, label)| {
            let selected = index == pending.selected_option();
            let color = if selected {
                palette::focus()
            } else {
                palette::text()
            };
            let option = Line::from(vec!["○ ".into(), label.to_owned().into()]).fg(color);
            if selected { option.bold() } else { option }
        }));
        let choices = start..lines.len();
        lines.push(Line::default());
        lines.push(Line::from(pending.title().to_owned().fg(palette::muted())));
        lines.extend(
            pending
                .details()
                .map(|detail| Line::from(detail.clone().fg(palette::muted()))),
        );
        Self { lines, choices }
    }

    fn choice_line_at(
        &self,
        body: Rect,
        segments: &[RequestPanelSegment],
        position: Position,
    ) -> Option<usize> {
        if !body.contains(position) {
            return None;
        }
        let line = segments.get(usize::from(position.y - body.y))?.logical_line;
        self.choices.contains(&line).then_some(line)
    }
}

pub(super) fn area(pending: &PendingApproval, transcript: Rect) -> Rect {
    let width = super::request_popup::width(transcript);
    let rows =
        request_panel_visual_line_count(&ApprovalRows::new(pending).lines, width.saturating_sub(4));
    super::request_popup::area(transcript, rows)
}

pub(super) fn action_at(
    pending: &PendingApproval,
    panel: Rect,
    position: Position,
) -> Option<ApprovalAction> {
    let body = super::request_popup::body(panel);
    if !body.contains(position) {
        return None;
    }
    let rows = ApprovalRows::new(pending);
    let segments = visible_approval_segments(pending, &rows, body.width, body.height);
    rows.choice_line_at(body, &segments, position)
        .map(|line| ApprovalAction::Choose(line - rows.choices.start))
}

pub(super) fn render(
    pending: &PendingApproval,
    transcript: Rect,
    pointer: Option<Position>,
    buf: &mut Buffer,
) {
    let panel = area(pending, transcript);
    super::request_popup::render_frame(panel, " Approval required ", buf);
    let body = super::request_popup::body(panel);
    let rows = ApprovalRows::new(pending);
    let segments = visible_approval_segments(pending, &rows, body.width, body.height);
    let hovered = pointer.and_then(|position| rows.choice_line_at(body, &segments, position));
    for (row, segment) in body.rows().zip(segments) {
        segment.content.render(row, buf);
        if hovered == Some(segment.logical_line) {
            buf.set_style(row, Style::new().bg(palette::border()));
        }
    }
}
