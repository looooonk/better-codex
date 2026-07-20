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
fn streaming_apply_patch_context_hunks_render_changed_lines() {
    let file = DiffFile::modified(
        "src/lib.rs",
        "@@ impl Widget\n-before\n+after\n",
        DiffStatus::InProgress,
    );

    assert_eq!(
        file.stats(),
        DiffStats {
            files: 1,
            additions: 1,
            removals: 1,
        }
    );
    assert_eq!(
        file.rows()[0].old.as_ref().map(|cell| cell.text.as_str()),
        Some("@@ impl Widget")
    );
    assert_eq!(
        file.rows()[1]
            .old
            .as_ref()
            .zip(file.rows()[1].new.as_ref())
            .map(|(old, new)| (old.line_number, new.line_number)),
        Some((None, None))
    );
}

#[test]
fn symbolic_hunks_are_not_composed_at_a_fabricated_line_number() {
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "item-1",
        &[change(
            "src/lib.rs",
            PatchChangeKind::Update { move_path: None },
            "@@ impl Widget\n-before\n+middle\n",
        )],
        PatchApplyStatus::Completed,
    );
    store.upsert_item(
        "turn-2",
        "item-2",
        &[change(
            "src/lib.rs",
            PatchChangeKind::Update { move_path: None },
            "@@ impl Widget\n-middle\n+after\n",
        )],
        PatchApplyStatus::Completed,
    );

    assert!(store.session_is_truncated());
    assert_eq!(
        store.session_files(),
        vec![DiffFile::modified(
            "src/lib.rs",
            "@@ impl Widget\n-middle\n+after\n",
            DiffStatus::Completed,
        )]
    );
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

    let expected = [
        DiffFile::modified(
            "src/edit.rs",
            "@@ -1 +1 @@\n-old\n+new",
            DiffStatus::Completed,
        ),
        DiffFile::added("src/new.rs", "first\n+++literal", DiffStatus::Completed),
        DiffFile::deleted("src/old.rs", "last\n---literal", DiffStatus::Completed),
        DiffFile::renamed(
            "src/from.rs",
            "src/to.rs",
            "@@ -8 +8 @@\n-before\n+after",
            DiffStatus::Completed,
        ),
    ];
    let visible_parts = |file: &DiffFile| {
        (
            file.old_label().map(str::to_string),
            file.new_label().map(str::to_string),
            file.kind(),
            file.status(),
            file.rows().to_vec(),
        )
    };
    assert_eq!(
        files.iter().map(visible_parts).collect::<Vec<_>>(),
        expected.iter().map(visible_parts).collect::<Vec<_>>()
    );
    assert_eq!(
        (
            files[1].metadata().mode_transition(),
            files[2].metadata().mode_transition(),
        ),
        (Some((None, Some("100644"))), Some((Some("100644"), None)),)
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
fn rename_metadata_disambiguates_unquoted_git_paths_with_b_prefixes() {
    let unified = "\
diff --git a/x b/a b/y
similarity index 50%
rename from x b/a
rename to y
--- a/x b/a
+++ b/y
@@ -1 +1 @@
-old
+new
";

    assert_eq!(
        parse_unified_diff(unified),
        vec![DiffFile::renamed(
            "x b/a",
            "y",
            "@@ -1 +1 @@\n-old\n+new",
            DiffStatus::Completed,
        )]
    );
}

#[test]
fn git_header_paths_win_when_raw_tabs_look_like_timestamps() {
    let timestamp_path = "src/timestamp.rs\t2026-07-20 12:34:56 +0900";
    let trailing_tab_path = "src/trailing-tab\t";
    let unified = format!(
        "diff --git a/{timestamp_path} b/{timestamp_path}\n--- a/{timestamp_path}\n+++ b/{timestamp_path}\n@@ -1 +1 @@\n-old timestamp\n+new timestamp\ndiff --git a/{trailing_tab_path} b/{trailing_tab_path}\n--- a/{trailing_tab_path}\n+++ b/{trailing_tab_path}\n@@ -1 +1 @@\n-old tab\n+new tab\n"
    );

    assert_eq!(
        parse_unified_diff(&unified),
        vec![
            DiffFile::modified(
                timestamp_path,
                "@@ -1 +1 @@\n-old timestamp\n+new timestamp",
                DiffStatus::Completed,
            ),
            DiffFile::modified(
                trailing_tab_path,
                "@@ -1 +1 @@\n-old tab\n+new tab",
                DiffStatus::Completed,
            ),
        ]
    );
}

#[test]
fn parses_metadata_only_binary_files_and_pure_renames() {
    let unified = "\
diff --git a/assets/new.bin b/assets/new.bin
new file mode 100644
index 0000000..1234567
Binary files /dev/null and b/assets/new.bin differ
diff --git a/assets/old.bin b/assets/old.bin
deleted file mode 100644
index 1234567..0000000
Binary files a/assets/old.bin and /dev/null differ
diff --git a/src/before.rs b/src/after.rs
similarity index 100%
rename from src/before.rs
rename to src/after.rs
diff --git a/scripts/run.sh b/scripts/run.sh
old mode 100644
new mode 100755
";

    let files = parse_unified_diff(unified);
    assert_eq!(
        files
            .iter()
            .map(|file| (file.kind(), file.display_path()))
            .collect::<Vec<_>>(),
        vec![
            (DiffFileKind::Added, "assets/new.bin".to_string()),
            (DiffFileKind::Deleted, "assets/old.bin".to_string()),
            (
                DiffFileKind::Renamed,
                "src/before.rs -> src/after.rs".to_string(),
            ),
            (DiffFileKind::Modified, "scripts/run.sh".to_string()),
        ]
    );
    assert_eq!(
        (
            files[0].metadata().mode_transition(),
            files[0].metadata().binary_oid_transition(),
            files[1].metadata().mode_transition(),
            files[1].metadata().binary_oid_transition(),
            files[3].metadata().mode_transition(),
        ),
        (
            Some((None, Some("100644"))),
            Some((None, Some("1234567"))),
            Some((Some("100644"), None)),
            Some((Some("1234567"), None)),
            Some((Some("100644"), Some("100755"))),
        )
    );
}

#[test]
fn parses_detected_copies_as_additions_without_removing_the_source() {
    let unified = "\
diff --git a/src/original.rs b/src/copy.rs
similarity index 100%
copy from src/original.rs
copy to src/copy.rs
";

    assert_eq!(
        parse_unified_diff(unified),
        vec![DiffFile::added("src/copy.rs", "", DiffStatus::Completed,)]
    );
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
fn a_new_item_after_an_aggregate_is_composed_until_the_next_aggregate() {
    let first = change("first.txt", PatchChangeKind::Add, "first\n");
    let second = change("second.txt", PatchChangeKind::Add, "second\n");
    let first_aggregate = "\
diff --git a/first.txt b/first.txt
new file mode 100644
--- /dev/null
+++ b/first.txt
@@ -0,0 +1 @@
+first
";
    let complete_aggregate = format!(
        "{first_aggregate}diff --git a/second.txt b/second.txt\nnew file mode 100644\n--- /dev/null\n+++ b/second.txt\n@@ -0,0 +1 @@\n+second\n"
    );
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "first-item",
        std::slice::from_ref(&first),
        PatchApplyStatus::Completed,
    );
    store.upsert_turn_diff("turn-1", first_aggregate);

    store.upsert_item(
        "turn-1",
        "second-item",
        std::slice::from_ref(&second),
        PatchApplyStatus::Completed,
    );

    assert_eq!(
        store.session_files(),
        [
            parse_unified_diff(first_aggregate),
            vec![DiffFile::from_change(&second, DiffStatus::Completed)],
        ]
        .concat()
    );
    assert!(!store.session_is_truncated());

    store.upsert_turn_diff("turn-1", &complete_aggregate);

    assert_eq!(
        store.session_files(),
        parse_unified_diff(&complete_aggregate)
    );
    assert!(!store.session_is_truncated());
}

#[test]
fn nonempty_turn_aggregate_retains_an_unrepresented_pure_rename() {
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
fn session_aggregation_reconciles_absolute_item_and_relative_git_paths() {
    let mut store = DiffStore::with_display_root(std::path::Path::new("/workspace/project/sub"));
    store.upsert_item(
        "turn-1",
        "item-1",
        &[change(
            "/workspace/project/sub/src/lib.rs",
            PatchChangeKind::Update { move_path: None },
            "@@ -1 +1 @@\n-before\n+item\n",
        )],
        PatchApplyStatus::Completed,
    );
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/sub/src/lib.rs b/sub/src/lib.rs\n--- a/sub/src/lib.rs\n+++ b/sub/src/lib.rs\n@@ -1 +1 @@\n-before\n+aggregate\n",
    );
    store.set_git_root(std::path::Path::new("/workspace/project"));

    let files = store.session_files();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].display_path(), "sub/src/lib.rs");
    assert_eq!(
        store.session_stats(),
        DiffStats {
            files: 1,
            additions: 1,
            removals: 1,
        }
    );
}

