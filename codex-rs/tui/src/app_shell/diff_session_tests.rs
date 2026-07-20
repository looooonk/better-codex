use super::*;
use crate::app_shell::diff_model::DiffFile;
use crate::app_shell::diff_model::DiffStatus;
use crate::app_shell::diff_model::parse_unified_diff;
use pretty_assertions::assert_eq;

fn compose(diffs: &[&str]) -> ComposedSessionFiles {
    compose_session_files(diffs.iter().flat_map(|diff| parse_unified_diff(diff)))
}

#[test]
fn zero_length_new_ranges_use_the_raw_hunk_position() {
    let composed = compose(&[
        "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -3,1 +2,0 @@\n-C\n",
        "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -2,0 +3,1 @@\n+C\n",
    ]);

    assert_eq!(composed.files, Vec::new());
    assert!(!composed.truncated);
}

#[test]
fn rename_delete_recreate_original_composes_to_the_net_content() {
    let rename =
        "diff --git a/a.txt b/b.txt\nsimilarity index 100%\nrename from a.txt\nrename to b.txt\n";
    let delete = "diff --git a/b.txt b/b.txt\ndeleted file mode 100644\n--- a/b.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-same\n";
    let recreate_same = "diff --git a/a.txt b/a.txt\nnew file mode 100644\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+same\n";
    let recreate_different = "diff --git a/a.txt b/a.txt\nnew file mode 100644\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+different\n";

    let reverted = compose(&[rename, delete, recreate_same]);
    assert_eq!(reverted.files, Vec::new());
    assert!(!reverted.truncated);

    let modified = compose(&[rename, delete, recreate_different]);
    assert_eq!(
        modified.files,
        vec![DiffFile::modified(
            "a.txt",
            "@@ -1,1 +1,1 @@\n-same\n+different",
            DiffStatus::Completed,
        )]
    );
    assert!(!modified.truncated);
}

#[test]
fn recreated_original_reconnects_after_the_renamed_path_is_deleted() {
    let rename =
        "diff --git a/a.txt b/b.txt\nsimilarity index 100%\nrename from a.txt\nrename to b.txt\n";
    let recreate_same = "diff --git a/a.txt b/a.txt\nnew file mode 100644\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+same\n";
    let recreate_different = "diff --git a/a.txt b/a.txt\nnew file mode 100644\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+different\n";
    let delete = "diff --git a/b.txt b/b.txt\ndeleted file mode 100644\n--- a/b.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-same\n";

    let reverted = compose(&[rename, recreate_same, delete]);
    assert_eq!(reverted.files, Vec::new());
    assert!(!reverted.truncated);

    let modified = compose(&[rename, recreate_different, delete]);
    assert_eq!(
        modified.files,
        vec![DiffFile::modified(
            "a.txt",
            "@@ -1,1 +1,1 @@\n-same\n+different",
            DiffStatus::Completed,
        )]
    );
    assert!(!modified.truncated);
}

#[test]
fn reconnected_recreation_can_be_replaced_by_a_later_add() {
    let composed = compose(&[
        "diff --git a/a.txt b/b.txt\nsimilarity index 100%\nrename from a.txt\nrename to b.txt\n",
        "diff --git a/a.txt b/a.txt\nnew file mode 100644\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+first\n",
        "diff --git a/b.txt b/b.txt\ndeleted file mode 100644\n--- a/b.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-old\n",
        "diff --git a/a.txt b/a.txt\nnew file mode 100644\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+latest\n",
    ]);

    assert_eq!(
        composed.files,
        vec![DiffFile::modified(
            "a.txt",
            "@@ -1,1 +1,1 @@\n-old\n+latest",
            DiffStatus::Completed,
        )]
    );
    assert!(!composed.truncated);
}

#[test]
fn rename_then_recreate_original_keeps_two_unambiguous_current_files() {
    let composed = compose(&[
        "diff --git a/a.txt b/b.txt\nsimilarity index 100%\nrename from a.txt\nrename to b.txt\n",
        "diff --git a/a.txt b/a.txt\nnew file mode 100644\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+new a\n",
    ]);

    assert_eq!(composed.files.len(), 2);
    assert_eq!(
        composed
            .files
            .iter()
            .map(DiffFile::display_path)
            .collect::<Vec<_>>(),
        vec!["a.txt -> b.txt", "a.txt"]
    );
    assert!(!composed.truncated);
}

#[test]
fn ambiguous_overwritten_destination_history_is_truncated() {
    let composed = compose_session_files([
        DiffFile::deleted("a.txt", "destination\n", DiffStatus::Completed),
        DiffFile::renamed("b.txt", "a.txt", "", DiffStatus::Completed),
    ]);

    assert_eq!(composed.files.len(), 2);
    assert!(composed.files[0].overlaps(&composed.files[1]));
    assert!(composed.truncated);
}

