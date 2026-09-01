use super::ShellState;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::design::selection_style;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
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
use std::ops::Range;

const MAX_POPUP_WIDTH: u16 = 72;
const MAX_VISIBLE_MESSAGES: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedMessagePopupLayout {
    area: Rect,
    content: Rect,
    visible_messages: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueuedMessagePopupHit {
    Message(usize),
    Chrome,
}

pub(super) fn hit_at(
    shell: &ShellState,
    transcript: Rect,
    input: Rect,
    position: Position,
) -> Option<QueuedMessagePopupHit> {
    let layout = popup_layout(shell, transcript, input)?;
    if !layout.area.contains(position) {
        return None;
    }
    if !layout.content.contains(position) {
        return Some(QueuedMessagePopupHit::Chrome);
    }
    let Some(visible_index) =
        usize::from(position.y.saturating_sub(layout.content.y)).checked_sub(1)
    else {
        return Some(QueuedMessagePopupHit::Chrome);
    };
    let index = layout.visible_messages.start.saturating_add(visible_index);
    Some(if index < layout.visible_messages.end {
        QueuedMessagePopupHit::Message(index)
    } else {
        QueuedMessagePopupHit::Chrome
    })
}

pub(super) fn render(shell: &ShellState, transcript: Rect, input: Rect, buf: &mut Buffer) {
    let Some(layout) = popup_layout(shell, transcript, input) else {
        return;
    };
    let hovered = shell.pointer_position.and_then(|position| {
        match hit_at(shell, transcript, input, position) {
            Some(QueuedMessagePopupHit::Message(index)) => Some(index),
            Some(QueuedMessagePopupHit::Chrome) | None => None,
        }
    });
    let editing = shell
        .composer
        .queued_edit_position()
        .map(|(index, _count)| index.saturating_sub(1));

    Clear.render(layout.area, buf);
    fill_rect(buf, layout.area, palette::surface());
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette::focus()))
        .style(pane_style(palette::surface()))
        .render(layout.area, buf);

    let mut lines = vec![
        vec![
            "◆ ".fg(palette::purple()),
            "QUEUED".fg(palette::text()).bold(),
            format!("  {}", shell.composer.queued_count()).fg(palette::muted()),
        ]
        .into(),
    ];
    lines.extend(
        shell
            .composer
            .queued_messages()
            .skip(layout.visible_messages.start)
            .take(layout.visible_messages.len())
            .map(|(index, message)| {
                let selected = editing == Some(index);
                let marker = if selected {
                    "▌".fg(palette::focus())
                } else {
                    " ".into()
                };
                let message = message.split_whitespace().collect::<Vec<_>>().join(" ");
                let line = truncate_line_with_ellipsis_if_overflow(
                    vec![
                        marker,
                        format!(" {:>2}  ", index.saturating_add(1)).fg(palette::muted()),
                        message.fg(palette::text()),
                    ]
                    .into(),
                    usize::from(layout.content.width),
                );
                if hovered == Some(index) {
                    line.style(Style::new().bg(palette::border()))
                } else if selected {
                    line.style(selection_style())
                } else {
                    line
                }
            }),
    );
    lines.push(truncate_line_with_ellipsis_if_overflow(
        Line::from("Click to edit   Alt+↑↓ navigate   Shift+Alt+↑↓ reorder").fg(palette::muted()),
        usize::from(layout.content.width),
    ));
    Paragraph::new(lines)
        .style(pane_style(palette::surface()))
        .render(layout.content, buf);
}

fn popup_layout(
    shell: &ShellState,
    transcript: Rect,
    input: Rect,
) -> Option<QueuedMessagePopupLayout> {
    if !shell.composer_owns_focus() || shell.slash_command_suggestions().is_some() {
        return None;
    }
    let message_count = shell.composer.queued_count();
    let visible_count = message_count
        .min(MAX_VISIBLE_MESSAGES)
        .min(usize::from(transcript.height.saturating_sub(4)));
    if visible_count == 0 || input.width == 0 {
        return None;
    }
    let height = u16::try_from(visible_count)
        .unwrap_or(u16::MAX)
        .saturating_add(4);
    let area = Rect::new(
        input.x,
        input.y.saturating_sub(height),
        input.width.min(MAX_POPUP_WIDTH),
        height,
    );
    Some(QueuedMessagePopupLayout {
        area,
        content: pane_content_rect(area),
        visible_messages: message_count.saturating_sub(visible_count)..message_count,
    })
}

#[cfg(test)]
#[path = "queued_message_popup_view_tests.rs"]
mod tests;
