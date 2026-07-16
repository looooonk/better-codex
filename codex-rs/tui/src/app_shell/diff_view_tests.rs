use super::*;
use codex_app_server_protocol::PatchChangeKind;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn change(path: &str, kind: PatchChangeKind, diff: &str) -> FileUpdateChange {
    FileUpdateChange {
        path: path.to_string(),
        kind,
        diff: diff.to_string(),
    }
}

#[test]
fn file_update_change_variants_build_expected_files() {
    let changes = [
        change("src/new.rs", PatchChangeKind::Add, "alpha\n+++literal\n\n"),
        change("src/old.rs", PatchChangeKind::Delete, "omega\n---literal\n"),
        change(
            "src/edit.rs",
            PatchChangeKind::Update { move_path: None },
            "@@ -4,3 +4,3 @@\n same\n-old\n+new\n tail\n",
        ),
        change(
            "src/from.rs",
            PatchChangeKind::Update {
                move_path: Some(PathBuf::from("src/to.rs")),
            },
            "@@ -1 +1 @@\n-left\n+right\n",
        ),
    ];

    let actual = changes
        .iter()
        .map(|change| DiffFile::from_change(change, DiffStatus::InProgress))
        .collect::<Vec<_>>();
    let expected = vec![
        DiffFile::added(
            "src/new.rs",
            "alpha\n+++literal\n\n",
            DiffStatus::InProgress,
        ),
        DiffFile::deleted("src/old.rs", "omega\n---literal\n", DiffStatus::InProgress),
        DiffFile::modified(
            "src/edit.rs",
            "@@ -4,3 +4,3 @@\n same\n-old\n+new\n tail\n",
            DiffStatus::InProgress,
        ),
        DiffFile::renamed(
            "src/from.rs",
            "src/to.rs",
            "@@ -1 +1 @@\n-left\n+right\n",
            DiffStatus::InProgress,
        ),
    ];

    assert_eq!(actual, expected);
}

#[test]
fn parses_multi_file_unified_diff_with_one_sided_files() {
    let unified = "\
diff --git a/src/edit.rs b/src/edit.rs
--- a/src/edit.rs
+++ b/src/edit.rs
@@ -1 +1 @@
-old
+new
diff --git a/src/new.rs b/src/new.rs
new file mode 100644
--- /dev/null
+++ b/src/new.rs
@@ -0,0 +1,2 @@
+first
++++literal
diff --git a/src/old.rs b/src/old.rs
deleted file mode 100644
--- a/src/old.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-last
----literal
diff --git a/src/from.rs b/src/to.rs
--- a/src/from.rs
+++ b/src/to.rs
@@ -8 +8 @@
-before
+after
";

    let files = parse_unified_diff(unified);

    assert_eq!(
        files,
        vec![
            DiffFile::modified(
                "src/edit.rs",
                "@@ -1 +1 @@\n-old\n+new",
                DiffStatus::Completed,
            ),
            DiffFile::added("src/new.rs", "first\n+++literal", DiffStatus::Completed,),
            DiffFile::deleted("src/old.rs", "last\n---literal", DiffStatus::Completed,),
            DiffFile::renamed(
                "src/from.rs",
                "src/to.rs",
                "@@ -8 +8 @@\n-before\n+after",
                DiffStatus::Completed,
            ),
        ]
    );
    assert!(files[1].rows().iter().all(|row| row.old.is_none()));
    assert!(files[2].rows().iter().all(|row| row.new.is_none()));
}

#[test]
fn patch_statuses_control_session_eligibility() {
    let statuses = [
        PatchApplyStatus::InProgress,
        PatchApplyStatus::Completed,
        PatchApplyStatus::Failed,
        PatchApplyStatus::Declined,
    ]
    .map(DiffStatus::from);

    assert_eq!(
        statuses,
        [
            DiffStatus::InProgress,
            DiffStatus::Completed,
            DiffStatus::Failed,
            DiffStatus::Declined,
        ]
    );
    assert_eq!(
        statuses.map(DiffStatus::is_session_edit),
        [true, true, false, false]
    );
}