#[test]
fn same_cwd_settings_update_preserves_the_confirmed_git_root() {
    let mut store = DiffStore::with_display_root(std::path::Path::new("/workspace/project/sub"));
    store.set_git_root(std::path::Path::new("/workspace/project"));
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/sub/src/lib.rs b/sub/src/lib.rs\n--- a/sub/src/lib.rs\n+++ b/sub/src/lib.rs\n@@ -1 +1 @@\n-before\n+middle\n",
    );

    store.set_display_root(std::path::Path::new("/workspace/project/sub"));
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/sub/src/lib.rs b/sub/src/lib.rs\n--- a/sub/src/lib.rs\n+++ b/sub/src/lib.rs\n@@ -1 +1 @@\n-middle\n+after\n",
    );

    assert_eq!(
        store
            .session_files()
            .iter()
            .map(DiffFile::display_path)
            .collect::<Vec<_>>(),
        vec!["sub/src/lib.rs"]
    );
    assert_eq!(
        store.session_stats(),
        DiffStats {
            files: 1,
            additions: 1,
            removals: 1,
        }
    );
}

#[test]
fn confirmed_git_root_change_does_not_reroot_historical_aggregates() {
    let mut store = DiffStore::with_display_root(std::path::Path::new("/workspace/outer/sub"));
    store.set_git_root(std::path::Path::new("/workspace/outer"));
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/sub/src/lib.rs b/sub/src/lib.rs\n--- a/sub/src/lib.rs\n+++ b/sub/src/lib.rs\n@@ -1 +1 @@\n-before\n+middle\n",
    );

    store.set_git_root(std::path::Path::new("/workspace/outer/sub"));
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-middle\n+after\n",
    );

    assert_eq!(
        store
            .session_files()
            .iter()
            .map(DiffFile::display_path)
            .collect::<Vec<_>>(),
        vec!["src/lib.rs"]
    );
    assert_eq!(
        store.session_stats(),
        DiffStats {
            files: 1,
            additions: 1,
            removals: 1,
        }
    );
}

