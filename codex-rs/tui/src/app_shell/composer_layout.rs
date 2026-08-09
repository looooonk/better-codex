use super::design::body_rect_after_title;
use super::design::pane_content_rect;
use ratatui::layout::Rect;
use std::ops::Range;
use textwrap::core::Fragment;
use textwrap::word_splitters::split_words;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const COMPOSER_INDENT_WIDTH: usize = 2;

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

pub(super) struct ComposerLogicalLineLayout {
    pub(super) source_start: usize,
    pub(super) rows: Vec<Range<usize>>,
}

pub(super) struct ComposerLayout {
    pub(super) lines: Vec<ComposerLogicalLineLayout>,
    pub(super) cursor: ComposerVisualCursor,
    pub(super) indent_width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ComposerVisualCursor {
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) line_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposerVerticalDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposerVerticalTarget {
    Cursor(usize),
    Boundary,
}

pub(super) struct ComposerViewport {
    pub(super) body: Rect,
    pub(super) layout: ComposerLayout,
    pub(super) visible_start: usize,
    pub(super) visible_height: usize,
}

pub(super) fn composer_layout(text: &str, cursor: usize, width: usize) -> Option<ComposerLayout> {
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

pub(super) fn composer_visual_cursor(
    text: &str,
    cursor: usize,
    width: usize,
) -> Option<ComposerVisualCursor> {
    composer_layout(text, cursor, width).map(|layout| layout.cursor)
}

pub(super) fn composer_viewport(
    input_area: Rect,
    text: &str,
    cursor: usize,
) -> Option<ComposerViewport> {
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

pub(super) fn composer_vertical_target(
    text: &str,
    cursor: usize,
    width: usize,
    direction: ComposerVerticalDirection,
) -> ComposerVerticalTarget {
    let Some(layout) = composer_layout(text, cursor, width) else {
        return ComposerVerticalTarget::Boundary;
    };
    let target_row = match direction {
        ComposerVerticalDirection::Up => layout.cursor.line.checked_sub(1),
        ComposerVerticalDirection::Down => layout
            .cursor
            .line
            .checked_add(1)
            .filter(|row| *row < layout.cursor.line_count),
    };
    let Some(row_range) = target_row.and_then(|row| composer_visual_row_range(&layout, row)) else {
        return ComposerVerticalTarget::Boundary;
    };
    let target_column = layout.cursor.column.saturating_sub(layout.indent_width);
    ComposerVerticalTarget::Cursor(byte_for_display_column(text, row_range, target_column))
}

pub(super) fn composer_visual_row_range(
    layout: &ComposerLayout,
    row: usize,
) -> Option<Range<usize>> {
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

fn byte_for_display_column(text: &str, range: Range<usize>, target_column: usize) -> usize {
    let mut boundary = range.start;
    let mut width = 0usize;
    for (offset, grapheme) in text[range.clone()].grapheme_indices(true) {
        let next_width = width.saturating_add(UnicodeWidthStr::width(grapheme));
        if next_width > target_column {
            break;
        }
        boundary = range
            .start
            .saturating_add(offset)
            .saturating_add(grapheme.len());
        width = next_width;
    }
    boundary
}

#[cfg(test)]
#[path = "composer_layout_tests.rs"]
mod tests;
