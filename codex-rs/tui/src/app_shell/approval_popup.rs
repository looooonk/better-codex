use super::ApprovalAction;
use super::PendingApproval;
use super::design::palette;
use super::design::pane_style;
use super::input_request_layout::RequestPanelSegment;
use super::input_request_layout::request_panel_visual_line_count;
use super::input_request_layout::visible_approval_segments;
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
    let width = (u32::from(transcript.width) * 7 / 10) as u16;
    let rows =
        request_panel_visual_line_count(&ApprovalRows::new(pending).lines, width.saturating_sub(4));
    let height = u16::try_from(rows)
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .min(22)
        .min(transcript.height);
    Rect::new(
        transcript.x + (transcript.width - width) / 2,
        transcript.bottom() - height,
        width,
        height,
    )
}

pub(super) fn body(panel: Rect) -> Rect {
    Rect::new(
        panel.x + 2,
        panel.y + 1,
        panel.width.saturating_sub(4),
        panel.height.saturating_sub(2),
    )
}

pub(super) fn action_at(
    pending: &PendingApproval,
    panel: Rect,
    position: Position,
) -> Option<ApprovalAction> {
    let body = body(panel);
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
    Clear.render(panel, buf);
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(pane_style(palette::surface()))
        .border_style(palette::focus())
        .title(" Approval required ".fg(palette::focus()).bold())
        .render(panel, buf);
    let body = body(panel);
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
