use super::*;
use codex_utils_path::write_atomically;
use pretty_assertions::assert_eq;

#[test]
fn first_write_forks_the_latest_base_or_profile_and_subsequent_edits_use_the_fork() -> io::Result<()>
{
    let home = tempfile::tempdir()?;
    for (source, target) in [
        ("config.toml", "better-codex.toml"),
        ("work.config.toml", "work.better-codex.toml"),
        ("config.config.toml", "config.better-codex.toml"),
    ] {
        let shared = home.path().join(source);
        let private = home.path().join(target);
        fs::write(&shared, "original")?;
        let edit = UserConfigEdit::open(home.path(), &shared)?;
        assert_eq!(edit.read_path.as_deref(), Some(shared.as_path()));
        assert_eq!(&edit.write_path, &private);
        write_atomically(&edit.write_path, "customized")?;
        drop(edit);
        for candidate in [&shared, &private] {
            let edit = UserConfigEdit::open(home.path(), candidate)?;
            assert_eq!(edit.read_path.as_deref(), Some(private.as_path()));
            assert_eq!(edit.write_path, private);
        }
        assert_eq!(fs::read_to_string(shared)?, "original");
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn private_symlink_is_replaced_without_following_it_on_write() -> io::Result<()> {
    let home = tempfile::tempdir()?;
    let shared = home.path().join("config.toml");
    let private = home.path().join("better-codex.toml");
    fs::write(&shared, "original")?;
    std::os::unix::fs::symlink(&shared, &private)?;
    let edit = UserConfigEdit::open(home.path(), &private)?;
    write_atomically(&edit.write_path, "customized")?;
    assert_eq!(fs::read_to_string(shared)?, "original");
    assert_eq!(fs::read_to_string(private)?, "customized");
    Ok(())
}

#[cfg(unix)]
#[test]
fn refuses_a_reverse_symlink_or_symlinked_lock_without_changing_codex_settings() -> io::Result<()> {
    let home = tempfile::tempdir()?;
    let official = home.path().join("config.toml");
    let private = home.path().join("better-codex.toml");
    fs::write(&private, "original")?;
    std::os::unix::fs::symlink(&private, &official)?;
    assert!(UserConfigEdit::open(home.path(), &official).is_err());
    assert_eq!(fs::read_to_string(&official)?, "original");

    fs::remove_file(&official)?;
    let other_profile = home.path().join("work.config.toml");
    std::os::unix::fs::symlink(&private, &other_profile)?;
    assert!(UserConfigEdit::open(home.path(), &official).is_err());
    assert_eq!(fs::read_to_string(&other_profile)?, "original");
    fs::remove_file(other_profile)?;

    fs::remove_file(private.with_added_extension("lock"))?;
    fs::write(&official, "original")?;
    std::os::unix::fs::symlink(&official, private.with_added_extension("lock"))?;
    assert!(UserConfigEdit::open(home.path(), &official).is_err());
    assert_eq!(fs::read_to_string(&official)?, "original");
    Ok(())
}

#[test]
fn explicit_files_outside_the_supplied_home_keep_their_name() -> io::Result<()> {
    let home = tempfile::tempdir()?;
    let external = tempfile::tempdir()?;
    let path = external.path().join("config.toml");
    let edit = UserConfigEdit::open(home.path(), &path)?;
    assert_eq!(edit.write_path, path);
    Ok(())
}

#[cfg(unix)]
#[test]
fn explicitly_supplying_a_shared_dotfile_target_preserves_it() -> io::Result<()> {
    let home = tempfile::tempdir()?;
    let dotfiles = tempfile::tempdir()?;
    let file = dotfiles.path().join("custom-codex.toml");
    fs::write(&file, "original")?;
    std::os::unix::fs::symlink(&file, home.path().join("work.config.toml"))?;
    assert!(UserConfigEdit::open(home.path(), &file).is_err());
    assert_eq!(fs::read_to_string(file)?, "original");
    Ok(())
}
