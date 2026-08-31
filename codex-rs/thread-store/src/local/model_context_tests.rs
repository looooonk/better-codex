use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AskForApproval;
use codex_rollout::CompactedItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::HistoryPosition;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_rollout::RolloutItem;
use codex_rollout::RolloutLine;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnContextItem;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::user_input::UserInput;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use uuid::Uuid;

use super::*;
use crate::ThreadStore;
use crate::local::test_support::test_config;
use crate::local::test_support::write_session_file_with_history_mode;

#[tokio::test]
async fn loads_latest_checkpoint_with_required_turn_metadata() {
    let home = TempDir::new().expect("temp dir");
    let uuid = Uuid::from_u128(/*v*/ 1001);
    let thread_id = codex_protocol::ThreadId::from_string(&uuid.to_string()).expect("thread id");
    write_paginated_rollout(
        home.path(),
        "2025-01-03T13-00-00",
        uuid,
        [
            turn_started("turn-1"),
            user_message("older turn"),
            completed_user_message("turn-1", "older turn"),
            turn_context(home.path(), "turn-1"),
            compacted("older checkpoint", Some(Vec::new())),
            turn_complete("turn-1"),
            turn_started("turn-2"),
            user_message("latest turn"),
            completed_user_message("turn-2", "latest turn"),
            turn_context(home.path(), "turn-2"),
            compacted("latest checkpoint", Some(Vec::new())),
            turn_complete("turn-2"),
        ],
    );
    let store = LocalThreadStore::new(test_config(home.path()), /*state_db*/ None);

    let context = store
        .load_latest_model_context(LoadThreadHistoryParams {
            thread_id,
            include_archived: false,
        })
        .await
        .expect("load model context");

    assert!(matches!(
        context.items.first(),
        Some(RolloutItem::SessionMeta(_))
    ));
    assert!(context.items.iter().any(|item| {
        matches!(item, RolloutItem::Compacted(compacted) if compacted.message == "latest checkpoint")
    }));
    assert!(!context.items.iter().any(|item| {
        matches!(item, RolloutItem::Compacted(compacted) if compacted.message == "older checkpoint")
    }));
    assert!(context.items.iter().any(|item| {
        matches!(item, RolloutItem::TurnContext(context) if context.turn_id.as_deref() == Some("turn-2"))
    }));
}

#[tokio::test]
async fn loads_turn_metadata_across_an_older_checkpoint() {
    let home = TempDir::new().expect("temp dir");
    let uuid = Uuid::from_u128(/*v*/ 1006);
    let thread_id = codex_protocol::ThreadId::from_string(&uuid.to_string()).expect("thread id");
    write_paginated_rollout(
        home.path(),
        "2025-01-03T13-00-05",
        uuid,
        [
            turn_started("turn-0"),
            user_message("oldest turn"),
            completed_user_message("turn-0", "oldest turn"),
            turn_context(home.path(), "turn-0"),
            turn_complete("turn-0"),
            turn_started("turn-1"),
            user_message("metadata turn"),
            completed_user_message("turn-1", "metadata turn"),
            turn_context(home.path(), "turn-1"),
            compacted("older checkpoint", Some(Vec::new())),
            turn_complete("turn-1"),
            turn_started("turn-2"),
            compacted("latest checkpoint", Some(Vec::new())),
            turn_complete("turn-2"),
        ],
    );
    let store = LocalThreadStore::new(test_config(home.path()), /*state_db*/ None);

    let context = store
        .load_latest_model_context(LoadThreadHistoryParams {
            thread_id,
            include_archived: false,
        })
        .await
        .expect("load model context");

    assert!(context.items.iter().any(|item| {
        matches!(item, RolloutItem::Compacted(compacted) if compacted.message == "latest checkpoint")
    }));
    assert!(context.items.iter().any(|item| {
        matches!(item, RolloutItem::TurnContext(context) if context.turn_id.as_deref() == Some("turn-1"))
    }));
    assert!(!context.items.iter().any(|item| {
        matches!(item, RolloutItem::TurnContext(context) if context.turn_id.as_deref() == Some("turn-0"))
    }));
}

