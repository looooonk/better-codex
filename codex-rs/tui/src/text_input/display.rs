use super::EditableText;
use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const TAB_STOP_WIDTH: usize = 8;

pub(crate) struct EditableTextDisplay<'a> {
    pub(super) text: Cow<'a, str>,
    pub(super) cursor: usize,
}

impl EditableTextDisplay<'_> {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }
}

impl EditableText {
    pub(crate) fn display(&self) -> EditableTextDisplay<'_> {
        if !self.text.contains('\t') {
            return EditableTextDisplay {
                text: Cow::Borrowed(&self.text),
                cursor: self.cursor,
            };
        }

        let mut text = String::with_capacity(self.text.len());
        let mut column = 0usize;
        let mut cursor = None;
        for (index, grapheme) in self.text.grapheme_indices(true) {
            if index == self.cursor {
                cursor = Some(text.len());
            }
            match grapheme {
                "\n" => {
                    text.push('\n');
                    column = 0;
                }
                "\t" => {
                    let spaces = TAB_STOP_WIDTH - column % TAB_STOP_WIDTH;
                    text.extend(std::iter::repeat_n(' ', spaces));
                    column += spaces;
                }
                _ => {
                    text.push_str(grapheme);
                    column += UnicodeWidthStr::width(grapheme);
                }
            }
        }
        EditableTextDisplay {
            cursor: cursor.unwrap_or(text.len()),
            text: Cow::Owned(text),
        }
    }
}
