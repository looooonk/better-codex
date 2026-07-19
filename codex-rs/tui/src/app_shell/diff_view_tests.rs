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
fn parses_git_c_quoted_paths_without_losing_bytes_or_boundaries() {
    let unified = r#"diff --git "a/quote\"-tab\t-line\n-caf\303\251.txt" "b/quote\"-tab\t-line\n-caf\303\251.txt"
--- "a/quote\"-tab\t-line\n-caf\303\251.txt"
+++ "b/quote\"-tab\t-line\n-caf\303\251.txt"
@@ -1 +1 @@
-old
+new
"#;

    let files = parse_unified_diff(unified);

    assert_eq!(
        files,
        vec![DiffFile::modified(
            "quote\"-tab\t-line\n-café.txt",
            "@@ -1 +1 @@\n-old\n+new",
            DiffStatus::Completed,
        )]
    );
    assert_eq!(files[0].display_path(), "quote\"-tab\\t-line\\n-café.txt");
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
fn session_aggregation_composes_cross_turn_updates_and_reversions() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/shared.txt b/shared.txt\n--- a/shared.txt\n+++ b/shared.txt\n@@ -1 +1 @@\n-original\n+middle\n",
    );
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/shared.txt b/shared.txt\n--- a/shared.txt\n+++ b/shared.txt\n@@ -1 +1 @@\n-middle\n+final\n",
    );

    assert_eq!(
        (store.session_files(), store.session_stats()),
        (
            vec![DiffFile::modified(
                "shared.txt",
                "@@ -1,1 +1,1 @@\n-original\n+final",
                DiffStatus::Completed,
            )],
            DiffStats {
                files: 1,
                additions: 1,
                removals: 1,
            },
        )
    );

    store.upsert_turn_diff(
        "turn-3",
        "diff --git a/shared.txt b/shared.txt\n--- a/shared.txt\n+++ b/shared.txt\n@@ -1 +1 @@\n-final\n+original\n",
    );
    assert_eq!(
        (store.session_files(), store.session_stats()),
        (Vec::new(), DiffStats::default())
    );
}

#[test]
fn session_aggregation_composes_large_line_numbers_in_a_bounded_window() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/large.txt b/large.txt\n--- a/large.txt\n+++ b/large.txt\n@@ -1000000 +1000000 @@\n-original\n+middle\n",
    );
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/large.txt b/large.txt\n--- a/large.txt\n+++ b/large.txt\n@@ -1000000 +1000000 @@\n-middle\n+final\n",
    );

    assert_eq!(
        store.session_files(),
        vec![DiffFile::modified(
            "large.txt",
            "@@ -1000000,1 +1000000,1 @@\n-original\n+final",
            DiffStatus::Completed,
        )]
    );
}

#[test]
fn oversized_diff_update_is_bounded_before_parsing() {
    let path = "p".repeat(8_000);
    let diff = std::iter::once("@@ -1,5000 +1,5000 @@".to_string())
        .chain((0..5_000).map(|index| format!(" {index:04} {}", "x".repeat(180))))
        .collect::<Vec<_>>()
        .join("\n");
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-large",
        "item-large",
        &[change(
            &path,
            PatchChangeKind::Update { move_path: None },
            &diff,
        )],
        PatchApplyStatus::Completed,
    );

    let files = store
        .item_files("item-large")
        .expect("bounded item should remain available");
    let size = store.retained_size();
    assert_eq!(
        (
            store.item_is_truncated("item-large"),
            store.session_is_truncated(),
            files.len(),
            files[0].display_path().len() <= 1_024,
            size.text_bytes <= MAX_DIFF_STORE_TEXT_BYTES,
            size.rows <= MAX_DIFF_STORE_ROWS,
            size.files <= MAX_DIFF_STORE_FILES,
        ),
        (true, true, 1, true, true, true, true)
    );
}