#[tokio::test]
async fn returns_scanned_full_history_for_unsupported_compaction() {
    let home = TempDir::new().expect("temp dir");
    let uuid = Uuid::from_u128(/*v*/ 1002);
    let path = write_paginated_rollout(
        home.path(),
        "2025-01-03T13-00-01",
        uuid,
        [
            turn_started("turn-1"),
            user_message("turn"),
            completed_user_message("turn-1", "turn"),
            turn_context(home.path(), "turn-1"),
            compacted("usable checkpoint", Some(Vec::new())),
            compacted("legacy checkpoint", /*replacement_history*/ None),
            turn_complete("turn-1"),
        ],
    );

    assert_reverse_scan_matches_full_history(path.as_path()).await;
}

#[tokio::test]
async fn returns_scanned_full_history_at_bof_without_checkpoint() {
    let home = TempDir::new().expect("temp dir");
    let uuid = Uuid::from_u128(/*v*/ 1003);
    let path = write_paginated_rollout(
        home.path(),
        "2025-01-03T13-00-02",
        uuid,
        [
            turn_started("turn-1"),
            user_message("turn"),
            completed_user_message("turn-1", "turn"),
            turn_context(home.path(), "turn-1"),
            turn_complete("turn-1"),
        ],
    );

    assert_reverse_scan_matches_full_history(path.as_path()).await;
}

#[tokio::test]
async fn uses_agent_message_turn_context_without_scanning_older_turn() {
    let home = TempDir::new().expect("temp dir");
    let uuid = Uuid::from_u128(/*v*/ 1004);
    let thread_id = codex_protocol::ThreadId::from_string(&uuid.to_string()).expect("thread id");
    write_paginated_rollout(
        home.path(),
        "2025-01-03T13-00-03",
        uuid,
        [
            turn_started("turn-1"),
            user_message("older turn"),
            completed_user_message("turn-1", "older turn"),
            turn_context(home.path(), "turn-1"),
            compacted("checkpoint", Some(Vec::new())),
            turn_complete("turn-1"),
            turn_started("turn-2"),
            turn_context(home.path(), "turn-2"),
            agent_message("child done"),
            turn_complete("turn-2"),
        ],
    );
    let store = LocalThreadStore::new(test_config(home.path()), /*state_db*/ None);

    let context = store
        .load_latest_model_context(LoadThreadHistoryParams {
            thread_id,
            include_archived: false,
        })
        .await
        .expect("load model context");

    assert!(context.items.iter().any(|item| {
        matches!(item, RolloutItem::TurnContext(context) if context.turn_id.as_deref() == Some("turn-2"))
    }));
    assert!(!context.items.iter().any(|item| {
        matches!(item, RolloutItem::TurnContext(context) if context.turn_id.as_deref() == Some("turn-1"))
    }));
}

#[tokio::test]
async fn ignores_contextual_user_messages_when_selecting_turn_context() {
    let home = TempDir::new().expect("temp dir");
    let uuid = Uuid::from_u128(/*v*/ 1005);
    let thread_id = codex_protocol::ThreadId::from_string(&uuid.to_string()).expect("thread id");
    write_paginated_rollout(
        home.path(),
        "2025-01-03T13-00-04",
        uuid,
        [
            turn_started("turn-1"),
            user_message("real user turn"),
            completed_user_message("turn-1", "real user turn"),
            turn_context(home.path(), "turn-1"),
            compacted("checkpoint", Some(Vec::new())),
            turn_complete("turn-1"),
            turn_started("turn-2"),
            contextual_user_message(),
            turn_context(home.path(), "turn-2"),
            turn_complete("turn-2"),
        ],
    );
    let store = LocalThreadStore::new(test_config(home.path()), /*state_db*/ None);

    let context = store
        .load_latest_model_context(LoadThreadHistoryParams {
            thread_id,
            include_archived: false,
        })
        .await
        .expect("load model context");

    assert!(context.items.iter().any(|item| {
        matches!(item, RolloutItem::TurnContext(context) if context.turn_id.as_deref() == Some("turn-1"))
    }));
}

