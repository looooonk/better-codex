use pretty_assertions::assert_eq;

use super::executable_identity_from_bytes;
use super::managed_codex_bin_with_install_root;
use super::parse_codex_version;

#[test]
fn parses_codex_cli_version_output() {
    assert_eq!(
        parse_codex_version("codex 1.2.3\n").expect("version"),
        "1.2.3"
    );
}

#[test]
fn rejects_malformed_codex_cli_version_output() {
    assert!(parse_codex_version("codex\n").is_err());
}

#[test]
fn executable_identity_uses_binary_contents() {
    let old = executable_identity_from_bytes(b"old");
    let same = executable_identity_from_bytes(b"old");
    let new = executable_identity_from_bytes(b"new");

    assert_eq!(old, same);
    assert_ne!(old, new);
}

#[test]
fn managed_binary_uses_better_codex_install_root() {
    assert_eq!(
        managed_codex_bin_with_install_root(
            std::path::Path::new("/codex-home"),
            Some(std::path::Path::new("/better-codex")),
        ),
        std::path::PathBuf::from("/better-codex/current/bin/codex")
    );
}
