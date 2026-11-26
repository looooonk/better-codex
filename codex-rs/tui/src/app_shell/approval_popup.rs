use super::ApprovalAction;
use super::PendingApproval;
use super::design::palette;
use super::design::pane_style;
use super::input_request_layout::approval_panel_hit;
use super::input_request_layout::request_panel_visual_line_count;
use super::input_request_layout::visible_approval_panel_lines;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
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
    let hit = approval_panel_hit(panel, position, pending)?;
    let rows = ApprovalRows::new(pending);
    if rows.choices.contains(&hit.line) {
        return Some(ApprovalAction::Choose(hit.line - rows.choices.start));
    }
    if hit.line != rows.lines.len() - 1 {
        return None;
    }
    let footer = rows.lines.last()?;
    let mut start = 0;
    for (span, action) in footer.spans.iter().zip([
        Some(ApprovalAction::Edit),
        None,
        Some(ApprovalAction::Explain),
    ]) {
        let end = start + span.width();
        if (start..end).contains(&hit.column) {
            return action;
        }
        start = end;
    }
    None
}

pub(super) fn render(pending: &PendingApproval, transcript: Rect, buf: &mut Buffer) {
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
    let lines = visible_approval_panel_lines(pending, body.width, body.height);
    Paragraph::new(lines).render(body, buf);
}
