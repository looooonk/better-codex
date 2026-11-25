use super::*;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[test]
fn concurrent_rules_are_retained_and_deduplicated() -> anyhow::Result<()> {
    let home = tempdir()?;
    let policy = path(home.path());
    let rules = ["first", "second", "first", "third"];
    std::thread::scope(|threads| {
        let policy = &policy;
        for rule in rules {
            threads.spawn(move || append_line(policy, rule).expect("append private rule"));
        }
    });
    let contents = fs::read_to_string(policy)?;
    let mut lines: Vec<_> = contents.lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, vec!["first", "second", "third"]);
    Ok(())
}

#[cfg(unix)]
#[test]
fn linked_approval_paths_preserve_standard_rules() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let home = tempdir()?;
    let policy = path(home.path());
    let standard = home.path().join("rules/default.rules");
    fs::create_dir(standard.parent().expect("rules directory"))?;
    fs::write(&standard, "original")?;

    symlink(&standard, &policy)?;
    assert!(append_line(&policy, "new").is_err());
    fs::remove_file(&policy)?;
    fs::hard_link(&standard, &policy)?;
    assert!(append_line(&policy, "new").is_err());
    fs::remove_file(&policy)?;

    let lock = policy.with_added_extension("lock");
    fs::remove_file(&lock)?;
    symlink(&standard, &lock)?;
    assert!(append_line(&policy, "new").is_err());
    assert_eq!(fs::read_to_string(&standard)?, "original");

    fs::remove_file(&lock)?;
    fs::remove_file(&standard)?;
    symlink(&policy, &standard)?;
    assert!(append_line(&policy, "new").is_err());
    assert!(!policy.exists());
    assert!(!lock.exists());
    Ok(())
}
