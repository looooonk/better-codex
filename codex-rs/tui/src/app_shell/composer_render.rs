use super::LocalSlashCommand;
use super::design::body_rect_after_title;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::text_selection_style;
use super::shell_command::ShellCommand;
use crate::render::line_utils::push_owned_lines;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_line;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
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
    wrapped_composer_lines_with_selection(text, is_empty, cursor, width, /*selection*/ None)
}

pub(super) fn wrapped_composer_lines_with_selection(
    text: &str,
    is_empty: bool,
    cursor: usize,
    width: usize,
    selection: Option<Range<usize>>,
) -> Vec<Line<'static>> {
    let width = width.max(/*other*/ 1);
    let initial_indent = Line::from("> ".fg(palette::focus()));
    let subsequent_indent = Line::from("  ".fg(palette::muted()));
    let mut wrapped_lines = Vec::new();
    if is_empty {
        let placeholder = "Type a message, Shift+Enter for newline"
            .fg(palette::muted())
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
            wrapped_lines.push(composer_row(
                logical_line,
                line_layout.source_start,
                range,
                command_range,
                selection.as_ref(),
                indent,
            ));
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
    let viewport = composer_viewport(input_area, text, cursor)?;
    let y = viewport
        .layout
        .cursor
        .line
        .checked_sub(viewport.visible_start)?;
    if y >= viewport.visible_height {
        return None;
    }

    Some(Position {
        x: viewport
            .body
            .x
            .saturating_add(u16::try_from(viewport.layout.cursor.column).unwrap_or(u16::MAX)),
        y: viewport
            .body
            .y
            .saturating_add(u16::try_from(y).unwrap_or(u16::MAX)),
    })
}

/// The display-byte range of a composer grapheme and the nearest caret for the same hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComposerTextHit {
    grapheme_range: Range<usize>,
    caret_range: Range<usize>,
}

impl ComposerTextHit {
    pub(super) fn grapheme_range(&self) -> Range<usize> {
        self.grapheme_range.clone()
    }

    pub(super) fn caret_range(&self) -> Range<usize> {
        self.caret_range.clone()
    }
}

/// Resolve a position inside the composer body to a display grapheme and caret.
pub(super) fn composer_text_hit_inside(
    input_area: Rect,
    text: &str,
    cursor: usize,
    position: Position,
) -> Option<ComposerTextHit> {
    let viewport = composer_viewport(input_area, text, cursor)?;
    if !viewport.body.contains(position) {
        return None;
    }
    composer_text_hit(&viewport, text, position)
}

/// Resolve a position to the closest hit in the cursor-following visible composer viewport.
pub(super) fn composer_text_hit_clamped_to_visible_viewport(
    input_area: Rect,
    text: &str,
    cursor: usize,
    position: Position,
) -> Option<ComposerTextHit> {
    let viewport = composer_viewport(input_area, text, cursor)?;
    let position = Position::new(
        position.x.clamp(
            viewport.body.x,
            viewport
                .body
                .x
                .saturating_add(viewport.body.width.saturating_sub(1)),
        ),
        position.y.clamp(
            viewport.body.y,
            viewport
                .body
                .y
                .saturating_add(viewport.body.height.saturating_sub(1)),
        ),
    );
    composer_text_hit(&viewport, text, position)
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
    source_start: usize,
    range: Range<usize>,
    command_range: Option<&Range<usize>>,
    selection: Option<&Range<usize>>,
    mut row: Line<'static>,
) -> Line<'static> {
    let mut spans = row.spans;
    let mut current_text = String::new();
    let mut current_style = None;
    for (offset, grapheme) in logical_line[range.clone()].grapheme_indices(true) {
        let start = range.start + offset;
        let local_range = start..start + grapheme.len();
        let display_range = source_start + local_range.start..source_start + local_range.end;
        let style = composer_grapheme_style(&local_range, &display_range, command_range, selection);
        if let Some(previous_style) = current_style
            && previous_style != style
        {
            spans.push(Span::styled(
                std::mem::take(&mut current_text),
                previous_style,
            ));
        }
        current_style = Some(style);
        current_text.push_str(grapheme);
    }
    if let Some(style) = current_style {
        spans.push(Span::styled(current_text, style));
    }
    row.spans = spans;
    row
}

fn composer_grapheme_style(
    local_range: &Range<usize>,
    display_range: &Range<usize>,
    command_range: Option<&Range<usize>>,
    selection: Option<&Range<usize>>,
) -> Style {
    let mut style = Style::new();
    if command_range.is_some_and(|command| ranges_overlap(local_range, command)) {
        style = style.fg(palette::focus()).bold();
    }
    if selection.is_some_and(|selection| ranges_overlap(display_range, selection)) {
        style = style.patch(text_selection_style());
    }
    style
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
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
    source_start: usize,
    rows: Vec<Range<usize>>,
}

