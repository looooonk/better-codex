use super::ShellState;
use super::design::centered_band_rect;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::selection_style;
use crate::text_formatting::truncate_text;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

pub(super) fn entry_at(shell: &ShellState, area: Rect, position: Position) -> Option<usize> {
    shell.command_palette.as_ref()?;
    let entries = shell.command_palette_entries();
    let content = pane_content_rect(palette_area(area, entries.len()));
    if !content.contains(position) {
        return None;
    }
    let index = usize::from(position.y.saturating_sub(content.y)).checked_sub(2)?;
    (index < entries.len()).then_some(index)
}

pub(super) fn render(shell: &ShellState, area: Rect, buf: &mut Buffer) {
    let Some(state) = &shell.command_palette else {
        return;
    };
    let entries = shell.command_palette_entries();
    let palette_area = palette_area(area, entries.len());
    let content = pane_content_rect(palette_area);
    buf.set_style(area, Style::new().fg(palette::MUTED).bg(palette::DARK));
    let shadow = Rect::new(
        palette_area.x.saturating_add(1),
        palette_area.y.saturating_add(1),
        palette_area.width.min(
            area.right()
                .saturating_sub(palette_area.x.saturating_add(1)),
        ),
        palette_area.height.min(
            area.bottom()
                .saturating_sub(palette_area.y.saturating_add(1)),
        ),
    );
    fill_rect(buf, shadow, palette::DARK);
    Clear.render(palette_area, buf);

    let hovered = shell
        .pointer_position
        .and_then(|position| entry_at(shell, area, position));
    let lines = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let selected = index == state.selected();
            let marker = if selected {
                "▌".set_style(Style::new().fg(palette::FOCUS))
            } else {
                " ".into()
            };
            let title = entry
                .title
                .to_string()
                .set_style(Style::new().fg(if entry.enabled {
                    palette::TEXT
                } else {
                    palette::MUTED
                }));
            let detail = if selected {
                format!("  {}", truncate_text(entry.detail, /*max_graphemes*/ 34))
                    .set_style(Style::new().fg(palette::MUTED))
            } else {
                String::new().into()
            };
            let line = Line::from(vec![marker, " ".dim(), title, detail]);
            if hovered == Some(index) {
                line.style(Style::new().bg(palette::BORDER))
            } else if selected {
                line.style(selection_style())
            } else {
                line
            }
        })
        .collect::<Vec<_>>();

    fill_rect(buf, palette_area, palette::ELEVATED);
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette::FOCUS))
        .style(pane_style(palette::ELEVATED))
        .render(palette_area, buf);
    let mut palette_lines = vec![
        Line::from(vec![
            "◆ ".set_style(Style::new().fg(palette::PURPLE)),
            "ACTIONS".set_style(Style::new().fg(palette::TEXT).bold()),
            "  Ctrl+P".set_style(Style::new().fg(palette::MUTED)),
        ]),
        Line::from(""),
    ];
    palette_lines.extend(lines);
    palette_lines.push(Line::from(
        "  ↑↓ / j k navigate   Enter select   Esc close".set_style(Style::new().fg(palette::MUTED)),
    ));
    Paragraph::new(palette_lines)
        .style(pane_style(palette::ELEVATED))
        .wrap(Wrap { trim: true })
        .render(content, buf);
}

fn palette_area(area: Rect, entry_count: usize) -> Rect {
    let height = u16::try_from(entry_count)
        .unwrap_or(u16::MAX)
        .saturating_add(5)
        .min(area.height);
    centered_band_rect(area, height)
}
