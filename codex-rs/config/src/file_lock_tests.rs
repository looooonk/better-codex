use tempfile::TempDir;

use super::*;

#[test]
fn lock_creates_sidecar_for_config() {
    let temp_dir = TempDir::new().unwrap();
    let config = temp_dir.path().join("config.toml");

    let lock = ConfigFileLock::acquire(&config).unwrap();

    lock.ensure_protects(&config).unwrap();
    assert!(temp_dir.path().join("config.toml.lock").exists());
}

#[cfg(unix)]
#[test]
fn lock_follows_config_symlink_target() {
    let temp_dir = TempDir::new().unwrap();
    let target = temp_dir.path().join("target.toml");
    let config = temp_dir.path().join("config.toml");
    std::fs::write(&target, "").unwrap();
    create_symlink(&target, &config);

    let lock = ConfigFileLock::acquire(&config).unwrap();

    lock.ensure_protects(&target).unwrap();
    assert!(temp_dir.path().join("target.toml.lock").exists());
}

#[cfg(unix)]
fn create_symlink(target: &std::path::Path, link: &std::path::Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}
