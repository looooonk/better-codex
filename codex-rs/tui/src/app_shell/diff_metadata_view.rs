use super::design::palette;
use super::design::pane_style;
use super::diff_view::DiffFile;
use super::diff_view::DiffFileKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

pub(super) fn render_empty_diff(file: &DiffFile, old: Rect, new: Rect, buf: &mut Buffer) -> bool {
    if !file.rows().is_empty() {
        return false;
    }
    let (old_message, new_message) = match file.kind() {
        DiffFileKind::Added => (None, Some("empty or binary file")),
        DiffFileKind::Deleted => (Some("empty or binary file"), None),
        DiffFileKind::Modified => (
            Some("binary or metadata-only change"),
            Some("binary or metadata-only change"),
        ),
        DiffFileKind::Renamed => (Some("renamed from"), Some("renamed to")),
    };
    for (message, area) in [(old_message, old), (new_message, new)] {
        if let Some(message) = message {
            Paragraph::new(Line::from(message).dim().italic())
                .style(pane_style(palette::SURFACE))
                .render(area, buf);
        }
    }
    true
}
