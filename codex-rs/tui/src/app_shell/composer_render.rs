use super::LocalSlashCommand;
use super::design::body_rect_after_title;
use super::design::palette;
use super::design::pane_content_rect;
use super::shell_command::ShellCommand;
use crate::render::line_utils::push_owned_lines;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_line;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::ops::Range;
use textwrap::core::Fragment;
use textwrap::word_splitters::split_words;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const COMPOSER_INDENT_WIDTH: usize = 2;

pub(super) fn wrapped_composer_lines(
    text: &str,
    is_empty: bool,
    cursor: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(/*other*/ 1);
    let initial_indent = Line::from("> ".fg(palette::FOCUS));
    let subsequent_indent = Line::from("  ".fg(palette::MUTED));
    let mut wrapped_lines = Vec::new();
    if is_empty {
        let placeholder = "Type a message, Shift+Enter for newline"
            .fg(palette::MUTED)
            .into();
        let options = RtOptions::new(width)
            .initial_indent(initial_indent)
            .subsequent_indent(subsequent_indent);
        push_owned_lines(&word_wrap_line(&placeholder, options), &mut wrapped_lines);
        return wrapped_lines;
    }

    let command_range = composer_command_range(text);
    let Some(layout) = composer_layout(text, cursor, width) else {
        return wrapped_lines;
    };
    for (logical_index, (logical_line, line_layout)) in
        text.split('\n').zip(layout.lines).enumerate()
    {
        if logical_line.is_empty() {
            wrapped_lines.push(Line::default());
            continue;
        }
        let command_range = if logical_index == 0 {
            command_range.as_ref()
        } else {
            None
        };
        for (row_index, range) in line_layout.rows.into_iter().enumerate() {
            let indent = if logical_index == 0 && row_index == 0 {
                initial_indent.clone()
            } else {
                subsequent_indent.clone()
            };
            wrapped_lines.push(composer_row(logical_line, range, command_range, indent));
        }
    }
    wrapped_lines.resize_with(layout.cursor.line_count, Line::default);
    wrapped_lines
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

fn composer_command_range(text: &str) -> Option<Range<usize>> {
    let logical_line = text.split('\n').next().unwrap_or_default();
    if LocalSlashCommand::parse(text).is_some()
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
    }
}

fn composer_row(
    logical_line: &str,
    range: Range<usize>,
    command_range: Option<&Range<usize>>,
    mut row: Line<'static>,
) -> Line<'static> {
    let mut spans = row.spans;
    let Some(command_range) = command_range else {
        spans.push(logical_line[range].to_string().into());
        row.spans = spans;
        return row;
    };
    let command_start = range.start.max(command_range.start);
    let command_end = range.end.min(command_range.end);
    if command_start >= command_end {
        spans.push(logical_line[range].to_string().into());
    } else {
        spans.push(logical_line[range.start..command_start].to_string().into());
        spans.push(
            logical_line[command_start..command_end]
                .to_string()
                .fg(palette::FOCUS)
                .bold(),
        );
        spans.push(logical_line[command_end..range.end].to_string().into());
    }
    row.spans = spans;
    row
}

#[derive(Debug)]
struct ComposerFragment {
    range: Range<usize>,
    width: usize,
}

impl Fragment for ComposerFragment {
    fn width(&self) -> f64 {
        self.width as f64
    }

    fn whitespace_width(&self) -> f64 {
        0.0
    }

    fn penalty_width(&self) -> f64 {
        0.0
    }
}

struct ComposerLogicalLineLayout {
    rows: Vec<Range<usize>>,
}

struct ComposerLayout {
    lines: Vec<ComposerLogicalLineLayout>,
    cursor: ComposerVisualCursor,
}

