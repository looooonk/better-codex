use super::*;
use crate::app_shell::diff_view::DiffFile;
use crate::app_shell::diff_view::DiffStatus;
use crate::app_shell::diff_view::DiffViewState;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

#[test]
fn renders_wide_and_compact_diff_popups() {
    let wide = fixture();
    insta::assert_snapshot!("wide_diff_popup", render(&wide, 140, 34));

    let mut compact = fixture();
    assert!(compact.select_file(/*selected*/ 1));
    let compact = render(&compact, 54, 16);
    for path in ["diff_vie…", "new_file…", "old_file…", "before.r…"] {
        assert!(compact.contains(path));
    }
    insta::assert_snapshot!("compact_diff_popup", compact);
}

#[test]
fn renders_retained_diff_subset() {
    let state = DiffViewState::new(
        "Session edits",
        /*source_item_id*/ None,
        vec![DiffFile::modified(
            "src/lib.rs",
            "@@ -1 +1 @@\n-before\n+after",
            DiffStatus::Completed,
        )],
    )
    .with_retention(DiffRetention::Truncated);

    insta::assert_snapshot!("retained_diff_subset", render(&state, 80, 16));
}

#[test]
fn horizontally_scrolled_diff_reveals_long_line_suffixes() {
    let prefix = "same-prefix-".repeat(12);
    let mut state = DiffViewState::new(
        "Long line change",
        /*source_item_id*/ None,
        vec![DiffFile::modified(
            "src/long_line.rs",
            format!("@@ -1 +1 @@\n-{prefix}old-tail\n+{prefix}new-tail"),
            DiffStatus::Completed,
        )],
    );
    let initial = render(&state, 100, 16);
    assert!(!initial.contains("old-tail"));
    assert!(!initial.contains("new-tail"));

    for _ in 0..20 {
        state.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
    }
    let panned = render(&state, 100, 16);

    assert!(panned.contains("old-tail"));
    assert!(panned.contains("new-tail"));
    insta::assert_snapshot!("horizontally_scrolled_diff_popup", panned);
}

#[test]
fn file_hit_testing_uses_the_visible_file_window() {
    let mut state = DiffViewState::new(
        "Session edits",
        /*source_item_id*/ None,
        (0..12)
            .map(|index| {
                DiffFile::added(
                    format!("src/file_{index:02}.rs"),
                    format!("contents {index}"),
                    DiffStatus::Completed,
                )
            })
            .collect(),
    );
    assert!(state.select_file(/*selected*/ 11));
    let screen = Rect::new(
        /*x*/ 5, /*y*/ 3, /*width*/ 54, /*height*/ 16,
    );
    let geometry = diff_view_geometry(screen);
    let start = visible_file_start(&state, usize::from(geometry.files.height));
    let first = Position::new(geometry.files.x, geometry.files.y);
    let selected_y = geometry.files.y.saturating_add(
        u16::try_from(state.selected_file_index().saturating_sub(start)).unwrap_or(u16::MAX),
    );

    assert_eq!(diff_view_file_at(&state, screen, first), Some(start));
    assert_eq!(
        diff_view_file_at(&state, screen, Position::new(geometry.files.x, selected_y)),
        Some(11)
    );
    assert_eq!(
        diff_view_file_at(
            &state,
            screen,
            Position::new(geometry.old.x, geometry.body.y)
        ),
        None
    );
}

#[test]
fn file_selector_area_covers_the_full_left_column() {
    for screen in [
        Rect::new(
            /*x*/ 5, /*y*/ 3, /*width*/ 140, /*height*/ 34,
        ),
        Rect::new(
            /*x*/ 7, /*y*/ 4, /*width*/ 54, /*height*/ 16,
        ),
    ] {
        let geometry = diff_view_geometry(screen);
        let selector = diff_view_file_selector_area(screen);

        assert_eq!(
            selector,
            Rect::new(
                geometry.files.x,
                geometry.header.y,
                geometry.files.width,
                geometry.body.bottom().saturating_sub(geometry.header.y),
            )
        );
        for y in [
            geometry.header.y,
            geometry.labels.y,
            geometry.body.bottom() - 1,
        ] {
            assert!(selector.contains(Position::new(geometry.files.x, y)));
            assert!(selector.contains(Position::new(geometry.files.right() - 1, y)));
        }
        for position in [
            Position::new(geometry.files.right(), geometry.body.y),
            Position::new(geometry.old.x, geometry.body.y),
            Position::new(geometry.new.x, geometry.body.y),
            Position::new(geometry.files.x, geometry.footer.y),
            Position::new(geometry.modal.x, geometry.modal.y),
            Position::new(screen.x, screen.y),
        ] {
            assert!(!selector.contains(position));
        }
    }
}

