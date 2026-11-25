use super::*;

#[test]
fn preserves_shared_references_in_named_profiles_and_ignores_private_registrations()
-> io::Result<()> {
    let home = tempfile::tempdir()?;
    fs::write(
        home.path().join("better-codex.toml"),
        "[plugins.'only@fork']\nenabled = true\n",
    )?;
    fs::write(
        home.path().join("work.config.toml"),
        "[plugins.'shared@team']\nenabled = true\n",
    )?;
    assert!(referenced_by_codex(home.path(), "plugins", "shared@team"));
    assert!(!referenced_by_codex(home.path(), "plugins", "only@fork"));
    Ok(())
}