fn composer_logical_line_layout(
    logical_line: &str,
    content_width: usize,
) -> ComposerLogicalLineLayout {
    if logical_line.is_empty() {
        return ComposerLogicalLineLayout {
            rows: std::iter::once(0..0).collect(),
        };
    }

    let mut fragments = Vec::new();
    let words = split_words(
        textwrap::WordSeparator::new().find_words(logical_line),
        &textwrap::WordSplitter::HyphenSplitter,
    );
    for word in words {
        if !word.word.is_empty() {
            let word_range = source_range(logical_line, word.word);
            if UnicodeWidthStr::width(word.word) <= content_width {
                fragments.push(ComposerFragment {
                    range: word_range,
                    width: UnicodeWidthStr::width(word.word),
                });
            } else {
                let mut chunk_start = word_range.start;
                let mut chunk_width = 0usize;
                for (offset, grapheme) in word.word.grapheme_indices(true) {
                    let width = UnicodeWidthStr::width(grapheme);
                    if chunk_width > 0 && chunk_width.saturating_add(width) > content_width {
                        fragments.push(ComposerFragment {
                            range: chunk_start..word_range.start + offset,
                            width: chunk_width,
                        });
                        chunk_start = word_range.start + offset;
                        chunk_width = 0;
                    }
                    chunk_width = chunk_width.saturating_add(width);
                }
                if chunk_start < word_range.end {
                    fragments.push(ComposerFragment {
                        range: chunk_start..word_range.end,
                        width: chunk_width,
                    });
                }
            }
        }

        if !word.whitespace.is_empty() {
            let whitespace_range = source_range(logical_line, word.whitespace);
            for (offset, grapheme) in word.whitespace.grapheme_indices(true) {
                fragments.push(ComposerFragment {
                    range: whitespace_range.start + offset
                        ..whitespace_range.start + offset + grapheme.len(),
                    width: UnicodeWidthStr::width(grapheme),
                });
            }
        }
    }

    // Standard wrapping drops separator whitespace, but editable spaces need cells.
    let rows = textwrap::wrap_algorithms::wrap_first_fit(&fragments, &[content_width as f64])
        .into_iter()
        .filter_map(|line| Some(line.first()?.range.start..line.last()?.range.end))
        .collect();
    ComposerLogicalLineLayout { rows }
}

fn source_range(source: &str, fragment: &str) -> Range<usize> {
    let source_start = source.as_ptr() as usize;
    let fragment_start = fragment.as_ptr() as usize;
    assert!(
        fragment_start >= source_start,
        "composer fragment should borrow from its logical line"
    );
    let start = fragment_start - source_start;
    assert!(
        start <= source.len() && fragment.len() <= source.len() - start,
        "composer fragment should be in bounds"
    );
    start..start + fragment.len()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ComposerVisualCursor {
    line: usize,
    column: usize,
    line_count: usize,
}

fn composer_visual_cursor(text: &str, cursor: usize, width: usize) -> Option<ComposerVisualCursor> {
    composer_layout(text, cursor, width).map(|layout| layout.cursor)
}

fn composer_layout(text: &str, cursor: usize, width: usize) -> Option<ComposerLayout> {
    let cursor = cursor.min(text.len());
    let line_start = text[..cursor]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let logical_cursor_line = text[..line_start].chars().filter(|ch| *ch == '\n').count();
    let cursor_in_logical_line = cursor.saturating_sub(line_start);
    let width = width.max(/*other*/ 1);
    let content_width = width.saturating_sub(COMPOSER_INDENT_WIDTH).max(/*other*/ 1);
    let indent_width = COMPOSER_INDENT_WIDTH.min(width.saturating_sub(1));
    let mut visual_line = 0usize;
    let mut target = None;
    let mut lines = Vec::new();

    for (index, logical_line) in text.split('\n').enumerate() {
        let layout = composer_logical_line_layout(logical_line, content_width);
        if index == logical_cursor_line {
            let logical_cursor = cursor_in_logical_line.min(logical_line.len());
            let range_index = layout
                .rows
                .partition_point(|range| range.start <= logical_cursor)
                .saturating_sub(1);
            let range_start = layout
                .rows
                .get(range_index)
                .map(|range| range.start)
                .unwrap_or(0);
            let display_columns =
                UnicodeWidthStr::width(&logical_line[range_start..logical_cursor]);
            target = Some((
                visual_line
                    .saturating_add(range_index)
                    .saturating_add(display_columns / content_width),
                indent_width.saturating_add(display_columns % content_width),
            ));
        }
        visual_line = visual_line.saturating_add(layout.rows.len().max(1));
        lines.push(layout);
    }

    target.map(|(line, column)| ComposerLayout {
        lines,
        cursor: ComposerVisualCursor {
            line,
            column,
            line_count: visual_line.max(line.saturating_add(1)),
        },
    })
}
