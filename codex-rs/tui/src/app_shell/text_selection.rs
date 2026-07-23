//! Grapheme-aware primitives for mouse-selected terminal text.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A display-cell location in a sequence of rendered terminal rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct VisualPosition {
    row: usize,
    column: usize,
}

impl VisualPosition {
    pub(super) fn new(row: usize, column: usize) -> Self {
        Self { row, column }
    }

    pub(super) fn row(self) -> usize {
        self.row
    }

    pub(super) fn column(self) -> usize {
        self.column
    }
}

/// The complete display-cell extent of the grapheme under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VisualGraphemeHit {
    position: VisualPosition,
    width: usize,
}

impl VisualGraphemeHit {
    pub(super) fn new(row: usize, column: usize, width: usize) -> Self {
        Self {
            position: VisualPosition::new(row, column),
            width: width.max(1),
        }
    }

    pub(super) fn position(self) -> VisualPosition {
        self.position
    }

    fn end_column(self) -> usize {
        self.position.column.saturating_add(self.width)
    }
}

/// An ordered, cell-inclusive selection represented with an exclusive final column.
///
/// Constructing the range from grapheme hits makes forward and reverse drags select the same
/// cells. It also expands a hit on either cell of a wide grapheme to the full grapheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NormalizedVisualRange {
    start: VisualPosition,
    end: VisualPosition,
}

impl NormalizedVisualRange {
    pub(super) fn from_hits(anchor: VisualGraphemeHit, focus: VisualGraphemeHit) -> Self {
        let (first, last) = if anchor.position <= focus.position {
            (anchor, focus)
        } else {
            (focus, anchor)
        };
        Self {
            start: first.position,
            end: VisualPosition::new(last.position.row, last.end_column()),
        }
    }

    pub(super) fn start(self) -> VisualPosition {
        self.start
    }

    pub(super) fn end(self) -> VisualPosition {
        self.end
    }

    /// Return the selected display columns on `row`, clamped to the rendered line width.
    pub(super) fn columns_on_row(self, row: usize, line_width: usize) -> Option<Range<usize>> {
        if row < self.start.row || row > self.end.row {
            return None;
        }

        let start = if row == self.start.row {
            self.start.column.min(line_width)
        } else {
            0
        };
        let end = if row == self.end.row {
            self.end.column.min(line_width)
        } else {
            line_width
        };
        Some(start.min(end)..end)
    }
}

/// Resolve a display column to the full grapheme occupying that terminal cell.
pub(super) fn grapheme_hit_at(text: &str, row: usize, column: usize) -> Option<VisualGraphemeHit> {
    let mut grapheme_column = 0usize;
    for grapheme in text.graphemes(true) {
        let width = grapheme.width();
        if width == 0 {
            continue;
        }
        let grapheme_end = grapheme_column.saturating_add(width);
        if (grapheme_column..grapheme_end).contains(&column) {
            return Some(VisualGraphemeHit::new(row, grapheme_column, width));
        }
        grapheme_column = grapheme_end;
    }
    None
}

/// Copy every complete grapheme intersecting a display-column range.
pub(super) fn graphemes_in_columns(text: &str, columns: Range<usize>) -> String {
    if columns.is_empty() {
        return String::new();
    }

    let mut selected = String::new();
    let mut grapheme_column = 0usize;
    for grapheme in text.graphemes(true) {
        let width = grapheme.width();
        if width == 0 {
            continue;
        }
        let grapheme_end = grapheme_column.saturating_add(width);
        if grapheme_column < columns.end && grapheme_end > columns.start {
            selected.push_str(grapheme);
        }
        grapheme_column = grapheme_end;
        if grapheme_column >= columns.end {
            break;
        }
    }
    selected
}

/// Remove right-edge spaces introduced solely to paint full-width terminal cards.
pub(super) fn trim_synthetic_right_padding(text: &str) -> &str {
    text.trim_end_matches(' ')
}