#[test]
fn definitive_non_repository_probe_falls_back_to_the_cwd() {
    let mut store = DiffStore::with_display_root(std::path::Path::new("/workspace/project/sub"));
    store.set_git_root(std::path::Path::new("/workspace/project"));
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/sub/src/lib.rs b/sub/src/lib.rs\n--- a/sub/src/lib.rs\n+++ b/sub/src/lib.rs\n@@ -1 +1 @@\n-before\n+middle\n",
    );

    store.confirm_no_git_root();
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-middle\n+after\n",
    );

    assert_eq!(
        store
            .session_files()
            .iter()
            .map(DiffFile::display_path)
            .collect::<Vec<_>>(),
        vec!["src/lib.rs"]
    );
    assert_eq!(
        store.session_stats(),
        DiffStats {
            files: 1,
            additions: 1,
            removals: 1,
        }
    );
}

#[test]
fn nonempty_turn_aggregate_drops_an_outside_item_path() {
    let mut store = DiffStore::with_display_root(std::path::Path::new("/workspace/project"));
    store.upsert_item(
        "turn-1",
        "item-1",
        &[change(
            "/tmp/other/src/lib.rs",
            PatchChangeKind::Update { move_path: None },
            "@@ -1 +1 @@\n-before\n+outside\n",
        )],
        PatchApplyStatus::Completed,
    );
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-before\n+workspace\n",
    );

    assert_eq!(store.session_files().len(), 1);
    assert_eq!(store.session_files()[0].display_path(), "src/lib.rs");
}