#[test]
fn duplicate_current_endpoints_are_truncated() {
    let composed = compose_session_files([
        DiffFile::renamed("a.txt", "shared.txt", "", DiffStatus::Completed),
        DiffFile::renamed("b.txt", "shared.txt", "", DiffStatus::Completed),
    ]);

    assert_eq!(composed.files.len(), 2);
    assert!(composed.truncated);
}

#[test]
fn possible_environment_namespace_transition_is_truncated() {
    let composed = compose_session_files([
        DiffFile::modified(
            "src/lib.rs",
            "@@ -1 +1 @@\n-before\n+middle",
            DiffStatus::Completed,
        ),
        DiffFile::modified(
            "local/src/lib.rs",
            "@@ -1 +1 @@\n-middle\n+after",
            DiffStatus::Completed,
        ),
    ]);

    assert_eq!(
        composed
            .files
            .iter()
            .map(DiffFile::display_path)
            .collect::<Vec<_>>(),
        vec!["src/lib.rs", "local/src/lib.rs"]
    );
    assert!(composed.truncated);
}

#[test]
fn namespace_transition_against_an_extinct_path_is_truncated() {
    let composed = compose_session_files([
        DiffFile::deleted("src/lib.rs", "before\n", DiffStatus::Completed),
        DiffFile::added("local/src/lib.rs", "after\n", DiffStatus::Completed),
    ]);

    assert_eq!(
        composed
            .files
            .iter()
            .map(DiffFile::display_path)
            .collect::<Vec<_>>(),
        vec!["src/lib.rs", "local/src/lib.rs"]
    );
    assert!(composed.truncated);
}

#[test]
fn exact_path_matches_still_check_other_namespace_variants() {
    let composed = compose_session_files([
        DiffFile::deleted("src/lib.rs", "before\n", DiffStatus::Completed),
        DiffFile::modified(
            "local/src/lib.rs",
            "@@ -1 +1 @@\n-old\n+middle",
            DiffStatus::Completed,
        ),
        DiffFile::modified(
            "local/src/lib.rs",
            "@@ -1 +1 @@\n-middle\n+after",
            DiffStatus::Completed,
        ),
    ]);

    assert_eq!(
        composed
            .files
            .iter()
            .map(DiffFile::display_path)
            .collect::<Vec<_>>(),
        vec!["src/lib.rs", "local/src/lib.rs"]
    );
    assert!(composed.truncated);
}

#[test]
fn pure_rename_cycles_are_truncated_because_contents_are_unknown() {
    let composed = compose(&[
        "diff --git a/a.txt b/tmp.txt\nsimilarity index 100%\nrename from a.txt\nrename to tmp.txt\n",
        "diff --git a/b.txt b/a.txt\nsimilarity index 100%\nrename from b.txt\nrename to a.txt\n",
        "diff --git a/tmp.txt b/b.txt\nsimilarity index 100%\nrename from tmp.txt\nrename to b.txt\n",
    ]);

    assert_eq!(
        composed
            .files
            .iter()
            .map(DiffFile::display_path)
            .collect::<Vec<_>>(),
        vec!["a.txt -> b.txt", "b.txt -> a.txt"]
    );
    assert!(composed.truncated);
}

#[test]
fn repeated_adds_replace_instead_of_append_content() {
    let composed = compose_session_files([
        DiffFile::added("new.txt", "one\n", DiffStatus::Completed),
        DiffFile::added("new.txt", "two\n", DiffStatus::Completed),
    ]);

    assert_eq!(
        composed.files,
        vec![DiffFile::added("new.txt", "two\n", DiffStatus::Completed,)]
    );
    assert!(!composed.truncated);
}

#[test]
fn repeated_add_after_delete_keeps_the_latest_recreated_content() {
    let composed = compose_session_files([
        DiffFile::deleted("existing.txt", "old\n", DiffStatus::Completed),
        DiffFile::added("existing.txt", "first\n", DiffStatus::Completed),
        DiffFile::added("existing.txt", "latest\n", DiffStatus::Completed),
    ]);

    assert_eq!(
        composed.files,
        vec![DiffFile::modified(
            "existing.txt",
            "@@ -1,1 +1,1 @@\n-old\n+latest",
            DiffStatus::Completed,
        )]
    );
    assert!(!composed.truncated);
}

#[test]
fn add_over_an_unknown_baseline_is_truncated() {
    let composed = compose_session_files([
        DiffFile::modified(
            "existing.txt",
            "@@ -1 +1 @@\n-before\n+middle",
            DiffStatus::Completed,
        ),
        DiffFile::added("existing.txt", "replacement\n", DiffStatus::Completed),
    ]);

    assert_eq!(
        composed.files,
        vec![DiffFile::added(
            "existing.txt",
            "replacement\n",
            DiffStatus::Completed,
        )]
    );
    assert!(composed.truncated);
}
