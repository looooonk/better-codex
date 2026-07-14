use super::design::body_rect_after_title;
use super::design::centered_band_rect;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::title_rect;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ModalHit {
    pub(super) line: usize,
    pub(super) column: usize,
}

pub(super) fn render_modal(screen: Rect, title: &str, lines: Vec<Line<'static>>, buf: &mut Buffer) {
    let panel = modal_panel_area(screen, &lines);
    let content = pane_content_rect(panel);
    let body = body_rect_after_title(content);
    let lines = wrapped_lines(lines, body.width);

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
    panel_line_at(modal_panel_area(screen, lines), position, lines)
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

    let visual_line = usize::from(position.y.saturating_sub(body.y));
    let mut offset = 0;
    for (line, content) in lines.iter().enumerate() {
        let wrapped = wrapped_lines(vec![content.clone()], body.width);
        let height = wrapped.len().max(1);
        if visual_line < offset + height {
            let wrapped_line = visual_line.saturating_sub(offset);
            let wrapped_prefix_width = wrapped
                .iter()
                .take(wrapped_line)
                .map(Line::width)
                .sum::<usize>()
                .saturating_add(wrapped_line);
            return Some(ModalHit {
                line,
                column: wrapped_prefix_width
                    .saturating_add(usize::from(position.x.saturating_sub(body.x))),
            });
        }
        offset += height;
    }
    None
}

fn modal_panel_area(screen: Rect, lines: &[Line<'static>]) -> Rect {
    let probe = centered_band_rect(screen, /*height*/ 5);
    let body_width = body_rect_after_title(pane_content_rect(probe)).width;
    let height = u16::try_from(wrapped_lines(lines.to_vec(), body_width).len())
        .unwrap_or(u16::MAX)
        .saturating_add(4);
    centered_band_rect(screen, height)
}

fn wrapped_lines(lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    word_wrap_lines(lines, RtOptions::new(usize::from(width.max(1))))
}