#[test]
fn distinct_long_paths_do_not_collapse_when_their_labels_are_bounded() {
    let prefix = "p".repeat(1_100);
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "item-1",
        &[
            change(
                &format!("/{prefix}/first.rs"),
                PatchChangeKind::Add,
                "first\n",
            ),
            change(
                &format!("/{prefix}/second.rs"),
                PatchChangeKind::Add,
                "second\n",
            ),
        ],
        PatchApplyStatus::Completed,
    );

    let files = store.session_files();
    assert_eq!(files.len(), 2);
    assert_ne!(files[0].display_path(), files[1].display_path());
}

#[test]
fn completed_items_ignore_late_in_progress_updates() {
    let completed = change("src/lib.rs", PatchChangeKind::Add, "completed\n");
    let stale = change("src/lib.rs", PatchChangeKind::Add, "stale\nextra\n");
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "item-1",
        std::slice::from_ref(&completed),
        PatchApplyStatus::Completed,
    );
    store.upsert_item("turn-1", "item-1", &[stale], PatchApplyStatus::InProgress);

    assert_eq!(
        store.item_files("item-1"),
        Some([DiffFile::from_change(&completed, DiffStatus::Completed)].as_slice())
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
fn session_aggregation_removes_reverted_renames_and_delete_recreates() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/old.txt b/new.txt\nsimilarity index 100%\nrename from old.txt\nrename to new.txt\n",
    );
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/new.txt b/old.txt\nsimilarity index 100%\nrename from new.txt\nrename to old.txt\n",
    );
    assert_eq!(store.session_files(), Vec::new());

    store.clear();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/file.txt b/file.txt\ndeleted file mode 100644\n--- a/file.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-same\n",
    );
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/file.txt b/file.txt\nnew file mode 100644\n--- /dev/null\n+++ b/file.txt\n@@ -0,0 +1 @@\n+same\n",
    );
    assert_eq!(store.session_files(), Vec::new());
    assert!(!store.session_is_truncated());
}

#[test]
fn delete_recreate_with_a_different_mode_retains_metadata_only() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/file.txt b/file.txt\ndeleted file mode 100644\n--- a/file.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-same\n",
    );
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/file.txt b/file.txt\nnew file mode 100755\n--- /dev/null\n+++ b/file.txt\n@@ -0,0 +1 @@\n+same\n",
    );

    assert_eq!(
        store.session_files(),
        vec![DiffFile::modified(
            "file.txt",
            "old mode 100644\nnew mode 100755",
            DiffStatus::Completed,
        )]
    );
    assert!(!store.session_is_truncated());
}

#[test]
fn binary_delete_recreate_composes_object_id_endpoints() {
    let deleted = "diff --git a/image.bin b/image.bin\ndeleted file mode 100644\nindex 1111111..0000000\nBinary files a/image.bin and /dev/null differ\n";
    let same = "diff --git a/image.bin b/image.bin\nnew file mode 100644\nindex 0000000..1111111\nBinary files /dev/null and b/image.bin differ\n";
    let different = "diff --git a/image.bin b/image.bin\nnew file mode 100644\nindex 0000000..2222222\nBinary files /dev/null and b/image.bin differ\n";
    let mut store = DiffStore::default();
    store.upsert_turn_diff("turn-1", deleted);
    store.upsert_turn_diff("turn-2", same);
    assert_eq!(store.session_files(), Vec::new());
    assert!(!store.session_is_truncated());

    store.clear();
    store.upsert_turn_diff("turn-1", deleted);
    store.upsert_turn_diff("turn-2", different);
    assert_eq!(
        store.session_files(),
        vec![DiffFile::modified(
            "image.bin",
            "index 1111111..2222222 100644\nBinary files a/image.bin and b/image.bin differ",
            DiffStatus::Completed,
        )]
    );
    assert!(!store.session_is_truncated());
}