#[test]
fn diff_cells_and_file_statuses_use_semantic_colors() {
    let state = fixture();
    let screen = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 120, /*height*/ 26,
    );
    let mut buf = Buffer::empty(screen);
    render_diff_view(&state, screen, &mut buf);
    let geometry = diff_view_geometry(screen);
    let hunk = position_of(&buf, geometry.old, "@@ -1,8 +1,8 @@").expect("hunk should render");
    let removed = position_of(&buf, geometry.old, "stale value").expect("removal should render");
    let added = position_of(&buf, geometry.new, "fresh value").expect("addition should render");

    assert_eq!(buf[hunk].style().fg, Some(palette::CYAN));
    assert_eq!(buf[removed].style().fg, Some(palette::ERROR));
    assert_eq!(buf[added].style().fg, Some(palette::SUCCESS));
    for (offset, color) in [
        palette::WARNING,
        palette::SUCCESS,
        palette::ERROR,
        palette::CYAN,
    ]
    .into_iter()
    .enumerate()
    {
        let y = geometry
            .files
            .y
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        assert_eq!(
            buf[(geometry.files.x.saturating_add(2), y)].style().fg,
            Some(color)
        );
    }
    assert_eq!(
        buf[(geometry.files.x, geometry.files.y)].style().bg,
        Some(palette::ELEVATED)
    );
}

