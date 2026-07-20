use super::*;
use crate::app_shell::diff_view::DiffFile;
use crate::app_shell::diff_view::DiffStatus;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;

#[test]
fn closes_session_popup_when_composed_edits_become_empty() {
    let mut shell = ShellState::snapshot_fixture();
    shell.record_turn_diff(
        "turn-1",
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-before\n+after\n",
    );
    assert!(shell.open_session_diff_view());

    shell.record_turn_diff(
        "turn-2",
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-after\n+before\n",
    );

    assert!(shell.diff_view.is_none());
}

#[test]
fn closes_item_popup_when_source_item_disappears() {
    let mut shell = ShellState::snapshot_fixture();
    let change = FileUpdateChange {
        path: "src/lib.rs".to_string(),
        kind: PatchChangeKind::Update { move_path: None },
        diff: "@@ -1 +1 @@\n-before\n+after\n".to_string(),
    };
    shell.record_file_changes(
        "turn-1",
        "item-1",
        std::slice::from_ref(&change),
        PatchApplyStatus::Completed,
    );
    shell.diff_view = Some(DiffViewState::new(
        "File changes",
        Some("item-1".to_string()),
        vec![DiffFile::from_change(&change, DiffStatus::Completed)],
    ));

    shell.diff_store.remove_turn("turn-1");
    shell.refresh_open_diff_view();

    assert!(shell.diff_view.is_none());
}

#[test]
fn refreshes_open_popup_paths_after_the_display_root_changes() {
    let mut shell = ShellState::snapshot_fixture();
    shell
        .diff_store
        .set_display_root(std::path::Path::new("/workspace"));
    shell.record_file_changes(
        "turn-1",
        "item-1",
        &[FileUpdateChange {
            path: "/workspace/project/src/lib.rs".to_string(),
            kind: PatchChangeKind::Update { move_path: None },
            diff: "@@ -1 +1 @@\n-before\n+after\n".to_string(),
        }],
        PatchApplyStatus::Completed,
    );
    assert!(shell.open_session_diff_view());
    assert_eq!(
        shell
            .diff_view
            .as_ref()
            .and_then(DiffViewState::selected_file)
            .map(DiffFile::display_path),
        Some("project/src/lib.rs".to_string())
    );

    shell
        .diff_store
        .set_display_root(std::path::Path::new("/workspace/project"));
    shell.refresh_open_diff_view();

    assert_eq!(
        shell
            .diff_view
            .as_ref()
            .and_then(DiffViewState::selected_file)
            .map(DiffFile::display_path),
        Some("src/lib.rs".to_string())
    );
}
