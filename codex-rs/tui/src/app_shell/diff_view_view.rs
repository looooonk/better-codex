use super::design::fill_rect;
use super::design::palette;
use super::design::pane_style;
use super::diff_view::DiffCell;
use super::diff_view::DiffFile;
use super::diff_view::DiffFileKind;
use super::diff_view::DiffLineKind;
use super::diff_view::DiffStatus;
use super::diff_view::DiffViewState;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

const MODAL_MARGIN: u16 = 2;
const MAX_MODAL_WIDTH: u16 = 160;
const MAX_MODAL_HEIGHT: u16 = 40;
const MIN_MODAL_HEIGHT: u16 = 10;
const MIN_FILES_WIDTH: u16 = 13;
const MIN_DIFF_WIDTH: u16 = 8;
const MAX_FILES_WIDTH: u16 = 32;
const MIN_COLUMNS_WIDTH: u16 = MIN_FILES_WIDTH + MIN_DIFF_WIDTH * 2;

#[derive(Debug, Clone, Copy)]
struct DiffViewGeometry {
    modal: Rect,
    header: Rect,
    labels: Rect,
    body: Rect,
    footer: Rect,
    files: Rect,
    old: Rect,
    new: Rect,
    separators: [Option<u16>; 2],
}

pub(super) fn render_diff_view(state: &DiffViewState, screen: Rect, buf: &mut Buffer) {
    let geometry = diff_view_geometry(screen);
    buf.set_style(screen, Style::new().add_modifier(Modifier::DIM));
    Clear.render(geometry.modal, buf);
    fill_rect(buf, geometry.modal, palette::SURFACE);

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette::FOCUS))
        .style(pane_style(palette::SURFACE))
        .title(Line::from(format!(" {} ", state.title())).bold())
        .render(geometry.modal, buf);

    render_headers(geometry, buf);
    render_files(state, geometry.files, buf);
    render_diff_rows(state, geometry, buf);
    render_separators(geometry, buf);
    render_footer(
        state,
        geometry.footer,
        usize::from(geometry.body.height),
        buf,
    );
}

pub(super) fn diff_view_panel_area(screen: Rect) -> Rect {
    diff_view_geometry(screen).modal
}

pub(super) fn diff_view_file_selector_area(screen: Rect) -> Rect {
    let geometry = diff_view_geometry(screen);
    Rect::new(
        geometry.files.x,
        geometry.header.y,
        geometry.files.width,
        geometry.body.bottom().saturating_sub(geometry.header.y),
    )
}

pub(super) fn diff_view_file_at(
    state: &DiffViewState,
    screen: Rect,
    position: Position,
) -> Option<usize> {
    let geometry = diff_view_geometry(screen);
    if !geometry.files.contains(position) {
        return None;
    }
    let visible = usize::from(geometry.files.height);
    let start = visible_file_start(state, visible);
    let index = start.saturating_add(usize::from(position.y.saturating_sub(geometry.files.y)));
    (index < state.files().len()).then_some(index)
}

fn render_headers(geometry: DiffViewGeometry, buf: &mut Buffer) {
    for (area, title) in [
        (
            column_slice(geometry.header, geometry.files),
            "CHANGED FILES",
        ),
        (column_slice(geometry.header, geometry.old), "OLD FILE"),
        (column_slice(geometry.header, geometry.new), "NEW FILE"),
    ] {
        Paragraph::new(truncate_line_with_ellipsis_if_overflow(
            Line::from(title).fg(palette::MUTED).bold(),
            usize::from(area.width),
        ))
        .style(pane_style(palette::SURFACE))
        .render(area, buf);
    }
}

fn render_files(state: &DiffViewState, area: Rect, buf: &mut Buffer) {
    let visible = usize::from(area.height);
    let start = visible_file_start(state, visible);
    for (offset, (index, file)) in state
        .files()
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .enumerate()
    {
        let row = Rect::new(
            area.x,
            area.y
                .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX)),
            area.width,
            1,
        );
        let selected = index == state.selected_file_index();
        let background = if selected {
            palette::ELEVATED
        } else {
            palette::SURFACE
        };
        fill_rect(buf, row, background);
        let (glyph, color) = file_glyph(file);
        let marker = if selected { ">" } else { " " };
        let path_width = usize::from(row.width).saturating_sub(4);
        let path = file.display_path();
        let path = if UnicodeWidthStr::width(path.as_str()) <= path_width {
            path
        } else {
            match (file.old_label(), file.new_label()) {
                (Some(old), Some(new)) if old != new => {
                    format!("{} -> {}", file_name(old), file_name(new))
                }
                (Some(path), _) | (_, Some(path)) => file_name(path).to_string(),
                (None, None) => String::new(),
            }
        };
        let line = Line::from(vec![
            marker
                .fg(if selected {
                    palette::FOCUS
                } else {
                    palette::MUTED
                })
                .bold(),
            " ".into(),
            glyph.fg(color).bold(),
            " ".into(),
            path.into(),
        ]);
        Paragraph::new(truncate_line_with_ellipsis_if_overflow(
            line,
            usize::from(row.width),
        ))
        .style(pane_style(background))
        .render(row, buf);
    }
}

