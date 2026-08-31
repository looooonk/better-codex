use super::*;
use std::io::Cursor;
use tar::EntryType;
use tar::Header;
use tempfile::tempdir;

#[derive(Clone, Copy)]
enum TestEntryType {
    File,
    Directory,
}

fn unpack_test_entries(entries: &[(&str, TestEntryType)]) -> Result<(), PluginBundleUnpackError> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, entry_type) in entries {
        let mut header = Header::new_gnu();
        header.set_path(path).expect("valid test archive path");
        header.set_size(0);
        header.set_mode(0o644);
        header.set_entry_type(match entry_type {
            TestEntryType::File => EntryType::Regular,
            TestEntryType::Directory => EntryType::Directory,
        });
        header.set_cksum();
        builder
            .append(&header, std::io::empty())
            .expect("append test archive entry");
    }
    let bytes = builder.into_inner().expect("finish test archive");
    let mut archive = Archive::new(Cursor::new(bytes));
    let destination = tempdir().expect("destination tempdir");
    unpack_plugin_bundle_tar(&mut archive, destination.path(), 1024)
}

#[test]
fn portable_root_manifest_can_be_packed_and_unpacked() {
    let source = tempdir().expect("source tempdir");
    fs::write(
        source.path().join("plugin.json"),
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"portable"}"#,
    )
    .expect("write portable manifest");
    fs::create_dir_all(source.path().join("skills/demo")).expect("create skill directory");
    fs::write(source.path().join("skills/demo/SKILL.md"), "# Demo\n").expect("write skill");

    let archive =
        pack_plugin_bundle_tar_gz(source.path(), 1024 * 1024).expect("pack portable plugin");
    let destination = tempdir().expect("destination tempdir");
    unpack_plugin_bundle_tar_gz(&archive, destination.path(), 1024 * 1024)
        .expect("unpack portable plugin");

    assert!(destination.path().join("plugin.json").is_file());
    assert!(destination.path().join("skills/demo/SKILL.md").is_file());
}

#[test]
fn invalid_portable_manifest_is_rejected_before_packing() {
    let source = tempdir().expect("source tempdir");
    fs::write(
        source.path().join("plugin.json"),
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"UPPER"}"#,
    )
    .expect("write invalid portable manifest");

    let error = pack_plugin_bundle_tar_gz(source.path(), 1024 * 1024)
        .expect_err("invalid Agent Plugin manifest should fail");

    assert!(matches!(
        error,
        PluginBundlePackError::InvalidPluginPath { .. }
    ));
}

#[cfg(unix)]
#[test]
fn packing_skips_nested_symlinks_and_special_entries() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = tempdir().expect("source tempdir");
    fs::write(
        source.path().join("plugin.json"),
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"portable"}"#,
    )
    .expect("write portable manifest");
    let nested = source.path().join("skills/demo");
    fs::create_dir_all(&nested).expect("create nested directory");
    let outside_root = tempdir().expect("outside tempdir");
    let outside = outside_root.path().join("outside-SKILL.md");
    fs::write(&outside, "# Outside\n").expect("write symlink target");
    std::os::unix::fs::symlink(&outside, nested.join("SKILL.md")).expect("create symlink");
    let fifo_path = nested.join("events.pipe");
    let fifo_path = CString::new(fifo_path.as_os_str().as_bytes()).expect("path without nul");
    // SAFETY: `fifo_path` is a valid, nul-terminated path and `mkfifo` does not retain it.
    assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o644) }, 0);

    let archive =
        pack_plugin_bundle_tar_gz(source.path(), 1024 * 1024).expect("pack portable plugin");
    let destination = tempdir().expect("destination tempdir");
    unpack_plugin_bundle_tar_gz(&archive, destination.path(), 1024 * 1024)
        .expect("unpack portable plugin");

    assert!(destination.path().join("plugin.json").is_file());
    assert!(!destination.path().join("skills/demo/SKILL.md").exists());
    assert!(!destination.path().join("skills/demo/events.pipe").exists());
}

#[cfg(unix)]
#[test]
fn packing_rejects_symlinked_root_manifest() {
    let source = tempdir().expect("source tempdir");
    let manifest = source.path().join("manifest-target.json");
    fs::write(
        &manifest,
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"portable"}"#,
    )
    .expect("write portable manifest target");
    let plugin_root = source.path().join("plugin");
    fs::create_dir_all(&plugin_root).expect("create plugin root");
    std::os::unix::fs::symlink(&manifest, plugin_root.join("plugin.json"))
        .expect("create root manifest symlink");

    assert!(matches!(
        pack_plugin_bundle_tar_gz(&plugin_root, 1024 * 1024),
        Err(PluginBundlePackError::InvalidPluginPath { .. })
    ));
}

#[test]
fn duplicate_normalized_archive_paths_are_rejected() {
    for entries in [
        [
            ("duplicate", TestEntryType::File),
            ("duplicate", TestEntryType::File),
        ],
        [
            ("collision", TestEntryType::Directory),
            ("./collision", TestEntryType::File),
        ],
    ] {
        let error = unpack_test_entries(&entries).expect_err("duplicate path should fail");
        assert!(matches!(
            error,
            PluginBundleUnpackError::InvalidBundle(message)
                if message.contains("duplicate path")
        ));
    }
}

#[test]
fn archive_entry_count_is_bounded_before_extraction() {
    enforce_packed_archive_entry_count(MAX_PLUGIN_BUNDLE_ENTRIES, Path::new("allowed"))
        .expect("pack entry limit should be allowed");
    assert!(matches!(
        enforce_packed_archive_entry_count(
            MAX_PLUGIN_BUNDLE_ENTRIES + 1,
            Path::new("too-many")
        ),
        Err(PluginBundlePackError::InvalidPluginPath { reason, .. })
            if reason.contains("maximum archive entry count")
    ));
    enforce_archive_entry_count(MAX_PLUGIN_BUNDLE_ENTRIES).expect("entry limit should be allowed");
    assert!(matches!(
        enforce_archive_entry_count(MAX_PLUGIN_BUNDLE_ENTRIES + 1),
        Err(PluginBundleUnpackError::InvalidBundle(message))
            if message.contains("maximum entry count")
    ));
}

#[test]
fn archive_path_depth_is_bounded() {
    let destination = Path::new("destination");
    let allowed =
        std::iter::repeat_n("component", MAX_PLUGIN_BUNDLE_PATH_COMPONENTS).collect::<PathBuf>();
    enforce_packed_archive_path(&allowed).expect("pack path depth limit should be allowed");
    assert_eq!(
        checked_tar_output_path(destination, &allowed).expect("path depth limit should be allowed"),
        destination.join(&allowed)
    );

    let too_deep = allowed.join("component");
    assert!(matches!(
        enforce_packed_archive_path(&too_deep),
        Err(PluginBundlePackError::InvalidPluginPath { reason, .. })
            if reason.contains("maximum depth")
    ));
    assert!(matches!(
        checked_tar_output_path(destination, &too_deep),
        Err(PluginBundleUnpackError::InvalidBundle(message))
            if message.contains("maximum path depth")
    ));
}
