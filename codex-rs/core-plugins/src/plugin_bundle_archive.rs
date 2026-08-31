use crate::manifest::load_plugin_manifest;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use tar::Archive;

const MAX_PLUGIN_BUNDLE_ENTRIES: usize = 10_000;
const MAX_PLUGIN_BUNDLE_PATH_COMPONENTS: usize = 64;

#[derive(Debug, thiserror::Error)]
pub(crate) enum PluginBundlePackError {
    #[error("invalid plugin path `{path}`: {reason}")]
    InvalidPluginPath { path: PathBuf, reason: String },

    #[error("plugin archive would be {bytes} bytes, exceeding maximum size of {max_bytes} bytes")]
    ArchiveTooLarge { bytes: usize, max_bytes: usize },

    #[error("failed to archive plugin bundle: {source}")]
    Io {
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PluginBundleUnpackError {
    #[error(
        "plugin bundle extracted size would be {bytes} bytes, exceeding maximum total size of {max_bytes} bytes"
    )]
    ExtractedBundleTooLarge { bytes: u64, max_bytes: u64 },

    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: io::Error,
    },

    #[error("{0}")]
    InvalidBundle(String),
}

impl PluginBundleUnpackError {
    fn io(context: &'static str, source: io::Error) -> Self {
        Self::Io { context, source }
    }
}

pub(crate) fn pack_plugin_bundle_tar_gz(
    plugin_path: &Path,
    max_bytes: usize,
) -> Result<Vec<u8>, PluginBundlePackError> {
    if !plugin_path.is_dir() {
        return Err(PluginBundlePackError::InvalidPluginPath {
            path: plugin_path.to_path_buf(),
            reason: "expected a plugin directory".to_string(),
        });
    }
    if !plugin_path.join(".codex-plugin/plugin.json").is_file()
        && load_plugin_manifest(plugin_path).is_none()
    {
        return Err(PluginBundlePackError::InvalidPluginPath {
            path: plugin_path.to_path_buf(),
            reason: "missing .codex-plugin/plugin.json or valid Agent Plugin manifest".to_string(),
        });
    }

    let encoder = GzEncoder::new(SizeLimitedBuffer::new(max_bytes), Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let mut entry_count = 0;
    append_plugin_tree(&mut archive, plugin_path, plugin_path, &mut entry_count)?;
    let encoder = archive.into_inner().map_err(archive_io_error)?;
    encoder
        .finish()
        .map(SizeLimitedBuffer::into_inner)
        .map_err(archive_io_error)
}

fn append_plugin_tree<W: Write>(
    archive: &mut tar::Builder<W>,
    plugin_root: &Path,
    current: &Path,
    entry_count: &mut usize,
) -> Result<(), PluginBundlePackError> {
    let mut entries = fs::read_dir(current)
        .map_err(|source| PluginBundlePackError::Io { source })?
        .collect::<Result<Vec<_>, io::Error>>()
        .map_err(|source| PluginBundlePackError::Io { source })?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| PluginBundlePackError::Io { source })?;
        if !file_type.is_dir() && !file_type.is_file() {
            continue;
        }
        let relative_path = path.strip_prefix(plugin_root).map_err(|err| {
            PluginBundlePackError::InvalidPluginPath {
                path: path.clone(),
                reason: format!("failed to compute plugin archive path: {err}"),
            }
        })?;
        enforce_packed_archive_path(relative_path)?;
        *entry_count = entry_count.saturating_add(1);
        enforce_packed_archive_entry_count(*entry_count, relative_path)?;
        if file_type.is_dir() {
            archive
                .append_dir(relative_path, &path)
                .map_err(archive_io_error)?;
            append_plugin_tree(archive, plugin_root, &path, entry_count)?;
        } else {
            archive
                .append_path_with_name(&path, relative_path)
                .map_err(archive_io_error)?;
        }
    }
    Ok(())
}

