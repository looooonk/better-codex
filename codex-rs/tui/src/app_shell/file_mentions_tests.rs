use super::*;
use crate::app_shell::render::ShellView;
use codex_app_server_protocol::FuzzyFileSearchMatchType;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn selecting_a_file_preserves_a_new_draft_and_quotes_ambiguous_paths() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.set_text("Please inspect");
    shell.insert_file_mention("/workspace/my file.rs");
    assert_eq!(
        shell.composer.text(),
        "Please inspect @\"/workspace/my file.rs\" "
    );
    shell.insert_file_mention("/workspace/lib.rs");
    assert_eq!(
        shell.composer.text(),
        "Please inspect @\"/workspace/my file.rs\" @/workspace/lib.rs "
    );
}

#[test]
fn matching_files_use_the_workspace_selector() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.open_file_mentions(vec![FuzzyFileSearchResult {
        root: "/workspace/project".to_string(),
        path: "src/main.rs".to_string(),
        match_type: FuzzyFileSearchMatchType::File,
        file_name: "main.rs".to_string(),
        score: 100,
        indices: None,
    }]);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 22,
    );
    let mut buffer = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut buffer);
    let text = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(text);
}