#[test]
fn diff_store_evicts_oldest_history_at_session_caps() {
    let content = (0..80)
        .map(|index| format!("line {index:03}: {}", "x".repeat(80)))
        .collect::<Vec<_>>()
        .join("\n");
    let mut store = DiffStore::default();
    for index in 0..=MAX_DIFF_STORE_ITEMS {
        store.upsert_item(
            "turn-items",
            format!("item-{index:03}"),
            &[change(
                &format!("src/file-{index:03}.rs"),
                PatchChangeKind::Add,
                &content,
            )],
            PatchApplyStatus::Completed,
        );
    }
    let item_size = store.retained_size();
    assert_eq!(
        (
            store.item_files("item-000"),
            store
                .item_files(&format!("item-{MAX_DIFF_STORE_ITEMS:03}"))
                .is_some(),
            item_size.text_bytes <= MAX_DIFF_STORE_TEXT_BYTES,
            item_size.rows <= MAX_DIFF_STORE_ROWS,
            item_size.files <= MAX_DIFF_STORE_FILES,
            item_size.items <= MAX_DIFF_STORE_ITEMS,
        ),
        (None, true, true, true, true, true)
    );
    for index in 0..=MAX_DIFF_STORE_TURNS {
        store.upsert_turn_diff(
            format!("turn-{index:03}"),
            &format!(
                "diff --git a/file-{index}.rs b/file-{index}.rs\n--- a/file-{index}.rs\n+++ b/file-{index}.rs\n@@ -1 +1 @@\n-old\n+new\n"
            ),
        );
    }

    let size = store.retained_size();
    assert_eq!(
        (
            store.item_files("item-000"),
            store.turns.len() <= MAX_DIFF_STORE_TURNS,
            size.text_bytes <= MAX_DIFF_STORE_TEXT_BYTES,
            size.rows <= MAX_DIFF_STORE_ROWS,
            size.files <= MAX_DIFF_STORE_FILES,
            size.items <= MAX_DIFF_STORE_ITEMS,
            store.session_is_truncated(),
        ),
        (None, true, true, true, true, true, true)
    );
    assert!(
        store
            .session_files()
            .iter()
            .any(|file| { file.display_path() == format!("file-{MAX_DIFF_STORE_TURNS}.rs") })
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
    state.horizontal_scroll.set_max(40);
    state.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));

    assert!(!state.select_file(/*selected*/ 0));
    assert_eq!((state.scroll(), state.horizontal_scroll.offset()), (7, 8));
    assert!(state.select_file(/*selected*/ 1));
    assert_eq!(
        (
            state.title(),
            state.source_item_id(),
            state.selected_file_index(),
            state.scroll(),
            state.scroll_max.get(),
            state.horizontal_scroll.offset(),
            state.horizontal_scroll.max(),
        ),
        ("Changes", Some("item-1"), 1, 0, 0, 0, 0)
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
    state.horizontal_scroll.set_max(20);
    state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT));
    let updated_b = DiffFile::modified(
        "b.rs",
        "@@ -1 +1,2 @@\n-b\n+B\n+more",
        DiffStatus::Completed,
    );
    let new_c = DiffFile::added("c.rs", "created\n", DiffStatus::Completed);

    state.replace_files(
        vec![updated_b.clone(), new_c.clone()],
        DiffRetention::Complete,
    );

    assert_eq!(state.selected_file(), Some(&updated_b));
    assert_eq!(
        (
            state.selected_file_index(),
            state.scroll(),
            state.scroll_max.get(),
            state.horizontal_scroll.offset(),
            state.horizontal_scroll.max(),
        ),
        (0, 0, 0, 0, 0)
    );

    state.set_scroll_max(10);
    state.scroll_down(/*amount*/ 4);
    state.replace_files(vec![new_c.clone()], DiffRetention::Complete);
    assert_eq!(state.selected_file(), Some(&new_c));
    assert_eq!((state.selected_file_index(), state.scroll()), (0, 0));
    state.replace_files(Vec::new(), DiffRetention::Complete);
    assert_eq!(state.selected_file(), None);
}

#[test]
fn horizontal_scroll_preserves_columns_across_wide_characters() {
    let mut state = DiffViewState::new("Changes", None, Vec::new());
    state.horizontal_scroll.set_max(1);
    state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT));

    assert_eq!(state.horizontal_scroll.visible_text("界x", 2), " x");
}
