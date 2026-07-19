use std::ffi::OsString;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use codex_utils_path::resolve_symlink_write_paths;

/// Holds the cross-process lock used to serialize user config writes.
///
/// Writers should acquire the lock before reading `config.toml` and retain it
/// until the replacement file has been committed.
pub struct ConfigFileLock {
    _file: File,
    write_path: PathBuf,
}

impl ConfigFileLock {
    pub fn acquire(config_path: &Path) -> io::Result<Self> {
        let write_path = resolve_symlink_write_paths(config_path)?.write_path;
        Self::acquire_for_write_path(&write_path)
    }

    pub fn acquire_for_write_path(write_path: &Path) -> io::Result<Self> {
        let parent = write_path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("config path has no parent: {}", write_path.display()),
            )
        })?;
        std::fs::create_dir_all(parent)?;

        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path(write_path)?)?;
        file.lock()?;

        Ok(Self {
            _file: file,
            write_path: write_path.to_path_buf(),
        })
    }

    pub fn ensure_protects(&self, write_path: &Path) -> io::Result<()> {
        if self.write_path == write_path {
            return Ok(());
        }

        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "config lock protects {}, not {}",
                self.write_path.display(),
                write_path.display()
            ),
        ))
    }
}

fn lock_path(write_path: &Path) -> io::Result<PathBuf> {
    let mut file_name = write_path.file_name().map(OsString::from).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("config path has no file name: {}", write_path.display()),
        )
    })?;
    file_name.push(".lock");
    Ok(write_path.with_file_name(file_name))
}

#[cfg(test)]
#[path = "file_lock_tests.rs"]
mod tests;
