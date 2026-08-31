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

fn unpack_test_entries(
    entries: &[(&str, TestEntryType)],
) -> Result<(), PluginBundleUnpackError> {
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
    let allowed = std::iter::repeat_n("component", MAX_PLUGIN_BUNDLE_PATH_COMPONENTS)
        .collect::<PathBuf>();
    assert_eq!(
        checked_tar_output_path(destination, &allowed).expect("path depth limit should be allowed"),
        destination.join(&allowed)
    );

    let too_deep = allowed.join("component");
    assert!(matches!(
        checked_tar_output_path(destination, &too_deep),
        Err(PluginBundleUnpackError::InvalidBundle(message))
            if message.contains("maximum path depth")
    ));
}
