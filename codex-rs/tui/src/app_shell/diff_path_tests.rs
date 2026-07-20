use super::*;
use pretty_assertions::assert_eq;

#[test]
fn unquoted_header_paths_keep_spaces_and_drop_tab_timestamps() {
    assert_eq!(
        header_path(
            &["--- a/src/file with spaces.rs\t2026-07-20 12:34:56 +0900"],
            "--- ",
        ),
        Some(Some("src/file with spaces.rs".to_string())),
    );
    assert_eq!(
        header_path(&["+++ b/src/trailing spaces.rs  "], "+++ "),
        Some(Some("src/trailing spaces.rs  ".to_string())),
    );
    assert_eq!(
        header_path(
            &["+++ b/src/spaces before timestamp.rs  \t2026-07-20 12:34:56 +0900"],
            "+++ ",
        ),
        Some(Some("src/spaces before timestamp.rs  ".to_string())),
    );
    assert_eq!(
        header_path(&["+++ b/src/tab\tin name.rs"], "+++ "),
        Some(Some("src/tab\tin name.rs".to_string())),
    );
    assert_eq!(
        header_path(&["+++ b/src/trailing-tab.rs\t"], "+++ "),
        Some(Some("src/trailing-tab.rs".to_string())),
    );
    assert_eq!(
        header_path(&["+++ b/src/date-like\t2026-07-20-not-a-timestamp"], "+++ "),
        Some(Some(
            "src/date-like\t2026-07-20-not-a-timestamp".to_string()
        )),
    );
}

#[test]
fn quoted_header_paths_still_decode_git_c_escapes() {
    assert_eq!(
        header_path(&[r#"--- "a/quote\"-tab\t-caf\303\251.txt"	stamp"#], "--- ",),
        Some(Some("quote\"-tab\t-café.txt".to_string())),
    );
}

#[test]
fn metadata_paths_preserve_leading_and_trailing_spaces() {
    assert_eq!(
        metadata_path(&["rename from  leading and trailing  "], "rename from "),
        Some(" leading and trailing  ".to_string()),
    );
}

#[test]
fn unquoted_git_paths_allow_spaces_and_preserve_trailing_whitespace() {
    assert_eq!(
        parse_git_paths("a/src/old file.rs b/src/new file.rs  "),
        Some((
            "src/old file.rs".to_string(),
            "src/new file.rs  ".to_string(),
        )),
    );
    assert_eq!(
        parse_git_paths("a/src/old  b/src/new"),
        Some(("src/old ".to_string(), "src/new".to_string())),
    );
}

#[test]
fn unquoted_same_path_uses_the_matching_a_and_b_boundaries() {
    assert_eq!(
        parse_git_paths("a/dir b/name.txt b/dir b/name.txt"),
        Some(("dir b/name.txt".to_string(), "dir b/name.txt".to_string())),
    );
}

#[test]
fn mixed_and_fully_quoted_git_paths_keep_c_path_decoding() {
    assert_eq!(
        parse_git_paths(r#""a/old\tname" b/new name"#),
        Some(("old\tname".to_string(), "new name".to_string())),
    );
    assert_eq!(
        parse_git_paths(r#"a/old name "b/new\tname""#),
        Some(("old name".to_string(), "new\tname".to_string())),
    );
    assert_eq!(
        parse_git_paths(r#""a/old\tname" "b/new\nname""#),
        Some(("old\tname".to_string(), "new\nname".to_string())),
    );
}

#[test]
fn diff_paths_keep_full_identity_while_rebasing_display_labels() {
    let mut absolute = DiffPath::new("/workspace/project/src/../src/lib.rs");
    absolute.resolve(
        Path::new("/workspace/project"),
        Path::new("/workspace/project"),
    );
    let relative = DiffPath::new("./src/lib.rs");

    assert_eq!(absolute.label(), "src/lib.rs");
    assert!(absolute.equivalent(&relative));
    assert_eq!(relative, DiffPath::new("src/lib.rs"));
}

#[test]
fn absolute_paths_only_match_their_rebased_git_paths() {
    let mut absolute = DiffPath::new("/workspace/project/sub/src/lib.rs");
    absolute.resolve(
        Path::new("/workspace/project"),
        Path::new("/workspace/project"),
    );

    assert!(absolute.equivalent(&DiffPath::new("sub/src/lib.rs")));
    assert!(!DiffPath::new("left/src/lib.rs").equivalent(&DiffPath::new("right/src/lib.rs")));

    let mut sibling = DiffPath::new("/workspace/project/other/src/lib.rs");
    sibling.resolve(
        Path::new("/workspace/project"),
        Path::new("/workspace/project"),
    );
    assert!(!sibling.equivalent(&DiffPath::new("src/lib.rs")));

    let mut outside_root = DiffPath::new("/tmp/other/src/lib.rs");
    outside_root.resolve(
        Path::new("/workspace/project"),
        Path::new("/workspace/project"),
    );
    assert!(!outside_root.equivalent(&DiffPath::new("src/lib.rs")));
}

#[test]
fn rebased_relative_paths_keep_their_original_workspace_identity() {
    let mut original = DiffPath::new("src/lib.rs");
    original.resolve(Path::new("/workspace/first"), Path::new("/workspace/first"));
    original.rebase(Path::new("/workspace/second"));
    let mut current = DiffPath::new("src/lib.rs");
    current.resolve(
        Path::new("/workspace/second"),
        Path::new("/workspace/second"),
    );

    assert_eq!(original.label(), "/workspace/first/src/lib.rs");
    assert_eq!(current.label(), "src/lib.rs");
    assert!(!original.equivalent(&current));
}

#[test]
fn display_bounding_does_not_collapse_distinct_path_identities() {
    let edge = "x".repeat(MAX_DIFF_PATH_BYTES);
    let first = DiffPath::new(format!("{edge}/first/{edge}"));
    let second = DiffPath::new(format!("{edge}/second/{edge}"));

    assert_ne!(bounded_path(first.label()), bounded_path(second.label()));
    assert_eq!(bounded_path(first.label()).len(), MAX_DIFF_PATH_BYTES);
    assert!(!first.equivalent(&second));
}

#[test]
fn retained_path_identities_have_a_hard_size_cap() {
    let first = DiffPath::new(format!("/workspace/{}/first.rs", "x".repeat(20_000)));
    let second = DiffPath::new(format!("/workspace/{}/second.rs", "x".repeat(20_000)));

    assert!(first.retained_text_bytes() <= MAX_DIFF_IDENTITY_BYTES * 2);
    assert!(second.retained_text_bytes() <= MAX_DIFF_IDENTITY_BYTES * 2);
    assert!(!first.equivalent(&second));
}