#[test]
fn diff_lines_use_full_pane_semantic_backgrounds() {
    let state = fixture();
    let screen = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 120, /*height*/ 26,
    );
    let mut buf = Buffer::empty(screen);
    render_diff_view(&state, screen, &mut buf);
    let geometry = diff_view_geometry(screen);
    let hunk = position_of(&buf, geometry.old, "@@ -1,8 +1,8 @@").expect("hunk should render");
    let context = position_of(&buf, geometry.old, "use ratatui::buffer::Buffer;")
        .expect("context should render");
    let removed = position_of(&buf, geometry.old, "stale value").expect("removal should render");
    let added = position_of(&buf, geometry.new, "fresh value").expect("addition should render");
    let unmatched = position_of(&buf, geometry.new, "new tail three")
        .expect("unmatched addition should render");
    assert_eq!(removed.y, added.y);

    let backgrounds = |area: Rect, y| {
        (area.x..area.right())
            .map(|x| buf[(x, y)].style().bg)
            .collect::<Vec<_>>()
    };
    let pair = |y| (backgrounds(geometry.old, y), backgrounds(geometry.new, y));
    let solid = |area: Rect, background| vec![Some(background); usize::from(area.width)];
    let surface_pair = || {
        (
            solid(geometry.old, palette::SURFACE),
            solid(geometry.new, palette::SURFACE),
        )
    };

    assert_eq!(
        pair(removed.y),
        (
            solid(geometry.old, palette::DIFF_REMOVED_BACKGROUND),
            solid(geometry.new, palette::DIFF_ADDED_BACKGROUND),
        )
    );
    for y in [hunk.y, context.y] {
        assert_eq!(pair(y), surface_pair());
    }
    assert_eq!(
        pair(unmatched.y),
        (
            solid(geometry.old, palette::SURFACE),
            solid(geometry.new, palette::DIFF_ADDED_BACKGROUND),
        )
    );
    let separator_styles = geometry
        .separators
        .into_iter()
        .flatten()
        .map(|x| {
            let style = buf[(x, removed.y)].style();
            (style.fg, style.bg)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        separator_styles,
        vec![
            (Some(palette::BORDER), Some(palette::SURFACE));
            geometry.separators.into_iter().flatten().count()
        ]
    );

    let background_rows = (geometry.body.y..geometry.body.bottom())
        .map(|y| {
            let label = |area| {
                let row = backgrounds(area, y);
                let background = row.first().copied().flatten();
                if row.iter().any(|cell| *cell != background) {
                    "mixed"
                } else if background == Some(palette::DIFF_ADDED_BACKGROUND) {
                    "added"
                } else if background == Some(palette::DIFF_REMOVED_BACKGROUND) {
                    "removed"
                } else if background == Some(palette::SURFACE) {
                    "surface"
                } else {
                    "other"
                }
            };
            format!(
                "{:>7} | {:<7}  {} | {}",
                label(geometry.old),
                label(geometry.new),
                row_text(&buf, Rect::new(geometry.old.x, y, geometry.old.width, 1)),
                row_text(&buf, Rect::new(geometry.new.x, y, geometry.new.width, 1)),
            )
            .trim_end()
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("diff_line_backgrounds", background_rows);
}

#[test]
fn added_and_deleted_files_leave_the_opposite_pane_empty() {
    let mut state = fixture();
    let screen = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 24,
    );
    assert!(state.select_file(/*selected*/ 1));
    let added = render(&state, screen.width, screen.height);
    assert!(added.contains("pub fn created"));
    assert_eq!(body_text(&state, screen, |geometry| geometry.old), "");
    assert_diff_pane_surface(&state, screen, |geometry| geometry.old);

    assert!(state.select_file(/*selected*/ 2));
    let deleted = render(&state, screen.width, screen.height);
    assert!(deleted.contains("fn obsolete"));
    assert_eq!(body_text(&state, screen, |geometry| geometry.new), "");
    assert_diff_pane_surface(&state, screen, |geometry| geometry.new);
}

#[test]
fn pane_labels_and_headers_align_with_their_columns() {
    let state = fixture();
    let screen = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 39, /*height*/ 16,
    );
    let mut buf = Buffer::empty(screen);
    render_diff_view(&state, screen, &mut buf);
    let geometry = diff_view_geometry(screen);

    assert_eq!(
        row_text(&buf, column_slice(geometry.header, geometry.files)),
        "CHANGED FILES"
    );
    assert_eq!(
        row_text(&buf, column_slice(geometry.header, geometry.old)),
        "OLD FILE"
    );
    assert_eq!(
        row_text(&buf, column_slice(geometry.header, geometry.new)),
        "NEW FILE"
    );
    assert!(row_text(&buf, column_slice(geometry.labels, geometry.old)).starts_with("src/app"));
    assert!(row_text(&buf, column_slice(geometry.labels, geometry.new)).starts_with("src/app"));
}

#[test]
fn narrow_geometry_stays_partitioned_and_inside_the_screen() {
    for width in 1..=40 {
        let screen = Rect::new(/*x*/ 3, /*y*/ 2, width, /*height*/ 12);
        let geometry = diff_view_geometry(screen);

        assert!(geometry.modal.x >= screen.x);
        assert!(geometry.modal.right() <= screen.right());
        assert!(geometry.modal.y >= screen.y);
        assert!(geometry.modal.bottom() <= screen.bottom());
        assert!(geometry.files.right() <= geometry.old.x);
        assert!(geometry.old.right() <= geometry.new.x);
        assert_eq!(geometry.new.right(), geometry.body.right());
        for separator in geometry.separators.into_iter().flatten() {
            assert!(separator >= geometry.body.x);
            assert!(separator < geometry.body.right());
        }

        let mut buf = Buffer::filled(screen, ratatui::buffer::Cell::new("."));
        render_diff_view(&fixture(), screen, &mut buf);
        for y in screen.y..screen.bottom() {
            for x in screen.x..screen.right() {
                if !geometry.modal.contains(Position::new(x, y)) {
                    assert_eq!(buf[(x, y)].symbol(), ".");
                }
            }
        }
    }
}