fn render_diff_rows(state: &DiffViewState, geometry: DiffViewGeometry, buf: &mut Buffer) {
    let Some(file) = state.selected_file() else {
        state.set_scroll_max(0);
        return;
    };
    render_file_label(
        file.old_label(),
        column_slice(geometry.labels, geometry.old),
        buf,
    );
    render_file_label(
        file.new_label(),
        column_slice(geometry.labels, geometry.new),
        buf,
    );

    let visible = usize::from(geometry.body.height);
    state.set_scroll_max(file.rows().len().saturating_sub(visible));
    let scroll = state.scroll();
    for (offset, row) in file.rows().iter().skip(scroll).take(visible).enumerate() {
        let y = geometry
            .body
            .y
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        render_cell(
            row.old.as_ref(),
            Rect::new(geometry.old.x, y, geometry.old.width, 1),
            buf,
        );
        render_cell(
            row.new.as_ref(),
            Rect::new(geometry.new.x, y, geometry.new.width, 1),
            buf,
        );
    }
}

fn render_file_label(label: Option<&str>, area: Rect, buf: &mut Buffer) {
    let line = label.map_or_else(Line::default, |label| {
        truncate_line_with_ellipsis_if_overflow(
            Line::from(label.to_string()).fg(palette::CYAN),
            usize::from(area.width),
        )
    });
    Paragraph::new(line)
        .style(pane_style(palette::SURFACE))
        .render(area, buf);
}

fn render_cell(cell: Option<&DiffCell>, area: Rect, buf: &mut Buffer) {
    let Some(cell) = cell else {
        return;
    };
    let number_width = usize::from(area.width).saturating_sub(4).min(5);
    let number = cell
        .line_number
        .map(|number| format!("{number:>number_width$}"))
        .unwrap_or_else(|| " ".repeat(number_width));
    let (marker, color, background, bold) = match cell.kind {
        DiffLineKind::Context => (" ", palette::TEXT, palette::SURFACE, false),
        DiffLineKind::Added => ("+", palette::SUCCESS, palette::DIFF_ADDED_BACKGROUND, false),
        DiffLineKind::Removed => ("-", palette::ERROR, palette::DIFF_REMOVED_BACKGROUND, false),
        DiffLineKind::Hunk => (" ", palette::CYAN, palette::SURFACE, true),
    };
    let text = if bold {
        Span::from(cell.text.clone()).fg(color).bold()
    } else {
        Span::from(cell.text.clone()).fg(color)
    };
    let line = Line::from(vec![
        number.fg(palette::MUTED),
        " ".into(),
        marker.fg(color).bold(),
        " ".into(),
        text,
    ]);
    Paragraph::new(truncate_line_with_ellipsis_if_overflow(
        line,
        usize::from(area.width),
    ))
    .style(pane_style(background))
    .render(area, buf);
}

fn render_separators(geometry: DiffViewGeometry, buf: &mut Buffer) {
    for x in geometry.separators.into_iter().flatten() {
        for y in geometry.header.y..geometry.footer.y {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_symbol("│")
                    .set_style(Style::new().fg(palette::BORDER).bg(palette::SURFACE));
            }
        }
    }
}

fn render_footer(state: &DiffViewState, area: Rect, visible: usize, buf: &mut Buffer) {
    let file = state.selected_file();
    let total = file.map_or(0, |file| file.rows().len());
    let first = usize::from(total > 0)
        .saturating_add(state.scroll())
        .min(total);
    let last = state.scroll().saturating_add(visible).min(total).max(first);
    let file_position = state
        .selected_file()
        .map_or(0, |_| state.selected_file_index().saturating_add(1));
    let range = format!(
        "file {}/{}  {first}-{last}/{total}",
        file_position.min(state.files().len()),
        state.files().len()
    );
    let width = usize::from(area.width);
    let hint = [
        "←/→ files  j/k scroll  PgUp/PgDn page  g/G ends  Esc close",
        "←/→ files  j/k scroll  PgUp/PgDn  Esc",
        "←/→ files  ↑/↓ scroll  Esc",
        "←/→  ↑/↓  Esc",
        "",
    ]
    .into_iter()
    .find(|hint| {
        let spacing = usize::from(!hint.is_empty()) * 3;
        UnicodeWidthStr::width(*hint) + spacing + UnicodeWidthStr::width(range.as_str()) <= width
    })
    .unwrap_or_default();
    Paragraph::new(Line::from(vec![
        if hint.is_empty() {
            "".into()
        } else {
            format!(" {hint}  ").fg(palette::MUTED)
        },
        range.fg(palette::PURPLE).bold(),
    ]))
    .style(pane_style(palette::SURFACE))
    .render(area, buf);
}

