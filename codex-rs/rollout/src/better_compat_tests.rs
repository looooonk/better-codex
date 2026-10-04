use super::*;
use codex_protocol::protocol::SessionMeta;
use codex_protocol::protocol::ThreadHistoryMode;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn resolves_replacement_heads_and_compressed_saved_revisions() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let thread_id = ThreadId::new();
    let head_id = ThreadId::new();
    let sessions = home.path().join("sessions/2026/09/01");
    let revisions = home
        .path()
        .join(REVISIONS_SUBDIR)
        .join(thread_id.to_string());
    tokio::fs::create_dir_all(&sessions).await?;
    tokio::fs::create_dir_all(&revisions).await?;
    let path = sessions.join(format!("rollout-2026-09-01T00-00-00-{thread_id}.jsonl"));
    let mut payload = serde_json::to_value(SessionMeta {
        id: thread_id,
        session_id: thread_id.into(),
        history_mode: ThreadHistoryMode::Paginated,
        ..Default::default()
    })?;
    let revision = json!({"timestamp": "2026-09-01T00:00:00Z", "ordinal": 0,
        "type": "session_meta", "payload": payload});
    let revision_path = revisions.join(format!("{thread_id}.jsonl.zst"));
    tokio::fs::write(
        &revision_path,
        zstd::stream::encode_all(format!("{revision}\n").as_bytes(), 0)?,
    )
    .await?;
    payload["rollout_id"] = json!(head_id);
    let head = json!({"timestamp": "2026-09-01T00:00:00Z", "ordinal": 0,
        "type": "session_meta", "payload": payload});
    tokio::fs::write(&path, format!("{head}\n")).await?;

    assert_eq!(rollout_id_for_path(&path).await?, Some(head_id));
    assert_eq!(
        crate::find_rollout_path_by_rollout_id(home.path(), thread_id).await?,
        Some(revision_path.clone())
    );
    assert_eq!(
        crate::find_rollout_path_by_rollout_id(home.path(), head_id).await?,
        Some(path.clone())
    );
    let index = crate::RolloutReferenceIndex::scan(home.path()).await?;
    let mut actual = index
        .rollouts_for_thread(thread_id)
        .map(|(id, path)| (id, path.to_path_buf()))
        .collect::<Vec<_>>();
    actual.sort_by_key(|(id, _)| id.to_string());
    let mut expected = vec![(head_id, path), (thread_id, revision_path)];
    expected.sort_by_key(|(id, _)| id.to_string());
    assert_eq!(actual, expected);
    Ok(())
}

#[tokio::test]
async fn rejects_inconsistent_legacy_storage_identity() -> anyhow::Result<()> {
    let home = TempDir::new()?;
    let thread_id = ThreadId::new();
    let path = home.path().join(format!("{}.jsonl", ThreadId::new()));
    let line = json!({"type":"session_meta", "payload": {
        "id": thread_id, "rollout_id": ThreadId::new()
    }});
    tokio::fs::write(&path, format!("{line}\n")).await?;
    assert_eq!(
        rollout_id_for_path(&path).await.unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    Ok(())
}
