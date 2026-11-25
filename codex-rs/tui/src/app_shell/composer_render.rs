#[path = "composer_markdown.rs"]
mod markdown;

use super::LocalSlashCommand;
use super::composer_layout::ComposerLayout;
use super::composer_layout::ComposerViewport;
use super::composer_layout::composer_body;
use super::composer_layout::composer_viewport;
use super::design::palette;
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
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[cfg(test)]
pub(super) fn wrapped_composer_lines(
    text: &str,
    is_empty: bool,
    cursor: usize,
    width: usize,
) -> Vec<Line<'static>> {
    wrapped_composer_lines_with_selection(text, is_empty, cursor, width, /*selection*/ None)
}

#[cfg(test)]
pub(super) fn wrapped_composer_lines_with_selection(
    text: &str,
    is_empty: bool,
    cursor: usize,
    width: usize,
    selection: Option<Range<usize>>,
) -> Vec<Line<'static>> {
    let layout = ComposerLayout::new(text, width);
    let lines = layout.cursor(cursor).line_count;
    composer_lines(text, is_empty, &layout, selection, 0..lines)
}

pub(super) fn composer_lines(
    text: &str,
    is_empty: bool,
    layout: &ComposerLayout,
    selection: Option<Range<usize>>,
    visible: Range<usize>,
) -> Vec<Line<'static>> {
    let initial_indent = Line::from("> ".fg(palette::focus()));
    let subsequent_indent = Line::from("  ".fg(palette::muted()));
    if is_empty {
        let placeholder = "Type a message".fg(palette::muted()).into();
        let options = RtOptions::new(layout.width)
            .initial_indent(initial_indent)
            .subsequent_indent(subsequent_indent);
        let mut lines = Vec::new();
        push_owned_lines(&word_wrap_line(&placeholder, options), &mut lines);
        return lines;
    }

    let command = composer_command_range(text);
    let mut markdown = markdown::SourceStyles::new(text);
    visible
        .map(|i| {
            let Some(range) = layout.rows.get(i) else {
                // The caret wraps to a fresh row at an exact-width end of draft.
                return Line::default();
            };
            let newline_selected = text.as_bytes().get(range.end) == Some(&b'\n')
                && selection
                    .as_ref()
                    .is_some_and(|selected| selected.contains(&range.end));
            if i != 0 && range.is_empty() && !newline_selected {
                return Line::default();
            }
            let indent = if i == 0 {
                initial_indent.clone()
            } else {
                subsequent_indent.clone()
            };
            let mut row = composer_row(
                text,
                range.clone(),
                command.as_ref(),
                selection.as_ref(),
                &mut markdown,
                indent,
            );
            if newline_selected {
                row.spans.push(Span::styled(" ", text_selection_style()));
            }
            row
        })
        .collect()
}

#[cfg(test)]
pub(super) fn composer_visual_cursor_line(
    text: &str,
    cursor: usize,
    width: usize,
) -> Option<usize> {
    Some(ComposerLayout::new(text, width).cursor(cursor).line)
}

#[cfg(test)]
pub(super) fn composer_cursor_position(input: Rect, text: &str, cursor: usize) -> Option<Position> {
    let body = composer_body(input);
    let layout = ComposerLayout::new(text, usize::from(body.width));
    viewport_cursor_position(&composer_viewport(body, layout.cursor(cursor))?)
}

pub(super) fn viewport_cursor_position(viewport: &ComposerViewport) -> Option<Position> {
    let y = viewport.cursor.line.checked_sub(viewport.visible_start)?;
    if y >= viewport.visible_height {
        return None;
    }

    Some(Position {
        x: viewport
            .body
            .x
            .saturating_add(u16::try_from(viewport.cursor.column).unwrap_or(u16::MAX)),
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
    let body = composer_body(input_area);
    let layout = ComposerLayout::new(text, usize::from(body.width));
    let viewport = composer_viewport(body, layout.cursor(cursor))?;
    if !viewport.body.contains(position) {
        return None;
    }
    composer_text_hit(&viewport, &layout, text, position)
}

/// Resolve a position to the closest hit in the cursor-following visible composer viewport.
pub(super) fn composer_text_hit_clamped_to_visible_viewport(
    input_area: Rect,
    text: &str,
    cursor: usize,
    position: Position,
) -> Option<ComposerTextHit> {
    let body = composer_body(input_area);
    let layout = ComposerLayout::new(text, usize::from(body.width));
    let viewport = composer_viewport(body, layout.cursor(cursor))?;
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
    composer_text_hit(&viewport, &layout, text, position)
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
    text: &str,
    range: Range<usize>,
    command_range: Option<&Range<usize>>,
    selection: Option<&Range<usize>>,
    markdown: &mut markdown::SourceStyles,
    mut row: Line<'static>,
) -> Line<'static> {
    let mut spans = row.spans;
    let mut current_text = String::new();
    let mut current_style = None;
    for (offset, grapheme) in text[range.clone()].grapheme_indices(true) {
        let start = range.start + offset;
        let display_range = start..start + grapheme.len();
        let style = composer_grapheme_style(
            markdown.at(display_range.start),
            &display_range,
            command_range,
            selection,
        );
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
    mut style: Style,
    display_range: &Range<usize>,
    command_range: Option<&Range<usize>>,
    selection: Option<&Range<usize>>,
) -> Style {
    if command_range.is_some_and(|command| ranges_overlap(display_range, command)) {
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

fn composer_text_hit(
    viewport: &ComposerViewport,
    layout: &ComposerLayout,
    text: &str,
    position: Position,
) -> Option<ComposerTextHit> {
    let visible_count = viewport
        .cursor
        .line_count
        .saturating_sub(viewport.visible_start)
        .min(viewport.visible_height);
    let row_offset = usize::from(position.y.saturating_sub(viewport.body.y))
        .min(visible_count.saturating_sub(1));
    let visual_row = viewport.visible_start.saturating_add(row_offset);
    let row_range = layout.rows.get(visual_row)?.clone();
    let column = usize::from(position.x.saturating_sub(viewport.body.x));
    if column < layout.indent_width {
        return Some(composer_caret_hit(row_range.start));
    }

    let content_column = column.saturating_sub(layout.indent_width);
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

#[cfg(test)]
#[path = "composer_render_tests.rs"]
mod tests;