#[test]
fn binary_changes_without_object_ids_remain_incomplete() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/image.bin b/image.bin\nBinary files a/image.bin and b/image.bin differ\n",
    );

    assert!(store.session_is_truncated());
    assert_eq!(store.session_files().len(), 1);
    assert_eq!(store.session_files()[0].display_path(), "image.bin");
}

#[test]
fn incomplete_existing_file_metadata_is_not_treated_as_exact() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/script.sh b/script.sh\nold mode 100644\n",
    );

    assert!(store.session_is_truncated());
    assert_eq!(store.session_files().len(), 1);
    assert_eq!(store.session_files()[0].display_path(), "script.sh");
}

#[test]
fn empty_turn_aggregate_composes_item_changes_to_their_net_state() {
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "item-add",
        &[change("temporary.txt", PatchChangeKind::Add, "one\n")],
        PatchApplyStatus::Completed,
    );
    store.upsert_item(
        "turn-1",
        "item-delete",
        &[change("temporary.txt", PatchChangeKind::Delete, "one\n")],
        PatchApplyStatus::Completed,
    );
    store.upsert_turn_diff("turn-1", "");

    assert_eq!(store.session_files(), Vec::new());
}

#[test]
fn metadata_edits_remain_visible_when_text_edits_revert() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/script.sh b/script.sh\nold mode 100644\nnew mode 100755\n--- a/script.sh\n+++ b/script.sh\n@@ -1 +1 @@\n-old\n+new\n",
    );
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/script.sh b/script.sh\n--- a/script.sh\n+++ b/script.sh\n@@ -1 +1 @@\n-new\n+old\n",
    );

    assert_eq!(
        store.session_files(),
        vec![DiffFile::modified(
            "script.sh",
            "old mode 100644\nnew mode 100755",
            DiffStatus::Completed,
        )]
    );
}

#[test]
fn reversed_mode_edits_disappear_from_the_session() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/script.sh b/script.sh\nold mode 100644\nnew mode 100755\n",
    );
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/script.sh b/script.sh\nold mode 100755\nnew mode 100644\n",
    );

    assert_eq!(store.session_files(), Vec::new());
}

#[test]
fn reversed_binary_edits_disappear_from_the_session() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/image.bin b/image.bin\nindex 1111111..2222222 100644\nBinary files a/image.bin and b/image.bin differ\n",
    );
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/image.bin b/image.bin\nindex 2222222..1111111 100644\nBinary files a/image.bin and b/image.bin differ\n",
    );

    assert_eq!(store.session_files(), Vec::new());
}

#[test]
fn unknown_rowless_metadata_remains_visible_conservatively() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/module.txt b/module.txt\nunknown metadata one\n",
    );
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/module.txt b/module.txt\nunknown metadata two\n",
    );

    assert_eq!(
        store.session_files(),
        vec![DiffFile::modified("module.txt", "", DiffStatus::Completed,)]
    );
    assert!(store.session_is_truncated());
}

#[test]
fn inconsistent_metadata_transitions_remain_visible_as_incomplete() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/script.sh b/script.sh\nold mode 100644\nnew mode 100755\n",
    );
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/script.sh b/script.sh\nold mode 100600\nnew mode 100700\n",
    );

    assert_eq!(
        store.session_files(),
        vec![DiffFile::modified("script.sh", "", DiffStatus::Completed,)]
    );
    assert!(store.session_is_truncated());
}

