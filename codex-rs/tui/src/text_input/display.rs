use super::EditableText;
use std::borrow::Cow;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const TAB_STOP_WIDTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TabExpansion {
    source: Range<usize>,
    display: Range<usize>,
}

pub(crate) struct EditableTextDisplay<'a> {
    pub(super) text: Cow<'a, str>,
    pub(super) cursor: usize,
    selection: Option<Range<usize>>,
    source_len: usize,
    tab_expansions: Vec<TabExpansion>,
}

impl EditableTextDisplay<'_> {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn selection_range(&self) -> Option<Range<usize>> {
        self.selection.clone()
    }

    pub(crate) fn source_range_for_display_range(
        &self,
        display_range: Range<usize>,
    ) -> Range<usize> {
        let start = self.normalized_display_boundary_forward(display_range.start);
        let end = self.normalized_display_boundary_forward(display_range.end.max(start));
        if start == end {
            let offset = self.source_offset_for_display_offset(start, DisplayEdge::Start);
            return offset..offset;
        }
        self.source_offset_for_display_offset(start, DisplayEdge::Start)
            ..self.source_offset_for_display_offset(end, DisplayEdge::End)
    }

    fn display_range_for_source_range(&self, source_range: Range<usize>) -> Range<usize> {
        self.display_offset_for_source_offset(source_range.start)
            ..self.display_offset_for_source_offset(source_range.end)
    }

    fn display_offset_for_source_offset(&self, source_offset: usize) -> usize {
        let source_offset = source_offset.min(self.source_len);
        let expansion_bytes = self
            .tab_expansions
            .iter()
            .take_while(|expansion| expansion.source.end <= source_offset)
            .map(|expansion| {
                expansion
                    .display
                    .len()
                    .saturating_sub(expansion.source.len())
            })
            .sum::<usize>();
        source_offset.saturating_add(expansion_bytes)
    }

    fn source_offset_for_display_offset(&self, display_offset: usize, edge: DisplayEdge) -> usize {
        let display_offset = display_offset.min(self.text.len());
        let mut expansion_bytes = 0usize;
        for expansion in &self.tab_expansions {
            if display_offset < expansion.display.start {
                break;
            }
            if display_offset == expansion.display.start {
                return expansion.source.start;
            }
            if display_offset < expansion.display.end {
                return match edge {
                    DisplayEdge::Start => expansion.source.start,
                    DisplayEdge::End => expansion.source.end,
                };
            }
            if display_offset == expansion.display.end {
                return expansion.source.end;
            }
            expansion_bytes = expansion_bytes.saturating_add(
                expansion
                    .display
                    .len()
                    .saturating_sub(expansion.source.len()),
            );
        }
        display_offset
            .saturating_sub(expansion_bytes)
            .min(self.source_len)
    }

    fn normalized_display_boundary_forward(&self, display_offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .find(|index| *index >= display_offset)
            .unwrap_or(self.text.len())
    }
}

#[derive(Debug, Clone, Copy)]
enum DisplayEdge {
    Start,
    End,
}

impl EditableText {
    pub(crate) fn display(&self) -> EditableTextDisplay<'_> {
        if !self.text.contains('\t') {
            return EditableTextDisplay {
                text: Cow::Borrowed(&self.text),
                cursor: self.cursor,
                selection: self.selection_range(),
                source_len: self.text.len(),
                tab_expansions: Vec::new(),
            };
        }

        let mut text = String::with_capacity(self.text.len());
        let mut column = 0usize;
        let mut tab_expansions = Vec::new();
        for (index, grapheme) in self.text.grapheme_indices(true) {
            match grapheme {
                "\n" => {
                    text.push('\n');
                    column = 0;
                }
                "\t" => {
                    let spaces = TAB_STOP_WIDTH - column % TAB_STOP_WIDTH;
                    let display_start = text.len();
                    text.extend(std::iter::repeat_n(' ', spaces));
                    tab_expansions.push(TabExpansion {
                        source: index..index + grapheme.len(),
                        display: display_start..text.len(),
                    });
                    column += spaces;
                }
                _ => {
                    text.push_str(grapheme);
                    column += UnicodeWidthStr::width(grapheme);
                }
            }
        }
        let mut display = EditableTextDisplay {
            cursor: 0,
            selection: None,
            source_len: self.text.len(),
            tab_expansions,
            text: Cow::Owned(text),
        };
        display.cursor = display.display_offset_for_source_offset(self.cursor);
        display.selection = self
            .selection_range()
            .map(|range| display.display_range_for_source_range(range));
        display
    }
}