struct ComposerLayout {
    lines: Vec<ComposerLogicalLineLayout>,
    cursor: ComposerVisualCursor,
    indent_width: usize,
}

fn composer_logical_line_layout(
    logical_line: &str,
    content_width: usize,
    source_start: usize,
) -> ComposerLogicalLineLayout {
    if logical_line.is_empty() {
        return ComposerLogicalLineLayout {
            source_start,
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
    ComposerLogicalLineLayout { source_start, rows }
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
    let mut source_start = 0usize;

    for (index, logical_line) in text.split('\n').enumerate() {
        let layout = composer_logical_line_layout(logical_line, content_width, source_start);
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
        source_start = source_start
            .saturating_add(logical_line.len())
            .saturating_add(1);
    }

    target.map(|(line, column)| ComposerLayout {
        lines,
        cursor: ComposerVisualCursor {
            line,
            column,
            line_count: visual_line.max(line.saturating_add(1)),
        },
        indent_width,
    })
}

struct ComposerViewport {
    body: Rect,
    layout: ComposerLayout,
    visible_start: usize,
    visible_height: usize,
}

fn composer_viewport(input_area: Rect, text: &str, cursor: usize) -> Option<ComposerViewport> {
    let body = body_rect_after_title(pane_content_rect(input_area));
    if body.width == 0 || body.height == 0 {
        return None;
    }

    let layout = composer_layout(text, cursor, usize::from(body.width))?;
    let visible_height = usize::from(body.height);
    let visible_start = composer_visible_start(layout.cursor, visible_height);
    Some(ComposerViewport {
        body,
        layout,
        visible_start,
        visible_height,
    })
}

fn composer_visible_start(cursor: ComposerVisualCursor, visible_height: usize) -> usize {
    if cursor.line_count > visible_height {
        cursor
            .line
            .saturating_add(1)
            .saturating_sub(visible_height)
            .min(cursor.line_count.saturating_sub(visible_height))
    } else {
        0
    }
}

fn composer_text_hit(
    viewport: &ComposerViewport,
    text: &str,
    position: Position,
) -> Option<ComposerTextHit> {
    let visible_count = viewport
        .layout
        .cursor
        .line_count
        .saturating_sub(viewport.visible_start)
        .min(viewport.visible_height);
    let row_offset = usize::from(position.y.saturating_sub(viewport.body.y))
        .min(visible_count.saturating_sub(1));
    let visual_row = viewport.visible_start.saturating_add(row_offset);
    let row_range = composer_visual_row_range(&viewport.layout, visual_row)?;
    let column = usize::from(position.x.saturating_sub(viewport.body.x));
    if column < viewport.layout.indent_width {
        return Some(composer_caret_hit(row_range.start));
    }

    let content_column = column.saturating_sub(viewport.layout.indent_width);
    let mut grapheme_column = 0usize;
    for (offset, grapheme) in text[row_range.clone()].grapheme_indices(true) {
        let width = UnicodeWidthStr::width(grapheme);
        if width == 0 {
            continue;
        }
        let grapheme_end_column = grapheme_column.saturating_add(width);
        if content_column < grapheme_end_column {
            let start = row_range.start.saturating_add(offset);
            let end = start.saturating_add(grapheme.len());
            let caret = if content_column
                .saturating_sub(grapheme_column)
                .saturating_mul(2)
                >= width
            {
                end
            } else {
                start
            };
            return Some(ComposerTextHit {
                grapheme_range: start..end,
                caret_range: caret..caret,
            });
        }
        grapheme_column = grapheme_end_column;
    }
    Some(composer_caret_hit(row_range.end))
}

fn composer_caret_hit(offset: usize) -> ComposerTextHit {
    ComposerTextHit {
        grapheme_range: offset..offset,
        caret_range: offset..offset,
    }
}

fn composer_visual_row_range(layout: &ComposerLayout, row: usize) -> Option<Range<usize>> {
    let mut row_start = 0usize;
    for line in &layout.lines {
        let row_count = line.rows.len().max(1);
        let row_end = row_start.saturating_add(row_count);
        if row < row_end {
            let range = line
                .rows
                .get(row.saturating_sub(row_start))
                .cloned()
                .unwrap_or(0..0);
            return Some(
                line.source_start.saturating_add(range.start)
                    ..line.source_start.saturating_add(range.end),
            );
        }
        row_start = row_end;
    }
    None
}

#[cfg(test)]
#[path = "composer_render_tests.rs"]
mod tests;