fn enforce_packed_archive_entry_count(
    entry_count: usize,
    path: &Path,
) -> Result<(), PluginBundlePackError> {
    if entry_count > MAX_PLUGIN_BUNDLE_ENTRIES {
        return Err(PluginBundlePackError::InvalidPluginPath {
            path: path.to_path_buf(),
            reason: format!(
                "plugin tree exceeds maximum archive entry count of {MAX_PLUGIN_BUNDLE_ENTRIES}"
            ),
        });
    }
    Ok(())
}

fn enforce_packed_archive_path(path: &Path) -> Result<(), PluginBundlePackError> {
    let component_count = path
        .components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count();
    if component_count > MAX_PLUGIN_BUNDLE_PATH_COMPONENTS {
        return Err(PluginBundlePackError::InvalidPluginPath {
            path: path.to_path_buf(),
            reason: format!(
                "plugin archive path exceeds maximum depth of {MAX_PLUGIN_BUNDLE_PATH_COMPONENTS}"
            ),
        });
    }
    Ok(())
}

fn archive_io_error(source: io::Error) -> PluginBundlePackError {
    if let Some(limit) = source
        .get_ref()
        .and_then(|err| err.downcast_ref::<ArchiveSizeLimitExceeded>())
    {
        return PluginBundlePackError::ArchiveTooLarge {
            bytes: limit.bytes,
            max_bytes: limit.max_bytes,
        };
    }

    PluginBundlePackError::Io { source }
}

pub(crate) fn unpack_plugin_bundle_tar_gz(
    bytes: &[u8],
    destination: &Path,
    max_total_bytes: u64,
) -> Result<(), PluginBundleUnpackError> {
    fs::create_dir_all(destination).map_err(|source| {
        PluginBundleUnpackError::io(
            "failed to create plugin bundle extraction directory",
            source,
        )
    })?;

    let archive = GzDecoder::new(std::io::Cursor::new(bytes));
    let mut archive = Archive::new(archive);
    unpack_plugin_bundle_tar(&mut archive, destination, max_total_bytes)
}

fn unpack_plugin_bundle_tar<R: Read>(
    archive: &mut Archive<R>,
    destination: &Path,
    max_total_bytes: u64,
) -> Result<(), PluginBundleUnpackError> {
    let mut extracted_bytes = 0u64;
    let mut entry_count = 0usize;
    let mut seen_paths = HashSet::new();
    let entries = archive.entries().map_err(|source| {
        PluginBundleUnpackError::io("failed to read plugin bundle tar", source)
    })?;
    for entry in entries {
        entry_count = entry_count.saturating_add(1);
        enforce_archive_entry_count(entry_count)?;
        let mut entry = entry.map_err(|source| {
            PluginBundleUnpackError::io("failed to read plugin bundle tar entry", source)
        })?;
        let entry_type = entry.header().entry_type();
        let entry_size = entry.size();
        let entry_path = entry
            .path()
            .map_err(|source| {
                PluginBundleUnpackError::io("failed to read plugin bundle tar entry path", source)
            })?
            .into_owned();
        let output_path = checked_tar_output_path(destination, &entry_path)?;
        if !seen_paths.insert(output_path.clone()) {
            return Err(PluginBundleUnpackError::InvalidBundle(format!(
                "plugin bundle tar contains duplicate path `{}`",
                entry_path.display()
            )));
        }

        if entry_type.is_dir() {
            fs::create_dir_all(&output_path).map_err(|source| {
                PluginBundleUnpackError::io("failed to create plugin bundle directory", source)
            })?;
            continue;
        }

        if entry_type.is_file() {
            enforce_total_extracted_size(entry_size, &mut extracted_bytes, max_total_bytes)?;
            let Some(parent) = output_path.parent() else {
                return Err(PluginBundleUnpackError::InvalidBundle(format!(
                    "plugin bundle output path has no parent: {}",
                    output_path.display()
                )));
            };
            fs::create_dir_all(parent).map_err(|source| {
                PluginBundleUnpackError::io("failed to create plugin bundle directory", source)
            })?;
            entry.unpack(&output_path).map_err(|source| {
                PluginBundleUnpackError::io("failed to unpack plugin bundle entry", source)
            })?;
            continue;
        }

        if entry_type.is_hard_link() || entry_type.is_symlink() {
            return Err(PluginBundleUnpackError::InvalidBundle(format!(
                "plugin bundle tar entry `{}` is a link",
                entry_path.display()
            )));
        }

        return Err(PluginBundleUnpackError::InvalidBundle(format!(
            "plugin bundle tar entry `{}` has unsupported type {:?}",
            entry_path.display(),
            entry_type
        )));
    }

    Ok(())
}

