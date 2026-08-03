use super::ShellState;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::selection_style;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use ratatui::buffer::Buffer;
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
use std::ops::Range;

const MAX_POPUP_WIDTH: u16 = 72;

pub(super) fn render(shell: &ShellState, transcript: Rect, input: Rect, buf: &mut Buffer) {
    let Some(suggestions) = shell.slash_command_suggestions() else {
        return;
    };
    let height = u16::try_from(suggestions.entries().len())
        .unwrap_or(u16::MAX)
        .saturating_add(4)
        .min(transcript.height);
    if height < 5 {
        return;
    }
    let area = Rect::new(
        input.x,
        input.y.saturating_sub(height),
        input.width.min(MAX_POPUP_WIDTH),
        height,
    );
    let content = pane_content_rect(area);
    let visible = usize::from(content.height.saturating_sub(2));
    let range = visible_range(suggestions.selected(), suggestions.entries().len(), visible);

    Clear.render(area, buf);
    fill_rect(buf, area, palette::surface());
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette::focus()))
        .style(pane_style(palette::surface()))
        .render(area, buf);

    let mut lines = vec![
        vec![
            "◆ ".fg(palette::purple()),
            "COMMANDS".fg(palette::text()).bold(),
        ]
        .into(),
    ];
    lines.extend(
        suggestions
            .entries()
            .iter()
            .enumerate()
            .skip(range.start)
            .take(range.len())
            .map(|(index, definition)| {
                let selected = index == suggestions.selected();
                let marker = if selected {
                    "▌".fg(palette::focus())
                } else {
                    " ".into()
                };
                let line = vec![
                    marker,
                    " ".into(),
                    format!("{:<8}", definition.name())
                        .fg(palette::cyan())
                        .bold(),
                    definition.description().fg(palette::muted()),
                ]
                .into();
                let line =
                    truncate_line_with_ellipsis_if_overflow(line, usize::from(content.width));
                if selected {
                    line.style(selection_style())
                } else {
                    line
                }
            }),
    );
    lines.push(truncate_line_with_ellipsis_if_overflow(
        Line::from("↑↓ navigate   Tab complete   Enter run   Esc close").fg(palette::muted()),
        usize::from(content.width),
    ));
    Paragraph::new(lines)
        .style(pane_style(palette::surface()))
        .render(content, buf);
}

fn visible_range(selected: usize, entry_count: usize, visible: usize) -> Range<usize> {
    let visible = visible.min(entry_count);
    let start = selected
        .saturating_sub(visible / 2)
        .min(entry_count.saturating_sub(visible));
    start..start.saturating_add(visible)
}
