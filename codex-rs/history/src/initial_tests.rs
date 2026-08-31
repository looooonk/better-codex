use std::sync::Arc;

use anyhow::Result;
use codex_protocol::ThreadId;
use codex_protocol::config_types::MultiAgentMode;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionMeta;
use codex_protocol::protocol::SessionMetaLine;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_utils_absolute_path::test_support::test_path_buf;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[test]
fn copied_history_uses_persisted_history_mode() -> Result<()> {
    let thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")?;
    let session_meta = RolloutItem::SessionMeta(SessionMetaLine {
        meta: SessionMeta {
            session_id: thread_id.into(),
            id: thread_id,
            history_mode: ThreadHistoryMode::Legacy,
            ..SessionMeta::default()
        },
        git: None,
    });
    let history = InitialHistory::Resumed(ResumedHistory {
        conversation_id: thread_id,
        history: Arc::new(vec![session_meta.clone()]),
        rollout_path: None,
    });

    assert_eq!(
        history.get_history_mode(ThreadHistoryMode::Paginated),
        ThreadHistoryMode::Legacy
    );
    assert_eq!(
        InitialHistory::Forked(vec![session_meta]).get_history_mode(ThreadHistoryMode::Paginated),
        ThreadHistoryMode::Legacy
    );
    assert_eq!(
        InitialHistory::New.get_history_mode(ThreadHistoryMode::Paginated),
        ThreadHistoryMode::Paginated
    );
    assert_eq!(
        InitialHistory::Resumed(ResumedHistory {
            conversation_id: thread_id,
            history: Arc::new(Vec::new()),
            rollout_path: None,
        })
        .get_history_mode(ThreadHistoryMode::Paginated),
        ThreadHistoryMode::Paginated
    );
    Ok(())
}

#[test]
fn multi_agent_version_uses_newest_present_session_meta_value() -> Result<()> {
    let thread_id = ThreadId::from_string("67e55044-10b1-426f-9247-bb680e5fe0c8")?;
    let older_meta = SessionMetaLine {
        meta: SessionMeta {
            session_id: thread_id.into(),
            id: thread_id,
            multi_agent_version: Some(MultiAgentVersion::V2),
            ..Default::default()
        },
        git: None,
    };
    let newer_meta_without_version = SessionMetaLine {
        meta: SessionMeta {
            session_id: thread_id.into(),
            id: thread_id,
            multi_agent_version: None,
            ..Default::default()
        },
        git: None,
    };
    let history = InitialHistory::Resumed(ResumedHistory {
        conversation_id: thread_id,
        history: Arc::new(vec![
            RolloutItem::SessionMeta(older_meta),
            RolloutItem::SessionMeta(newer_meta_without_version),
        ]),
        rollout_path: None,
    });

    assert_eq!(history.get_multi_agent_version(), Some(MultiAgentVersion::V2));
    Ok(())
}

#[test]
fn latest_effective_multi_agent_mode_uses_latest_turn_context_even_when_unset() -> Result<()> {
    let turn_context_item = |multi_agent_mode| -> Result<RolloutItem> {
        let mut value = json!({
            "cwd": test_path_buf("/tmp"),
            "approval_policy": "never",
            "sandbox_policy": { "type": "danger-full-access" },
            "model": "gpt-5",
            "summary": "auto",
        });
        value["multi_agent_mode"] = serde_json::to_value(multi_agent_mode)?;
        Ok(RolloutItem::TurnContext(serde_json::from_value(value)?))
    };

    assert_eq!(
        InitialHistory::Forked(vec![
            turn_context_item(Some(MultiAgentMode::Proactive))?,
            turn_context_item(/*multi_agent_mode*/ None)?,
        ])
        .get_latest_effective_multi_agent_mode(),
        None
    );
    Ok(())
}

#[test]
fn latest_effective_multi_agent_mode_maps_legacy_none_to_empty_custom() -> Result<()> {
    let value = json!({
        "cwd": test_path_buf("/tmp"),
        "approval_policy": "never",
        "sandbox_policy": { "type": "danger-full-access" },
        "model": "gpt-5",
        "multi_agent_mode": "none",
        "summary": "auto",
    });
    let item = RolloutItem::TurnContext(serde_json::from_value(value)?);

    assert_eq!(
        InitialHistory::Forked(vec![item]).get_latest_effective_multi_agent_mode(),
        Some(MultiAgentMode::Custom(String::new()))
    );
    Ok(())
}