fn checked_tar_output_path(
    destination: &Path,
    entry_name: &Path,
) -> Result<PathBuf, PluginBundleUnpackError> {
    let mut output_path = destination.to_path_buf();
    let mut component_count = 0usize;
    for component in entry_name.components() {
        match component {
            std::path::Component::Normal(component) => {
                component_count = component_count.saturating_add(1);
                if component_count > MAX_PLUGIN_BUNDLE_PATH_COMPONENTS {
                    return Err(PluginBundleUnpackError::InvalidBundle(format!(
                        "plugin bundle tar entry `{}` exceeds maximum path depth of {MAX_PLUGIN_BUNDLE_PATH_COMPONENTS}",
                        entry_name.display()
                    )));
                }
                output_path.push(component);
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(PluginBundleUnpackError::InvalidBundle(format!(
                    "plugin bundle tar entry `{}` escapes extraction root",
                    entry_name.display()
                )));
            }
        }
    }
    if component_count == 0 {
        return Err(PluginBundleUnpackError::InvalidBundle(
            "plugin bundle tar entry has an empty path".to_string(),
        ));
    }
    Ok(output_path)
}

fn enforce_archive_entry_count(entry_count: usize) -> Result<(), PluginBundleUnpackError> {
    if entry_count > MAX_PLUGIN_BUNDLE_ENTRIES {
        return Err(PluginBundleUnpackError::InvalidBundle(format!(
            "plugin bundle tar exceeds maximum entry count of {MAX_PLUGIN_BUNDLE_ENTRIES}"
        )));
    }
    Ok(())
}

fn enforce_total_extracted_size(
    entry_size: u64,
    extracted_bytes: &mut u64,
    max_total_bytes: u64,
) -> Result<(), PluginBundleUnpackError> {
    let next_total = extracted_bytes.checked_add(entry_size).ok_or(
        PluginBundleUnpackError::ExtractedBundleTooLarge {
            bytes: u64::MAX,
            max_bytes: max_total_bytes,
        },
    )?;
    if next_total > max_total_bytes {
        return Err(PluginBundleUnpackError::ExtractedBundleTooLarge {
            bytes: next_total,
            max_bytes: max_total_bytes,
        });
    }
    *extracted_bytes = next_total;
    Ok(())
}

struct SizeLimitedBuffer {
    bytes: Vec<u8>,
    max_bytes: usize,
}

impl SizeLimitedBuffer {
    fn new(max_bytes: usize) -> Self {
        Self {
            bytes: Vec::new(),
            max_bytes,
        }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for SizeLimitedBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let next_len = self.bytes.len().checked_add(buf.len()).ok_or_else(|| {
            io::Error::other(ArchiveSizeLimitExceeded {
                bytes: usize::MAX,
                max_bytes: self.max_bytes,
            })
        })?;
        if next_len > self.max_bytes {
            return Err(io::Error::other(ArchiveSizeLimitExceeded {
                bytes: next_len,
                max_bytes: self.max_bytes,
            }));
        }

        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct ArchiveSizeLimitExceeded {
    bytes: usize,
    max_bytes: usize,
}

impl fmt::Display for ArchiveSizeLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "archive would be {} bytes, exceeding maximum size of {} bytes",
            self.bytes, self.max_bytes
        )
    }
}

impl std::error::Error for ArchiveSizeLimitExceeded {}

#[cfg(test)]
#[path = "plugin_bundle_archive_tests.rs"]
mod tests;