#[tokio::test]
async fn replays_replacement_lineage_with_canonical_head_metadata() {
    let home = TempDir::new().expect("temp dir");
    let uuid = Uuid::from_u128(/*v*/ 1100);
    let thread_id = codex_protocol::ThreadId::from_string(&uuid.to_string()).expect("thread id");
    let root_path = write_paginated_rollout(
        home.path(),
        "2025-01-03T13-01-00",
        uuid,
        [user_message("root message")],
    );
    let middle_id = codex_protocol::RolloutId::new();
    let middle_path = write_replacement_rollout(
        home.path(),
        "2025-01-03T13-01-01",
        uuid,
        middle_id,
        [user_message("middle message")],
    );
    set_history_base(
        middle_path.as_path(),
        HistoryPosition {
            thread_id,
            end_ordinal_exclusive: 2,
            end_byte_offset: std::fs::metadata(root_path)
                .expect("root metadata")
                .len(),
        },
    );
    let head_id = codex_protocol::RolloutId::new();
    let head_path = write_replacement_rollout(
        home.path(),
        "2025-01-03T13-01-02",
        uuid,
        head_id,
        [user_message("head message")],
    );
    set_history_base(
        head_path.as_path(),
        HistoryPosition {
            thread_id: middle_id,
            end_ordinal_exclusive: 4,
            end_byte_offset: std::fs::metadata(middle_path)
                .expect("middle metadata")
                .len(),
        },
    );
    let store = LocalThreadStore::new(test_config(home.path()), /*state_db*/ None);

    let context = store
        .load_latest_model_context(LoadThreadHistoryParams {
            thread_id,
            include_archived: false,
        })
        .await
        .expect("load replacement lineage");

    assert!(matches!(
        context.items.first(),
        Some(RolloutItem::SessionMeta(meta)) if meta.meta.id == thread_id
    ));
    assert_eq!(
        context
            .items
            .iter()
            .filter_map(|item| match item {
                RolloutItem::ResponseItem(ResponseItem::Message { content, .. }) => {
                    content.first().and_then(|content| match content {
                        ContentItem::InputText { text } => Some(text.as_str()),
                        _ => None,
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec!["root message", "middle message", "head message"]
    );
}

fn write_paginated_rollout<const N: usize>(
    home: &Path,
    timestamp: &str,
    uuid: Uuid,
    items: [RolloutItem; N],
) -> PathBuf {
    let path =
        write_session_file_with_history_mode(home, timestamp, uuid, ThreadHistoryMode::Paginated)
            .expect("write session file");
    append_items(path.as_path(), items);
    path
}

fn write_replacement_rollout<const N: usize>(
    home: &Path,
    timestamp: &str,
    uuid: Uuid,
    rollout_id: codex_protocol::RolloutId,
    items: [RolloutItem; N],
) -> PathBuf {
    let path = write_paginated_rollout(home, timestamp, uuid, items);
    let replacement = path.with_file_name(format!(
        "rollout-{timestamp}-{}_{rollout_id}.jsonl",
        uuid
    ));
    std::fs::rename(path, replacement.as_path()).expect("rename replacement rollout");
    replacement
}

fn set_history_base(path: &Path, history_base: HistoryPosition) {
    let contents = std::fs::read_to_string(path).expect("read rollout");
    let mut lines = contents.lines();
    let mut head: serde_json::Value =
        serde_json::from_str(lines.next().expect("session metadata")).expect("parse metadata");
    head["payload"]["history_base"] =
        serde_json::to_value(history_base).expect("serialize history base");
    let mut updated = serde_json::to_string(&head).expect("serialize metadata");
    for line in lines {
        updated.push('\n');
        updated.push_str(line);
    }
    updated.push('\n');
    std::fs::write(path, updated).expect("write history base");
}

async fn assert_reverse_scan_matches_full_history(path: &Path) {
    let session_meta = codex_rollout::read_session_meta_line(path)
        .await
        .expect("read session metadata");
    let items =
        scan_model_context_from_end_blocking(path, session_meta).expect("scan model context");
    let full_items = read_thread::load_history_items(path)
        .await
        .expect("load full history");

    assert_eq!(
        serde_json::to_value(items).expect("serialize scanned items"),
        serde_json::to_value(full_items).expect("serialize full items")
    );
}

fn append_items<const N: usize>(path: &Path, items: [RolloutItem; N]) {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open session file");
    for item in items {
        let line = RolloutLine {
            timestamp: "2025-01-03T13:00:01Z".to_string(),
            ordinal: None,
            item,
        };
        writeln!(
            file,
            "{}",
            serde_json::to_string(&line).expect("serialize line")
        )
        .expect("append rollout line");
    }
}

fn turn_started(turn_id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: turn_id.to_string(),
        trace_id: None,
        started_at: None,
        model_context_window: Some(128_000),
        collaboration_mode_kind: Default::default(),
    }))
}

fn turn_complete(turn_id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
        turn_id: turn_id.to_string(),
        last_agent_message: None,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
        time_to_first_token_ms: None,
    }))
}

