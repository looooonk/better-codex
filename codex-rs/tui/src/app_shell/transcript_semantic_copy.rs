use super::*;
use crate::markdown_copy::SelectedLine;
use crate::terminal_hyperlinks::LogicalLineSource;
use unicode_segmentation::UnicodeSegmentation;

pub(in crate::app_shell) fn selected_markdown(
    shell: &ShellState,
    area: Rect,
    selection: NormalizedVisualRange,
) -> Option<String> {
    let plain = transcript_selected_text(shell, area, selection)?;
    let viewport = transcript_viewport(shell, area);
    let mut selected = Vec::new();
    for row in selection.start().row()..=selection.end().row() {
        let separator = if row == selection.start().row() {
            ""
        } else {
            "\n"
        };
        let Some(line) = viewport.layout.row_at(row)?.line() else {
            SelectedLine::append(
                &mut selected,
                LogicalLineSource::new(String::new()),
                0..0,
                separator,
            );
            continue;
        };
        let text = rendered_line_text(line);
        let text = trim_synthetic_right_padding(&text);
        let columns = selectable_columns_on_row(
            selection,
            row,
            crate::width::display_width(text),
            line.synthetic_prefix_width,
        )?;
        let mut column = 0;
        let mut bytes = text.len()..text.len();
        for (offset, grapheme) in text.grapheme_indices(true) {
            let next_column = column + crate::width::display_width(grapheme);
            if column < columns.end && next_column > columns.start {
                bytes.start = bytes.start.min(offset);
                bytes.end = offset + grapheme.len();
            }
            column = next_column;
        }
        if let Some(source) = &line.source {
            let mut source = source.clone();
            source.copy_as_prose |= source.copy.is_none();
            let displayed_end = source.prefix_bytes + source.range.len();
            let start = bytes.start.clamp(source.prefix_bytes, displayed_end);
            let end = bytes.end.clamp(source.prefix_bytes, displayed_end);
            let range = source.range.start + start - source.prefix_bytes
                ..source.range.start + end - source.prefix_bytes;
            SelectedLine::append(&mut selected, source, range, separator);
        } else {
            let mut source = LogicalLineSource::new(text[bytes].to_string());
            source.copy_as_prose = true;
            let range = source.range.clone();
            SelectedLine::append(&mut selected, source, range, separator);
        }
    }
    Some(crate::markdown_copy::selection(&selected, &plain).0)
}

#[cfg(test)]
#[path = "transcript_semantic_copy_tests.rs"]
mod tests;