#[test]
fn panel_uses_the_wide_cap_and_screen_margin() {
    assert_eq!(
        diff_view_panel_area(Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 220, /*height*/ 60
        )),
        Rect::new(
            /*x*/ 30,
            /*y*/ 10,
            /*width*/ MAX_MODAL_WIDTH,
            /*height*/ MAX_MODAL_HEIGHT,
        )
    );
    assert_eq!(
        diff_view_panel_area(Rect::new(
            /*x*/ 4, /*y*/ 2, /*width*/ 50, /*height*/ 14
        )),
        Rect::new(
            /*x*/ 6, /*y*/ 4, /*width*/ 46, /*height*/ 10
        )
    );
}

fn fixture() -> DiffViewState {
    let unified = "\
@@ -1,8 +1,8 @@
 use ratatui::buffer::Buffer;
-const VALUE: &str = \"stale value\";
+const VALUE: &str = \"fresh value\";
\x20
 fn render() {
     draw();
 }
 context six
 context seven
 context eight
@@ -20,4 +20,8 @@
-old tail one
-old tail two
+new tail one
+new tail two
+new tail three
 context tail
";
    DiffViewState::new(
        "Session edits",
        /*source_item_id*/ None,
        vec![
            DiffFile::modified("src/app_shell/diff_view.rs", unified, DiffStatus::Completed),
            DiffFile::added(
                "src/app_shell/new_file.rs",
                "pub fn created() {\n    println!(\"ready\");\n}\n",
                DiffStatus::Completed,
            ),
            DiffFile::deleted(
                "src/app_shell/old_file.rs",
                "fn obsolete() {\n    unreachable!();\n}\n",
                DiffStatus::Completed,
            ),
            DiffFile::renamed(
                "src/app_shell/before.rs",
                "src/app_shell/after.rs",
                "@@ -1 +1 @@\n-old name\n+new name\n",
                DiffStatus::Completed,
            ),
        ],
    )
}

fn render(state: &DiffViewState, width: u16, height: u16) -> String {
    let area = Rect::new(/*x*/ 0, /*y*/ 0, width, height);
    let mut buf = Buffer::empty(area);
    render_diff_view(state, area, &mut buf);
    buffer_text(&buf, area)
}

fn body_text(
    state: &DiffViewState,
    screen: Rect,
    pane: impl FnOnce(DiffViewGeometry) -> Rect,
) -> String {
    let mut buf = Buffer::empty(screen);
    render_diff_view(state, screen, &mut buf);
    let geometry = diff_view_geometry(screen);
    let pane = pane(geometry);
    buffer_text(
        &buf,
        Rect::new(pane.x, geometry.body.y, pane.width, geometry.body.height),
    )
    .trim()
    .to_string()
}

fn assert_diff_pane_surface(
    state: &DiffViewState,
    screen: Rect,
    pane: impl FnOnce(DiffViewGeometry) -> Rect,
) {
    let mut buf = Buffer::empty(screen);
    render_diff_view(state, screen, &mut buf);
    let geometry = diff_view_geometry(screen);
    let pane = pane(geometry);
    for y in geometry.labels.y..geometry.body.bottom() {
        for x in pane.x..pane.right() {
            assert_eq!(buf[(x, y)].style().bg, Some(palette::SURFACE));
        }
    }
}

fn row_text(buf: &Buffer, area: Rect) -> String {
    buffer_text(buf, area).trim().to_string()
}

fn position_of(buf: &Buffer, area: Rect, needle: &str) -> Option<Position> {
    (area.y..area.bottom()).find_map(|y| {
        let row = (area.x..area.right())
            .filter_map(|x| buf.cell((x, y)))
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        let start = row.find(needle)?;
        Some(Position::new(
            area.x
                .saturating_add(u16::try_from(row[..start].chars().count()).unwrap_or(u16::MAX)),
            y,
        ))
    })
}

fn buffer_text(buf: &Buffer, area: Rect) -> String {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .filter_map(|x| buf.cell((x, y)))
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
