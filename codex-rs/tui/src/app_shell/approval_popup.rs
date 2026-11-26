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

const EDIT_BUTTON: &str = "[e Edit]";

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
        lines.push(Line::default());
        lines.push(Line::from(vec![
            EDIT_BUTTON.fg(palette::cyan()).bg(palette::elevated()),
            "  ".into(),
            format!("[{}]", pending.explain_label())
                .fg(palette::cyan())
                .bg(palette::elevated()),
        ]));
        Self { lines, choices }
    }
    // Geometry is shared by clicks and hover. A choice owns its whole row,
    // while each footer button owns only its visible cells after wrapping.
    fn actions(&self, segment: &RequestPanelSegment, row: Rect) -> Vec<(ApprovalAction, Rect)> {
        if self.choices.contains(&segment.logical_line) {
            return vec![(
                ApprovalAction::Choose(segment.logical_line - self.choices.start),
                row,
            )];
        }
        if segment.logical_line != self.lines.len() - 1 {
            return Vec::new();
        }
        let footer = &self.lines[segment.logical_line];
        let visible_start = segment.source_column;
        let visible_end = visible_start
            + segment
                .content
                .width()
                .saturating_sub(segment.display_prefix_width);
        let mut start = 0;
        let mut buttons = Vec::new();
        for (span, action) in footer.spans.iter().zip([
            Some(ApprovalAction::Edit),
            None,
            Some(ApprovalAction::Explain),
        ]) {
            let end = start + span.width();
            let left = start.max(visible_start);
            let right = end.min(visible_end);
            if let Some(action) = action
                && left < right
            {
                let x = segment.display_prefix_width + left - visible_start;
                buttons.push((
                    action,
                    Rect::new(row.x + x as u16, row.y, (right - left) as u16, 1).intersection(row),
                ));
            }
            start = end;
        }
        buttons
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
    let segment = segments.get(usize::from(position.y - body.y))?;
    let row = Rect::new(body.x, position.y, body.width, 1);
    rows.actions(segment, row)
        .into_iter()
        .find_map(|(action, rect)| rect.contains(position).then_some(action))
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
    let mut actions = Vec::new();
    for (row, segment) in body.rows().zip(segments) {
        if pointer.is_some_and(|position| body.contains(position)) {
            actions.extend(rows.actions(&segment, row));
        }
        segment.content.render(row, buf);
    }
    if let Some(hovered) = actions
        .iter()
        .find_map(|(action, rect)| pointer.filter(|p| rect.contains(*p)).map(|_| *action))
    {
        for (action, rect) in actions {
            if action == hovered {
                buf.set_style(rect, Style::new().bg(palette::border()));
            }
        }
    }
}
