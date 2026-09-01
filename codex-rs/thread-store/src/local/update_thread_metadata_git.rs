use std::path::Path;

use codex_protocol::ThreadId;
use codex_protocol::protocol::GitInfo;
use codex_rollout::RolloutItem;
use codex_rollout::RolloutRecorder;

use super::LocalThreadStore;
use super::helpers::git_info_from_parts;
use crate::GitInfoPatch;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;

pub(super) struct ResolvedGitInfoUpdate {
    pub(super) sha: Option<String>,
    pub(super) branch: Option<String>,
    pub(super) origin_url: Option<String>,
    pub(super) memory_mode: Option<String>,
}

pub(super) async fn resolve_git_info_update(
    store: &LocalThreadStore,
    thread_id: ThreadId,
    rollout_path: &Path,
    patch: GitInfoPatch,
    updated_memory_mode: Option<&str>,
) -> ThreadStoreResult<ResolvedGitInfoUpdate> {
    let Some(state_db) = store.state_db().await else {
        return Err(ThreadStoreError::Internal {
            message: format!("sqlite state db unavailable for thread {thread_id}"),
        });
    };
    let metadata =
        state_db
            .get_thread(thread_id)
            .await
            .map_err(|err| ThreadStoreError::Internal {
                message: format!("failed to read git metadata for thread {thread_id}: {err}"),
            })?;
    let existing = match metadata {
        Some(metadata) => git_info_from_parts(
            metadata.git_sha,
            metadata.git_branch,
            metadata.git_origin_url,
        ),
        None => latest_rollout_git_info(rollout_path, thread_id).await?,
    };
    let (existing_sha, existing_branch, existing_origin_url) = match existing {
        Some(info) => (
            info.commit_hash.map(|sha| sha.0),
            info.branch,
            info.repository_url,
        ),
        None => (None, None, None),
    };
    let memory_mode = match updated_memory_mode {
        Some(memory_mode) => Some(memory_mode.to_string()),
        None => state_db
            .get_thread_memory_mode(thread_id)
            .await
            .map_err(|err| ThreadStoreError::Internal {
                message: format!("failed to read memory mode for thread {thread_id}: {err}"),
            })?,
    };
    Ok(ResolvedGitInfoUpdate {
        sha: patch.sha.unwrap_or(existing_sha),
        branch: patch.branch.unwrap_or(existing_branch),
        origin_url: patch.origin_url.unwrap_or(existing_origin_url),
        memory_mode,
    })
}

async fn latest_rollout_git_info(
    rollout_path: &Path,
    thread_id: ThreadId,
) -> ThreadStoreResult<Option<GitInfo>> {
    let (items, _thread_id, _parse_errors) = RolloutRecorder::load_rollout_items(rollout_path)
        .await
        .map_err(|err| ThreadStoreError::Internal {
            message: format!("failed to read git metadata for thread {thread_id}: {err}"),
        })?;
    Ok(items
        .iter()
        .filter_map(|item| match item {
            RolloutItem::SessionMeta(meta) if meta.meta.id == thread_id => meta.git.as_ref(),
            RolloutItem::SessionMeta(_)
            | RolloutItem::ResponseItem(_)
            | RolloutItem::InterAgentCommunication(_)
            | RolloutItem::InterAgentCommunicationMetadata { .. }
            | RolloutItem::Compacted(_)
            | RolloutItem::TurnContext(_)
            | RolloutItem::WorldState(_)
            | RolloutItem::SecurityRiskScore(_)
            | RolloutItem::EventMsg(_) => None,
        })
        .next_back()
        .cloned()
        .and_then(|git| {
            git_info_from_parts(
                git.commit_hash.map(|sha| sha.0),
                git.branch,
                git.repository_url,
            )
        }))
}
