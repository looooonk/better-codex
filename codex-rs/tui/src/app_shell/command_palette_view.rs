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
use std::ops::Range;
use unicode_width::UnicodeWidthStr;

pub(super) fn entry_at(shell: &ShellState, area: Rect, position: Position) -> Option<usize> {
    let state = shell.command_palette.as_ref()?;
    let entries = shell.command_palette_entries();
    let content = pane_content_rect(palette_area(area, entries.len()));
    if !content.contains(position) {
        return None;
    }
    let range = visible_entry_range(state.selected(), entries.len(), content.height);
    let visible_index = usize::from(position.y.saturating_sub(content.y)).checked_sub(2)?;
    let index = range.start.saturating_add(visible_index);
    (index < range.end).then_some(index)
}

pub(super) fn render(shell: &ShellState, area: Rect, buf: &mut Buffer) {
    let Some(state) = &shell.command_palette else {
        return;
    };
    let entries = shell.command_palette_entries();
    let palette_area = palette_area(area, entries.len());
    let content = pane_content_rect(palette_area);
    let visible_range = visible_entry_range(state.selected(), entries.len(), content.height);
    buf.set_style(area, Style::new().fg(palette::MUTED).bg(palette::DARK));
    Clear.render(palette_area, buf);

    let hovered = shell
        .pointer_position
        .and_then(|position| entry_at(shell, area, position));
    let lines = entries
        .iter()
        .enumerate()
        .skip(visible_range.start)
        .take(visible_range.len())
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
    palette_lines.push(palette_footer(
        state.selected(),
        entries.len(),
        &visible_range,
        content.width,
    ));
    Paragraph::new(palette_lines)
        .style(pane_style(palette::ELEVATED))
        .render(content, buf);
}

pub(super) fn palette_area(area: Rect, entry_count: usize) -> Rect {
    let height = u16::try_from(entry_count)
        .unwrap_or(u16::MAX)
        .saturating_add(5)
        .min(area.height);
    centered_band_rect(area, height)
}

fn visible_entry_range(selected: usize, entry_count: usize, content_height: u16) -> Range<usize> {
    let visible = usize::from(content_height.saturating_sub(3)).min(entry_count);
    let start = selected
        .saturating_sub(visible / 2)
        .min(entry_count.saturating_sub(visible));
    start..start.saturating_add(visible)
}

fn palette_footer(
    selected: usize,
    entry_count: usize,
    visible_range: &Range<usize>,
    width: u16,
) -> Line<'static> {
    let before = if visible_range.start > 0 { "↑" } else { "" };
    let after = if visible_range.end < entry_count {
        "↓"
    } else {
        ""
    };
    let position = format!(
        "{before}{}/{}{after}",
        selected.saturating_add(1),
        entry_count
    );
    let hints = [
        "↑↓ / j k navigate   Enter select   Esc close",
        "j k navigate   Enter select   Esc close",
        "j k move   Enter select   Esc close",
        "j k   Enter   Esc",
        "",
    ];
    let hint = hints
        .into_iter()
        .find(|hint| {
            hint.width()
                .saturating_add(position.width())
                .saturating_add(usize::from(!hint.is_empty()) * 2)
                <= usize::from(width)
        })
        .unwrap_or_default();
    let gap = if hint.is_empty() { "" } else { "  " };
    Line::from(vec![
        hint.to_string().fg(palette::MUTED),
        gap.into(),
        position.fg(palette::PURPLE).bold(),
    ])
}
