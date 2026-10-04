use std::io;
use std::path::Path;
use std::path::PathBuf;

use codex_protocol::RolloutId;
use codex_protocol::ThreadId;
use serde::Deserialize;

pub(crate) const REVISIONS_SUBDIR: &str = "rollout_revisions";

pub(crate) fn revision_id(path: &Path) -> Option<RolloutId> {
    let name = path.file_name()?.to_str()?;
    let name = name
        .strip_suffix(".zst")
        .unwrap_or(name)
        .strip_suffix(".jsonl")?;
    RolloutId::from_string(name).ok()
}

/// Reads immutable storage identity from older Better Codex metadata or current filenames.
pub async fn rollout_id_for_path(path: &Path) -> io::Result<Option<RolloutId>> {
    #[derive(Deserialize)]
    struct Identity {
        id: ThreadId,
        rollout_id: Option<RolloutId>,
    }
    let path_id = crate::rollout_id_from_path(path);
    let mut reader = crate::open_rollout_line_reader(path).await?;
    while let Some(line) = reader.next_line().await? {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(serde_json::Value::as_str) != Some("session_meta") {
            return Ok(path_id);
        }
        let identity: Identity = serde_json::from_value(value["payload"].clone())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if let Some(rollout_id) = identity.rollout_id {
            if path_id.is_some_and(|id| id != identity.id && id != rollout_id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "rollout metadata identity disagrees with filename",
                ));
            }
            return Ok(Some(rollout_id));
        }
        return Ok(path_id);
    }
    Ok(path_id)
}

pub(crate) async fn find_legacy_rollout(
    codex_home: &Path,
    rollout_id: RolloutId,
) -> io::Result<Option<PathBuf>> {
    let revisions = codex_home.join(REVISIONS_SUBDIR);
    if !tokio::fs::try_exists(&revisions).await? {
        return Ok(None);
    }
    let mut directories = vec![
        codex_home.join(crate::SESSIONS_SUBDIR),
        codex_home.join(crate::ARCHIVED_SESSIONS_SUBDIR),
        revisions,
    ];
    while let Some(directory) = directories.pop() {
        let mut entries = match tokio::fs::read_dir(directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            let path = entry.path();
            if file_type.is_dir() {
                directories.push(path);
            } else if file_type.is_file()
                && let Some(file) = crate::compression::RolloutFile::from_path(path)
                && (revision_id(file.path()) == Some(rollout_id)
                    || revision_id(file.path()).is_none())
                && matches!(rollout_id_for_path(file.path()).await,
                    Ok(Some(id)) if id == rollout_id)
            {
                return Ok(Some(file.into_path()));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
#[path = "better_compat_tests.rs"]
mod tests;
