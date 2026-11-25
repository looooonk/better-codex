//! Better Codex shares Codex's home and reads its user settings until the first
//! edit. Each base file or named profile then has an independent sibling file.
//! Using the same parent preserves relative paths and hook discovery.

use codex_file_system::ExecutorFileSystem;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path::SymlinkWritePaths;
use codex_utils_path::resolve_symlink_write_paths;
use codex_utils_path_uri::PathUri;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

pub const PRIVATE_CONFIG_TOML_FILE: &str = "better-codex.toml";

struct SettingsFiles {
    inherited: PathBuf,
    private: PathBuf,
}

impl SettingsFiles {
    fn for_path(path: &Path) -> Option<Self> {
        let directory = path.parent()?;
        let name = path.file_name()?.to_str()?;
        let (inherited, private) = match name {
            crate::CONFIG_TOML_FILE | PRIVATE_CONFIG_TOML_FILE => (
                directory.join(crate::CONFIG_TOML_FILE),
                directory.join(PRIVATE_CONFIG_TOML_FILE),
            ),
            _ => {
                let profile: crate::ProfileV2Name = name
                    .strip_suffix(".config.toml")
                    .or_else(|| name.strip_suffix(".better-codex.toml"))?
                    .parse()
                    .ok()?;
                (
                    directory.join(format!("{profile}.config.toml")),
                    directory.join(format!("{profile}.better-codex.toml")),
                )
            }
        };
        Some(Self { inherited, private })
    }
    fn current(self) -> PathBuf {
        if self.private.exists() {
            self.private
        } else {
            self.inherited
        }
    }

    fn for_owned_path(codex_home: &Path, path: &Path) -> Option<Self> {
        let directory = path.parent()?;
        let files = Self::for_path(path)?;
        if path == files.private || directory == codex_home || directory.ends_with(".codex") {
            return Some(files);
        }
        let directory = directory.canonicalize().ok()?;
        (directory == codex_home || directory == codex_home.canonicalize().ok()?).then_some(files)
    }
}

/// Resolve the destination while the caller knows which local home owns this layer.
pub fn write_path(codex_home: &Path, path: &Path) -> PathBuf {
    SettingsFiles::for_owned_path(codex_home, path)
        .map_or_else(|| path.to_owned(), |files| files.private)
}

/// The current readable layer. A missing private file inherits the original.
pub fn read_path(path: &Path) -> PathBuf {
    SettingsFiles::for_path(path).map_or_else(|| path.to_owned(), SettingsFiles::current)
}

/// Reread a recorded layer after a first edit may have created its private sibling.
pub fn read_user_layer(codex_home: &Path, path: &Path) -> PathBuf {
    SettingsFiles::for_owned_path(codex_home, path)
        .map_or_else(|| path.to_owned(), SettingsFiles::current)
}

/// Resolve layers against the executor that will actually read them.
pub(crate) async fn executor_read_path(
    fs: &dyn ExecutorFileSystem,
    path: AbsolutePathBuf,
) -> io::Result<AbsolutePathBuf> {
    let Some(files) = SettingsFiles::for_path(path.as_path()) else {
        return Ok(path);
    };
    let private = AbsolutePathBuf::from_absolute_path(files.private)?;
    let uri = PathUri::from_abs_path(&private);
    match fs
        .get_metadata(&uri, Default::default(), /*sandbox*/ None)
        .await
    {
        Ok(_) => Ok(private),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            AbsolutePathBuf::from_absolute_path(files.inherited)
        }
        // Some read-only executors can read configuration but cannot stat it.
        // Probe the same executor and fall back only when the file is absent.
        Err(_) => match fs.read_file_text(&uri, Default::default(), None).await {
            Ok(_) => Ok(private),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                AbsolutePathBuf::from_absolute_path(files.inherited)
            }
            Err(err) => Err(err),
        },
    }
}

/// A read-modify-write operation holds the fork's file lock through publication.
/// Replace the private directory entry atomically, even when it is a symlink.
pub struct UserConfigEdit {
    pub read_path: Option<PathBuf>,
    pub write_path: PathBuf,
    _lock: Option<fs::File>,
}

impl UserConfigEdit {
    pub fn open(codex_home: &Path, path: &Path) -> io::Result<Self> {
        let files = if let Some(files) = SettingsFiles::for_owned_path(codex_home, path) {
            files
        } else {
            // An explicitly supplied external file retains its write-through
            // semantics unless the symlink resolves into a Codex settings layer.
            let SymlinkWritePaths {
                read_path,
                write_path,
            } = resolve_symlink_write_paths(path)?;
            match SettingsFiles::for_owned_path(codex_home, &write_path) {
                Some(files) => files,
                None => {
                    ensure_not_codex_symlink_target(codex_home, &write_path)?;
                    return Ok(Self {
                        read_path,
                        write_path,
                        _lock: None,
                    });
                }
            }
        };
        let parent = files.private.parent().expect("private file has a parent");
        fs::create_dir_all(parent)?;
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
        }
        let lock = options.open(files.private.with_added_extension("lock"))?;
        lock.lock()?;
        ensure_not_codex_symlink_target(parent, &files.private)?;
        if parent != codex_home {
            ensure_not_codex_symlink_target(codex_home, &files.private)?;
        }
        let read_path = if files.private.try_exists()? {
            files.private.clone()
        } else {
            files.inherited
        };
        Ok(Self {
            read_path: Some(read_path),
            write_path: files.private,
            _lock: Some(lock),
        })
    }

    pub fn write(&self, contents: &str) -> io::Result<()> {
        codex_utils_path::write_atomically(&self.write_path, contents)
    }
}

// Check other profile names as well. A dotfile passed by its actual filename
// can also be the target of a symlink from the standard Codex home.
fn ensure_not_codex_symlink_target(directory: &Path, destination: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_symlink() {
            continue;
        }
        let source = entry.path();
        if SettingsFiles::for_path(&source).is_some_and(|settings| settings.inherited == source)
            && codex_utils_path::paths_match_after_normalization(
                resolve_symlink_write_paths(&source)?.write_path,
                resolve_symlink_write_paths(destination)?.write_path,
            )
        {
            return Err(io::Error::other(format!(
                "Codex settings at {} use {}; select the logical settings file or separate the files before saving",
                source.display(),
                destination.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "user_config_file_tests.rs"]
mod tests;