#[test]
fn session_aggregation_filters_statuses_and_prefers_turn_diffs() {
    let completed = change("one.txt", PatchChangeKind::Add, "one\ntwo\n");
    let failed = change("failed.txt", PatchChangeKind::Delete, "gone\n");
    let in_progress = change(
        "progress.txt",
        PatchChangeKind::Update { move_path: None },
        "@@ -1 +1 @@\n-old\n+new\n",
    );
    let declined = change("declined.txt", PatchChangeKind::Add, "nope\n");
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "completed",
        vec![completed.clone()],
        PatchApplyStatus::Completed,
    );
    store.upsert_item("turn-1", "failed", vec![failed], PatchApplyStatus::Failed);
    store.upsert_item(
        "turn-2",
        "progress",
        vec![in_progress.clone()],
        PatchApplyStatus::InProgress,
    );
    store.upsert_item(
        "turn-2",
        "declined",
        vec![declined],
        PatchApplyStatus::Declined,
    );

    assert!(store.has_session_edits());
    assert_eq!(
        store.session_files(),
        vec![
            DiffFile::from_change(&completed, DiffStatus::Completed),
            DiffFile::from_change(&in_progress, DiffStatus::InProgress),
        ]
    );
    assert_eq!(
        store.session_stats(),
        DiffStats {
            files: 2,
            additions: 3,
            removals: 1,
        }
    );

    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/progress.txt b/progress.txt\n--- a/progress.txt\n+++ b/progress.txt\n@@ -1 +1 @@\n-old\n+net\n",
    );
    let aggregate = parse_unified_diff(
        "diff --git a/progress.txt b/progress.txt\n--- a/progress.txt\n+++ b/progress.txt\n@@ -1 +1 @@\n-old\n+net\n",
    );
    assert_eq!(
        store.session_files(),
        [
            vec![DiffFile::from_change(&completed, DiffStatus::Completed,)],
            aggregate.clone()
        ]
        .concat()
    );

    store.upsert_item(
        "turn-1",
        "completed",
        vec![completed],
        PatchApplyStatus::Failed,
    );
    assert_eq!(store.session_stats(), DiffStats::from_files(&aggregate));
    store.remove_turn("turn-2");
    assert!(!store.has_session_edits());
    assert_eq!(store.session_stats(), DiffStats::default());
    store.clear();
    assert_eq!(store.session_files(), Vec::new());
}

#[test]
fn session_aggregation_keeps_item_changes_missing_from_the_turn_diff() {
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "rename",
        vec![change(
            "old.txt",
            PatchChangeKind::Update {
                move_path: Some(PathBuf::from("new.txt")),
            },
            "@@ -1 +1 @@\n same\n\nMoved to: new.txt",
        )],
        PatchApplyStatus::Completed,
    );
    store.upsert_item(
        "turn-1",
        "edit",
        vec![change(
            "edited.txt",
            PatchChangeKind::Update { move_path: None },
            "@@ -1 +1 @@\n-before\n+after\n",
        )],
        PatchApplyStatus::Completed,
    );
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/edited.txt b/edited.txt\n--- a/edited.txt\n+++ b/edited.txt\n@@ -1 +1 @@\n-before\n+after\n",
    );

    assert_eq!(
        store
            .session_files()
            .iter()
            .map(DiffFile::display_path)
            .collect::<Vec<_>>(),
        vec!["edited.txt", "old.txt -> new.txt"]
    );
}

#[test]
fn selecting_files_and_keyboard_navigation_reset_scroll() {
    let files = vec![
        DiffFile::modified("a.rs", "@@\n-a\n+A", DiffStatus::Completed),
        DiffFile::modified("b.rs", "@@\n-b\n+B", DiffStatus::Completed),
    ];
    let mut state = DiffViewState::new("Changes", Some("item-1".to_string()), files);
    state.set_scroll_max(40);
    state.scroll_down(/*amount*/ 7);

    assert!(!state.select_file(/*selected*/ 0));
    assert_eq!(state.scroll(), 7);
    assert!(state.select_file(/*selected*/ 1));
    assert_eq!(
        (
            state.title(),
            state.source_item_id(),
            state.selected_file_index(),
            state.scroll(),
            state.scroll_max.get(),
        ),
        ("Changes", Some("item-1"), 1, 0, 0)
    );

    state.set_scroll_max(40);
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
        DiffViewAction::Pending
    );
    assert_eq!(state.scroll(), DIFF_PAGE_STEP);
    state.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(state.scroll(), 0);
    state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!((state.selected_file_index(), state.scroll()), (0, 0));
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        DiffViewAction::Close
    );
}

#[test]
fn live_file_replacement_preserves_identity_and_resets_scroll() {
    let old_a = DiffFile::modified("a.rs", "@@\n-a\n+A", DiffStatus::InProgress);
    let old_b = DiffFile::modified("b.rs", "@@\n-b\n+B", DiffStatus::InProgress);
    let mut state = DiffViewState::new("Changes", None, vec![old_a, old_b]);
    state.select_file(/*selected*/ 1);
    state.set_scroll_max(20);
    state.scroll_down(/*amount*/ 9);
    let updated_b = DiffFile::modified(
        "b.rs",
        "@@ -1 +1,2 @@\n-b\n+B\n+more",
        DiffStatus::Completed,
    );
    let new_c = DiffFile::added("c.rs", "created\n", DiffStatus::Completed);

    state.replace_files(vec![updated_b.clone(), new_c.clone()]);

    assert_eq!(state.selected_file(), Some(&updated_b));
    assert_eq!(
        (
            state.selected_file_index(),
            state.scroll(),
            state.scroll_max.get(),
        ),
        (0, 0, 0)
    );

    state.set_scroll_max(10);
    state.scroll_down(/*amount*/ 4);
    state.replace_files(vec![new_c.clone()]);
    assert_eq!(state.selected_file(), Some(&new_c));
    assert_eq!((state.selected_file_index(), state.scroll()), (0, 0));
    state.replace_files(Vec::new());
    assert_eq!(state.selected_file(), None);
}