fn file_glyph(file: &DiffFile) -> (&'static str, ratatui::style::Color) {
    if matches!(file.status(), DiffStatus::Failed | DiffStatus::Declined) {
        return ("!", palette::ERROR);
    }
    if file.status() == DiffStatus::InProgress {
        return ("~", palette::CYAN);
    }
    match file.kind() {
        DiffFileKind::Added => ("A", palette::SUCCESS),
        DiffFileKind::Deleted => ("D", palette::ERROR),
        DiffFileKind::Modified => ("M", palette::WARNING),
        DiffFileKind::Renamed => ("R", palette::CYAN),
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn visible_file_start(state: &DiffViewState, visible: usize) -> usize {
    let selected = state
        .selected_file_index()
        .min(state.files().len().saturating_sub(1));
    selected
        .saturating_add(1)
        .saturating_sub(visible)
        .min(state.files().len().saturating_sub(visible))
}

fn column_slice(row: Rect, column: Rect) -> Rect {
    Rect::new(column.x, row.y, column.width, row.height)
}

fn diff_view_geometry(screen: Rect) -> DiffViewGeometry {
    let available_width = screen.width.saturating_sub(MODAL_MARGIN.saturating_mul(2));
    let available_height = screen.height.saturating_sub(MODAL_MARGIN.saturating_mul(2));
    let width = available_width.min(MAX_MODAL_WIDTH);
    let height = available_height
        .min(MAX_MODAL_HEIGHT)
        .max(available_height.min(MIN_MODAL_HEIGHT));
    let modal = Rect::new(
        screen
            .x
            .saturating_add(screen.width.saturating_sub(width) / 2),
        screen
            .y
            .saturating_add(screen.height.saturating_sub(height) / 2),
        width,
        height,
    );
    let inner = Rect::new(
        modal.x.saturating_add(u16::from(modal.width > 1)),
        modal.y.saturating_add(u16::from(modal.height > 1)),
        modal.width.saturating_sub(2),
        modal.height.saturating_sub(2),
    );
    let padding = u16::from(inner.width > 2);
    let content = Rect::new(
        inner.x.saturating_add(padding),
        inner.y,
        inner.width.saturating_sub(padding.saturating_mul(2)),
        inner.height,
    );
    let header_height = content.height.min(1);
    let labels_height = u16::from(content.height > 2);
    let footer_height = u16::from(content.height > header_height.saturating_add(labels_height));
    let body_height = content
        .height
        .saturating_sub(header_height)
        .saturating_sub(labels_height)
        .saturating_sub(footer_height);
    let header = Rect::new(content.x, content.y, content.width, header_height);
    let labels = Rect::new(content.x, header.bottom(), content.width, labels_height);
    let body = Rect::new(content.x, labels.bottom(), content.width, body_height);
    let footer = Rect::new(content.x, body.bottom(), content.width, footer_height);
    let (files, old, new, separators) = column_geometry(body, content.width);
    DiffViewGeometry {
        modal,
        header,
        labels,
        body,
        footer,
        files,
        old,
        new,
        separators,
    }
}

fn column_geometry(body: Rect, width: u16) -> (Rect, Rect, Rect, [Option<u16>; 2]) {
    let separator_count = width.min(2);
    let usable = width.saturating_sub(separator_count);
    let files_width = if usable >= MIN_COLUMNS_WIDTH {
        (usable / 4)
            .clamp(MIN_FILES_WIDTH, MAX_FILES_WIDTH)
            .min(usable - MIN_DIFF_WIDTH * 2)
    } else {
        usable / 3
    };
    let remaining = usable.saturating_sub(files_width);
    let old_width = remaining / 2;
    let new_width = remaining.saturating_sub(old_width);
    let files = Rect::new(body.x, body.y, files_width, body.height);
    let first_separator = (separator_count > 0).then_some(files.right());
    let old_x = files
        .right()
        .saturating_add(u16::from(first_separator.is_some()));
    let old = Rect::new(old_x, body.y, old_width, body.height);
    let second_separator = (separator_count > 1).then_some(old.right());
    let new_x = old
        .right()
        .saturating_add(u16::from(second_separator.is_some()));
    let new = Rect::new(new_x, body.y, new_width, body.height);
    (files, old, new, [first_separator, second_separator])
}

#[cfg(test)]
#[path = "diff_view_view_tests.rs"]
mod tests;
