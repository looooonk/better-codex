use super::composer_toolbar::toolbar_rows;
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

/// Geometry shared by sizing, painting, cursor movement and hit testing.
#[derive(Default)]
pub(super) struct ComposerLayout {
    text: String,
    pub(super) width: usize,
    pub(super) rows: Vec<Range<usize>>,
    pub(super) indent_width: usize,
}

impl ComposerLayout {
    pub(super) fn new(text: &str, width: usize) -> Self {
        let width = width.max(1);
        let content_width = width.saturating_sub(COMPOSER_INDENT_WIDTH).max(1);
        let mut rows = Vec::new();
        let mut start = 0;
        for line in text.split('\n') {
            wrap_logical_line(line, content_width, start, &mut rows);
            start += line.len() + 1;
        }
        Self {
            text: text.to_owned(),
            width,
            rows,
            indent_width: COMPOSER_INDENT_WIDTH.min(width.saturating_sub(1)),
        }
    }

    pub(super) fn for_draft(&mut self, text: &str, width: usize) -> &Self {
        if self.width != width.max(1) || self.text != text {
            *self = Self::new(text, width);
        }
        self
    }

    pub(super) fn cursor(&self, offset: usize) -> ComposerVisualCursor {
        let offset = offset.min(self.text.len());
        let row = self
            .rows
            .partition_point(|range| range.start <= offset)
            .saturating_sub(1);
        let columns = self.text[self.rows[row].start..offset].width();
        let content_width = self.width.saturating_sub(self.indent_width).max(1);
        let line = row + columns / content_width;
        ComposerVisualCursor {
            line,
            column: self.indent_width + columns % content_width,
            line_count: self.rows.len().max(line + 1),
        }
    }
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
    pub(super) cursor: ComposerVisualCursor,
    pub(super) visible_start: usize,
    pub(super) visible_height: usize,
}

pub(super) fn composer_body(input: Rect) -> Rect {
    let content = pane_content_rect(input);
    Rect {
        height: content.height.saturating_sub(toolbar_rows(content)),
        ..content
    }
}

pub(super) fn composer_viewport(
    body: Rect,
    cursor: ComposerVisualCursor,
) -> Option<ComposerViewport> {
    if body.width == 0 || body.height == 0 {
        return None;
    }
    let visible_height = usize::from(body.height);
    let visible_start = composer_visible_start(cursor, visible_height);
    Some(ComposerViewport {
        body,
        cursor,
        visible_start,
        visible_height,
    })
}

impl ComposerLayout {
    pub(super) fn vertical_target(
        &self,
        offset: usize,
        direction: ComposerVerticalDirection,
    ) -> ComposerVerticalTarget {
        let cursor = self.cursor(offset);
        let row = match direction {
            ComposerVerticalDirection::Up => cursor.line.checked_sub(1),
            ComposerVerticalDirection::Down => cursor.line.checked_add(1),
        };
        let Some(range) = row.and_then(|i| self.rows.get(i)) else {
            return ComposerVerticalTarget::Boundary;
        };
        let column = cursor.column.saturating_sub(self.indent_width);
        ComposerVerticalTarget::Cursor(byte_for_display_column(&self.text, range.clone(), column))
    }
}

fn wrap_logical_line(
    logical_line: &str,
    content_width: usize,
    start: usize,
    rows: &mut Vec<Range<usize>>,
) {
    if logical_line.is_empty() {
        rows.push(start..start);
        return;
    }
    let mut fragments = Vec::new();
    let words = split_words(
        textwrap::WordSeparator::new().find_words(logical_line),
        &textwrap::WordSplitter::HyphenSplitter,
    );
    for word in words {
        if !word.word.is_empty() {
            let word_range = source_range(logical_line, word.word);
            let word_width = word.word.width();
            if word_width <= content_width {
                fragments.push(ComposerFragment {
                    range: word_range,
                    width: word_width,
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
    rows.extend(
        textwrap::wrap_algorithms::wrap_first_fit(&fragments, &[content_width as f64])
            .into_iter()
            .filter_map(|line| {
                Some(start + line.first()?.range.start..start + line.last()?.range.end)
            }),
    );
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
