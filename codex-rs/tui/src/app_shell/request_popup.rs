use super::design::palette;
use super::design::pane_style;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

pub(super) fn width(transcript: Rect) -> u16 {
    (u32::from(transcript.width) * 7 / 10) as u16
}

pub(super) fn area(transcript: Rect, rows: usize) -> Rect {
    let width = width(transcript);
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

pub(super) fn render_frame(panel: Rect, title: &str, buf: &mut Buffer) {
    Clear.render(panel, buf);
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(pane_style(palette::surface()))
        .border_style(palette::focus())
        .title(title.fg(palette::focus()).bold())
        .render(panel, buf);
}