#[test]
fn session_composition_uses_one_shared_memory_budget() {
    let content = std::iter::once("@@ -1,4200 +1,4200 @@".to_string())
        .chain((0..4_200).map(|index| format!(" line {index}")))
        .collect::<Vec<_>>()
        .join("\n");
    let first = DiffFile::modified("first.txt", &content, DiffStatus::Completed);
    let second = DiffFile::modified("second.txt", &content, DiffStatus::Completed);
    let composed =
        crate::app_shell::diff_session::compose_session_files(vec![first, second.clone(), second]);

    assert!(composed.truncated);
    assert_eq!(composed.files.len(), 2);
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
fn session_aggregation_marks_uncomposable_sparse_history_as_truncated() {
    let mut store = DiffStore::default();
    store.upsert_turn_diff(
        "turn-1",
        "diff --git a/sparse.txt b/sparse.txt\n--- a/sparse.txt\n+++ b/sparse.txt\n@@ -10000 +10000 @@\n-original\n+middle\n",
    );
    store.upsert_turn_diff(
        "turn-2",
        "diff --git a/sparse.txt b/sparse.txt\n--- a/sparse.txt\n+++ b/sparse.txt\n@@ -1 +1 @@\n-top\n+new top\n",
    );

    assert!(store.session_is_truncated());
    assert_eq!(store.session_files().len(), 1);
    assert_eq!(store.session_files()[0].display_path(), "sparse.txt");
}

#[test]
fn repeated_metadata_only_changes_remain_visible() {
    let mut store = DiffStore::default();
    let mode_change = "diff --git a/script.sh b/script.sh\nold mode 100644\nnew mode 100755\n";
    store.upsert_turn_diff("turn-1", mode_change);
    store.upsert_turn_diff("turn-2", mode_change);

    assert_eq!(
        store.session_files(),
        vec![DiffFile::modified(
            "script.sh",
            mode_change,
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
fn complete_turn_aggregate_supersedes_a_truncated_item_without_warning() {
    let oversized = change(
        "src/lib.rs",
        PatchChangeKind::Update { move_path: None },
        &std::iter::once("@@ -1,3000 +1,3000 @@".to_string())
            .chain((0..3_000).map(|index| format!(" context {index} {}", "x".repeat(120))))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let aggregate = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-before\n+after\n";
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "item-1",
        &[oversized],
        PatchApplyStatus::Completed,
    );
    assert!(store.item_is_truncated("item-1"));

    store.upsert_turn_diff("turn-1", aggregate);

    assert!(!store.session_is_truncated());
    assert_eq!(store.session_files(), parse_unified_diff(aggregate));
}

#[test]
fn omitted_item_files_keep_the_warning_when_an_aggregate_cannot_represent_renames() {
    let oversized = change(
        "src/lib.rs",
        PatchChangeKind::Update { move_path: None },
        &std::iter::once("@@ -1,3000 +1,3000 @@".to_string())
            .chain((0..3_000).map(|index| format!(" context {index} {}", "x".repeat(120))))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let rename = change(
        "old.txt",
        PatchChangeKind::Update {
            move_path: Some(PathBuf::from("new.txt")),
        },
        "@@\n same\n\nMoved to: new.txt",
    );
    let aggregate = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-before\n+after\n";
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "item-1",
        &[oversized, rename],
        PatchApplyStatus::Completed,
    );
    store.upsert_turn_diff("turn-1", aggregate);

    assert!(store.session_is_truncated());
    assert_eq!(store.session_files(), parse_unified_diff(aggregate));
}

#[test]
fn incomplete_item_data_uncovered_by_an_empty_aggregate_keeps_warning() {
    let oversized = change(
        "other.txt",
        PatchChangeKind::Update { move_path: None },
        &std::iter::once("@@ -1,3000 +1,3000 @@".to_string())
            .chain((0..3_000).map(|index| format!(" context {index} {}", "x".repeat(120))))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "item-1",
        &[oversized],
        PatchApplyStatus::Completed,
    );
    store.upsert_turn_diff("turn-1", "");

    assert!(store.session_is_truncated());
    assert_eq!(
        store
            .session_files()
            .iter()
            .map(DiffFile::display_path)
            .collect::<Vec<_>>(),
        vec!["other.txt"]
    );
}

#[test]
fn terminal_item_changes_remain_uncertain_until_a_nonempty_aggregate_arrives() {
    for status in [PatchApplyStatus::Failed, PatchApplyStatus::Declined] {
        let terminal = change(
            "src/lib.rs",
            PatchChangeKind::Update { move_path: None },
            "@@ -1 +1 @@\n-before\n+maybe\n",
        );
        let mut store = DiffStore::default();
        store.upsert_item("turn-1", "item-1", &[terminal], status);

        assert!(store.session_is_truncated());
        assert_eq!(store.session_files(), Vec::new());

        store.upsert_turn_diff("turn-1", "");
        assert!(store.session_is_truncated());

        let aggregate = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-before\n+actual\n";
        store.upsert_turn_diff("turn-1", aggregate);
        assert!(!store.session_is_truncated());
        assert_eq!(store.session_files(), parse_unified_diff(aggregate));
    }
}

#[test]
fn truncated_turn_aggregate_does_not_replace_a_complete_item_diff() {
    let item = change(
        "src/lib.rs",
        PatchChangeKind::Update { move_path: None },
        "@@ -1 +1 @@\n-before\n+complete item\n",
    );
    let mut aggregate = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-before\n+partial aggregate\n".to_string();
    aggregate.push_str(
        &(0..3_000)
            .map(|index| format!(" context {index} {}", "x".repeat(120)))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "item-1",
        std::slice::from_ref(&item),
        PatchApplyStatus::Completed,
    );
    store.upsert_turn_diff("turn-1", &aggregate);

    assert!(store.session_is_truncated());
    assert_eq!(
        store.session_files(),
        vec![DiffFile::from_change(&item, DiffStatus::Completed)]
    );
}

#[test]
fn complete_files_in_a_truncated_item_beat_partial_aggregate_files() {
    let complete = change(
        "src/lib.rs",
        PatchChangeKind::Update { move_path: None },
        "@@ -1 +1 @@\n-before\n+complete item\n",
    );
    let oversized = change(
        "other.txt",
        PatchChangeKind::Update { move_path: None },
        &std::iter::once("@@ -1,3000 +1,3000 @@".to_string())
            .chain((0..3_000).map(|index| format!(" context {index}")))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let mut aggregate = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-before\n+partial aggregate\n".to_string();
    aggregate.push_str(
        &(0..3_000)
            .map(|index| format!(" context {index}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let mut store = DiffStore::default();
    store.upsert_item(
        "turn-1",
        "item-1",
        &[complete.clone(), oversized],
        PatchApplyStatus::Completed,
    );
    store.upsert_turn_diff("turn-1", &aggregate);

    let retained = store
        .session_files()
        .into_iter()
        .find(|file| file.display_path() == "src/lib.rs")
        .expect("complete file should remain retained");
    assert_eq!(
        retained,
        DiffFile::from_change(&complete, DiffStatus::Completed)
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
fn live_file_replacement_preserves_identity_and_scroll() {
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
        (0, 9, 20, 20, 20)
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
fn live_file_replacement_preserves_selection_across_absolute_and_relative_paths() {
    let mut absolute = DiffFile::modified(
        "/workspace/project/src/lib.rs",
        "@@ -1 +1 @@\n-old\n+middle",
        DiffStatus::InProgress,
    );
    absolute.rebase_display_root(std::path::Path::new("/workspace/project"));
    let mut state = DiffViewState::new("Changes", None, vec![absolute]);
    state.set_scroll_max(20);
    state.scroll_down(/*amount*/ 7);
    let relative = DiffFile::modified(
        "src/lib.rs",
        "@@ -1 +1 @@\n-old\n+final",
        DiffStatus::Completed,
    );

    state.replace_files(vec![relative.clone()], DiffRetention::Complete);

    assert_eq!(state.selected_file(), Some(&relative));
    assert_eq!(state.scroll(), 7);
}

#[test]
fn horizontal_scroll_preserves_columns_across_wide_characters() {
    let mut state = DiffViewState::new("Changes", None, Vec::new());
    state.horizontal_scroll.set_max(1);
    state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT));

    assert_eq!(state.horizontal_scroll.visible_text("界x", 2), " x");
}