fn user_message(message: &str) -> RolloutItem {
    RolloutItem::ResponseItem(ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: message.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    })
}

fn contextual_user_message() -> RolloutItem {
    user_message("<environment_context>context only</environment_context>")
}

fn completed_user_message(turn_id: &str, message: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
        thread_id: codex_protocol::ThreadId::from_string("00000000-0000-0000-0000-000000000000")
            .expect("fixture thread id"),
        turn_id: turn_id.to_string(),
        item: TurnItem::UserMessage(UserMessageItem {
            id: format!("user-{turn_id}"),
            client_id: None,
            content: vec![UserInput::Text {
                text: message.to_string(),
                text_elements: Vec::new(),
            }],
        }),
        completed_at_ms: 0,
    }))
}

fn agent_message(message: &str) -> RolloutItem {
    RolloutItem::ResponseItem(ResponseItem::AgentMessage {
        id: None,
        author: "worker".to_string(),
        recipient: "root".to_string(),
        content: vec![AgentMessageInputContent::InputText {
            text: message.to_string(),
        }],
        internal_chat_message_metadata_passthrough: None,
    })
}

fn turn_context(root: &Path, turn_id: &str) -> RolloutItem {
    RolloutItem::TurnContext(TurnContextItem {
        turn_id: Some(turn_id.to_string()),
        cwd: serde_json::from_value(serde_json::json!(root)).expect("absolute cwd"),
        workspace_roots: None,
        current_date: None,
        timezone: None,
        approval_policy: AskForApproval::Never,
        approvals_reviewer: None,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        permission_profile: None,
        network: None,
        file_system_sandbox_policy: None,
        model: "test-model".to_string(),
        comp_hash: None,
        personality: None,
        collaboration_mode: None,
        multi_agent_version: None,
        multi_agent_mode: None,
        realtime_active: None,
        effort: None,
        summary: ReasoningSummary::Auto,
    })
}

fn compacted(message: &str, replacement_history: Option<Vec<ResponseItem>>) -> RolloutItem {
    RolloutItem::Compacted(CompactedItem {
        message: message.to_string(),
        replacement_history,
        window_number: Some(1),
        first_window_id: None,
        previous_window_id: None,
        window_id: None,
    })
}
