use pretty_assertions::assert_eq;

use super::ExecutableIdentity;
use super::executable_identity;
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

#[tokio::test]
async fn executable_identity_uses_path_and_binary_contents() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let executable = directory.path().join("codex");
    // Span multiple reads, including a partial final buffer, and preserve the
    // digest stored by older clients that hashed the complete file in memory.
    let mut bytes: Vec<u8> = (0..200_003).map(|index| (index % 251) as u8).collect();
    for contents in [&bytes[..], &[][..]] {
        std::fs::write(&executable, contents).expect("write executable");
        assert_eq!(
            executable_identity(&executable).await.expect("identity"),
            ExecutableIdentity {
                digest: *blake3::hash(contents).as_bytes(),
                path_digest: Some(super::path_digest(
                    &std::fs::canonicalize(&executable).expect("canonical executable"),
                )),
            }
        );
    }
    let copy = directory.path().join("codex-copy");
    std::fs::copy(&executable, &copy).expect("copy executable");
    let identity = executable_identity(&executable).await.expect("identity");
    let copy_identity = executable_identity(&copy).await.expect("copy identity");
    assert_ne!(identity, copy_identity);
    assert!(identity.same_contents(&copy_identity));
    std::fs::write(&executable, &bytes).expect("write executable");
    let old = executable_identity(&executable).await.expect("identity");
    bytes[100_000] ^= 1;
    std::fs::write(&executable, bytes).expect("replace executable");
    assert_ne!(
        executable_identity(&executable)
            .await
            .expect("new identity"),
        old
    );
}

#[cfg(unix)]
#[test]
fn latest_channel_prerelease_requires_matching_selection_and_binary() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("packages/app-server-daemon");
    let name = "0.1.0-alpha.15-aarch64-apple-darwin";
    let release = root.join("releases").join(name);
    std::fs::create_dir_all(release.join("bin")).unwrap();
    let binary = release.join("bin/codex");
    std::fs::write(&binary, b"binary").unwrap();
    std::os::unix::fs::symlink(&release, root.join("current")).unwrap();
    assert!(!super::is_stable_standalone_release(home.path(), &binary));
    std::fs::write(root.join("auto-update-version"), name).unwrap();
    assert!(super::is_stable_standalone_release(home.path(), &binary));
    let other = home.path().join("other");
    std::fs::write(&other, b"binary").unwrap();
    assert!(!super::is_stable_standalone_release(home.path(), &other));
    std::fs::write(root.join("auto-update-version"), "another-release").unwrap();
    assert!(!super::is_stable_standalone_release(home.path(), &binary));
}
