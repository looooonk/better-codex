use super::*;
use crate::app_shell::ShellState;
use crate::app_shell::tests::render_shell;
use crate::markdown_render::source_file_link;
use pretty_assertions::assert_eq;

#[test]
fn citation_opens_at_its_line_and_column() {
    let link = source_file_link("./Makefile#L5C3").unwrap();
    assert_eq!(link.location, Some(SourceLocation { line: 5, column: 3 }));
    let source = "one\ntwo\nthree\nfour\nabcdef\nsix\nseven\n";
    let mut shell = ShellState::snapshot_fixture();
    shell.preview = Some(crate::app_shell::preview::Preview::Source(
        SourcePreview::new(link, source.as_bytes().to_vec()).unwrap(),
    ));
    let area = Rect::new(0, 0, 64, 14);
    insta::assert_snapshot!("source_citation", render_shell(&shell, area));
    let mut buffer = Buffer::empty(area);
    shell.preview.as_ref().unwrap().render(area, &mut buffer);
    assert!(
        buffer.content.iter().any(|cell| cell.symbol() == "c"
            && cell.modifier.contains(ratatui::style::Modifier::REVERSED))
    );
}

#[test]
fn binary_and_oversized_previews_are_rejected() {
    let link = source_file_link("src/main.rs:7:2").unwrap();
    assert!(SourcePreview::new(link.clone(), vec![0]).is_err());
    assert!(SourcePreview::new(link, vec![b'a'; MAX_SOURCE_BYTES + 1]).is_err());
}

#[test]
fn source_columns_follow_characters_across_tabs_and_combining_marks() {
    let link = source_file_link("hello.rs:1:5").unwrap();
    let preview = SourcePreview::new(link, "\t界e\u{301}z".as_bytes().to_vec()).unwrap();
    let area = Rect::new(0, 0, 48, 12);
    let mut buffer = Buffer::empty(area);
    preview.render(area, &mut buffer);
    assert!(
        buffer.content.iter().any(|cell| cell.symbol() == "z"
            && cell.modifier.contains(ratatui::style::Modifier::REVERSED))
    );
}
