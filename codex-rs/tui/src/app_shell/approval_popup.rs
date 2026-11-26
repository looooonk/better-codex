use super::PendingApproval;
use super::design::fill_rect;
use super::design::palette;
use super::input_request_layout::request_panel_visual_line_count;
use super::input_request_layout::visible_approval_panel_lines;
use super::input_request_view::approval_lines;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

pub(super) fn area(pending: &PendingApproval, transcript: Rect) -> Rect {
    let width = transcript.width.min(76);
    let rows = request_panel_visual_line_count(&approval_lines(pending), width.saturating_sub(3));
    let height = u16::try_from(rows)
        .unwrap_or(u16::MAX)
        .saturating_add(1)
        .min(20)
        .min(transcript.height);
    Rect::new(
        transcript.x,
        transcript.bottom().saturating_sub(height),
        width,
        height,
    )
}

pub(super) fn body(panel: Rect) -> Rect {
    Rect::new(
        panel.x + 2,
        panel.y + 1,
        panel.width.saturating_sub(3),
        panel.height.saturating_sub(1),
    )
}

pub(super) fn render(pending: &PendingApproval, transcript: Rect, buf: &mut Buffer) {
    let panel = area(pending, transcript);
    Clear.render(panel, buf);
    fill_rect(buf, panel, palette::elevated());
    Block::new()
        .borders(Borders::LEFT)
        .border_style(palette::focus())
        .render(panel, buf);
    let body = body(panel);
    Paragraph::new("Approval required")
        .fg(palette::focus())
        .render(Rect::new(body.x, panel.y, body.width, 1), buf);
    let lines = visible_approval_panel_lines(pending, body.width, body.height);
    Paragraph::new(lines).render(body, buf);
}
