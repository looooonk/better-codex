//! Store Better Codex approvals separately from the stock rules directory.

use super::RULE_EXTENSION;
use super::RULES_DIR_NAME;
use codex_execpolicy::AmendError;
use codex_execpolicy::Decision;
use codex_execpolicy::rule::NetworkRule;
use codex_utils_path::paths_match_after_normalization;
use codex_utils_path::resolve_symlink_write_paths;
use codex_utils_path::symlink_metadata_if_exists;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

pub(super) fn path(home: &Path) -> PathBuf {
    home.join("better-codex.exec-policy")
}

/// The host's last allow or deny wins within each client. Apply Better's
/// effective denials after stock user rules and before project and managed rules.
pub(super) fn defer_effective_denials(
    rules: &mut Vec<NetworkRule>,
    start: usize,
) -> Vec<NetworkRule> {
    if !rules[start..]
        .iter()
        .any(|rule| rule.decision == Decision::Forbidden)
    {
        return Vec::new();
    }
    let private = rules.split_off(start);
    let mut seen = HashSet::new();
    let (denials, remaining): (Vec<_>, Vec<_>) =
        private
            .into_iter()
            .rev()
            .partition(|rule| match rule.decision {
                Decision::Allow | Decision::Forbidden => {
                    seen.insert(rule.host.clone()) && rule.decision == Decision::Forbidden
                }
                Decision::Prompt | Decision::PromptUser => false,
            });
    rules.extend(remaining.into_iter().rev());
    denials.into_iter().rev().collect()
}

pub(super) async fn exists(path: &Path) -> io::Result<bool> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => check_regular_file(&metadata).map(|()| true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn check_regular_file(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_file() {
        return Err(io::Error::other(
            "expected a regular Better Codex policy file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            return Err(io::Error::other(
                "Better Codex policy file has multiple hard links",
            ));
        }
    }
    Ok(())
}

pub(super) fn append_line(path: &Path, line: &str) -> Result<(), AmendError> {
    write_line(path, line).map_err(|source| AmendError::WritePolicyFile {
        path: path.to_owned(),
        source,
    })
}

fn write_line(path: &Path, line: &str) -> io::Result<()> {
    let home = path
        .parent()
        .ok_or_else(|| io::Error::other("policy path needs a parent"))?;
    fs::create_dir_all(home)?;
    let lock_path = path.with_added_extension("lock");
    // Even a dangling stock symlink must not start pointing to our file or lock
    // when the user first saves a Better Codex approval.
    check_stock_symlink(&home.join(".sandbox_migration"), path, &lock_path)?;
    match fs::read_dir(home.join(RULES_DIR_NAME)) {
        Ok(entries) => {
            for entry in entries {
                let source = entry?.path();
                if source.extension().is_some_and(|ext| ext == RULE_EXTENSION) {
                    check_stock_symlink(&source, path, &lock_path)?;
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let mut options = fs::OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
    }
    let lock = match options.create_new(true).open(&lock_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            check_regular_file(&fs::symlink_metadata(&lock_path)?)?;
            options.create_new(false).open(&lock_path)?
        }
        Err(error) => return Err(error),
    };
    lock.lock()?;

    // Keep the lock on a stable sibling across atomic replacement. A failure
    // leaves both the old policy and the stock rules intact.
    let mut contents = if let Some(metadata) = symlink_metadata_if_exists(path)? {
        check_regular_file(&metadata)?;
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    // Repeating a network rule after an intervening decision can change the
    // effective host permission. Prefix permissions have no such ordering.
    let already_saved = if line.starts_with("network_rule(") {
        contents.lines().last() == Some(line)
    } else {
        contents.lines().any(|existing| existing == line)
    };
    if already_saved {
        return Ok(());
    }
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(line);
    contents.push('\n');
    codex_utils_path::write_atomically(path, &contents)
}

fn check_stock_symlink(source: &Path, path: &Path, lock: &Path) -> io::Result<()> {
    if symlink_metadata_if_exists(source)?.is_some_and(|meta| meta.is_symlink()) {
        let target = resolve_symlink_write_paths(source)?.write_path;
        if paths_match_after_normalization(&target, path)
            || paths_match_after_normalization(target, lock)
        {
            return Err(io::Error::other(format!(
                "stock Codex path {} points to the Better Codex policy or lock",
                source.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "private_rules_tests.rs"]
mod tests;
