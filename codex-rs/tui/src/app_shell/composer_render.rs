use super::LocalSlashCommand;
use super::design::body_rect_after_title;
use super::design::palette;
use super::design::pane_content_rect;
use super::shell_command::ShellCommand;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use crate::wrapping::wrap_ranges;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

pub(super) fn wrapped_composer_lines(
    text: &str,
    is_empty: bool,
    width: usize,
) -> Vec<Line<'static>> {
    word_wrap_lines(composer_lines(text, is_empty), RtOptions::new(width.max(1)))
}

pub(super) fn composer_visual_cursor_line(
    text: &str,
    cursor: usize,
    width: usize,
) -> Option<usize> {
    composer_visual_cursor(text, cursor, width).map(|cursor| cursor.line)
}

pub(super) fn composer_cursor_position(
    input_area: Rect,
    text: &str,
    cursor: usize,
) -> Option<Position> {
    let body = body_rect_after_title(pane_content_rect(input_area));
    if body.width == 0 || body.height == 0 {
        return None;
    }

    let cursor = composer_visual_cursor(text, cursor, usize::from(body.width))?;
    let visible_height = usize::from(body.height);
    let visible_start = if cursor.line_count > visible_height {
        cursor
            .line
            .saturating_add(1)
            .saturating_sub(visible_height)
            .min(cursor.line_count.saturating_sub(visible_height))
    } else {
        0
    };
    let y = cursor.line.checked_sub(visible_start)?;
    if y >= visible_height {
        return None;
    }

    Some(Position {
        x: body
            .x
            .saturating_add(u16::try_from(cursor.column).unwrap_or(u16::MAX)),
        y: body.y.saturating_add(u16::try_from(y).unwrap_or(u16::MAX)),
    })
}

fn composer_lines(text: &str, is_empty: bool) -> Vec<Line<'static>> {
    if is_empty {
        return vec![Line::from(vec![
            "> ".fg(palette::FOCUS),
            "Type a message, Shift+Enter for newline".fg(palette::MUTED),
        ])];
    }

    let mut lines = Vec::new();
    for (index, logical_line) in text.split('\n').enumerate() {
        if logical_line.is_empty() {
            lines.push(Line::default());
            continue;
        }

        let prefix = if index == 0 {
            "> ".fg(palette::FOCUS)
        } else {
            "  ".fg(palette::MUTED)
        };
        let mut spans = vec![prefix];
        let command_range = if index != 0 {
            None
        } else if LocalSlashCommand::parse(text).is_some()
            && let Some(command_start) = logical_line.find('/')
        {
            let command_end = logical_line[command_start..]
                .find(char::is_whitespace)
                .map(|offset| command_start + offset)
                .unwrap_or(logical_line.len());
            Some(command_start..command_end)
        } else if ShellCommand::parse(text).is_some()
            && let Some(command_start) = logical_line.find('!')
        {
            Some(command_start..command_start + 1)
        } else {
            None
        };
        if let Some(command_range) = command_range {
            spans.push(logical_line[..command_range.start].to_string().into());
            spans.push(
                logical_line[command_range.clone()]
                    .to_string()
                    .fg(palette::FOCUS)
                    .bold(),
            );
            spans.push(logical_line[command_range.end..].to_string().into());
        } else {
            spans.push(logical_line.to_string().into());
        }
        lines.push(Line::from(spans));
    }
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ComposerVisualCursor {
    line: usize,
    column: usize,
    line_count: usize,
}

fn composer_visual_cursor(text: &str, cursor: usize, width: usize) -> Option<ComposerVisualCursor> {
    let cursor = cursor.min(text.len());
    let line_start = text[..cursor]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let logical_cursor_line = text[..line_start].chars().filter(|ch| *ch == '\n').count();
    let cursor_in_logical_line = cursor.saturating_sub(line_start);
    let width = width.max(1);
    let options = textwrap::Options::new(width).wrap_algorithm(textwrap::WrapAlgorithm::FirstFit);
    let mut visual_line = 0usize;
    let mut target = None;

    for (index, logical_line) in text.split('\n').enumerate() {
        if logical_line.is_empty() {
            if index == logical_cursor_line {
                target = Some((visual_line, 2.min(width.saturating_sub(1))));
            }
            visual_line = visual_line.saturating_add(1);
            continue;
        }

        let prefix = if index == 0 { "> " } else { "  " };
        let display_line = format!("{prefix}{logical_line}");
        let ranges = wrap_ranges(&display_line, options.clone());
        let wrapped_line_count = ranges.len().max(1);
        if index == logical_cursor_line {
            let display_cursor = prefix
                .len()
                .saturating_add(cursor_in_logical_line)
                .min(display_line.len());
            let range_index = ranges
                .partition_point(|range| range.start <= display_cursor)
                .saturating_sub(1);
            let range_start = ranges
                .get(range_index)
                .map(|range| range.start)
                .unwrap_or(0);
            target = Some((
                visual_line.saturating_add(range_index),
                UnicodeWidthStr::width(&display_line[range_start..display_cursor]),
            ));
        }
        visual_line = visual_line.saturating_add(wrapped_line_count);
    }

    target.map(|(line, column)| ComposerVisualCursor {
        line,
        column,
        line_count: visual_line,
    })
}
