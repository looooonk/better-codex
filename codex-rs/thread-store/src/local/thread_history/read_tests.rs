use chrono::Utc;
use codex_app_server_protocol::CodexErrorInfo;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use uuid::Uuid;

use super::*;
use crate::SearchThreadOccurrencesParams;
use crate::local::test_support::test_config;
use crate::local::test_support::write_session_file_with_history_mode;

#[tokio::test]
async fn list_turns_pages_projected_rows_and_applies_item_views() {
    let (_home, store, thread_id) = store_with_mode(ThreadHistoryMode::Paginated).await;
    let db = history_db(&store).await;
    for (turn_id, ordinal, status, error, first_user, final_agent) in [
        (
            "turn-1",
            10,
            "completed",
            None,
            Some("user-1"),
            Some("agent-1"),
        ),
        (
            "turn-2",
            20,
            "failed",
            Some(
                r#"{"message":"turn failed","codexErrorInfo":"serverOverloaded","additionalDetails":"retry later"}"#,
            ),
            None,
            None,
        ),
        ("turn-3", 30, "inProgress", None, None, None),
    ] {
        insert_turn(
            db,
            thread_id,
            turn_id,
            ordinal,
            status,
            error,
            first_user,
            final_agent,
        )
        .await;
    }
    for (turn_id, item_id, ordinal) in [
        ("turn-1", "user-1", 11),
        ("turn-1", "middle-1", 12),
        ("turn-1", "agent-1", 13),
    ] {
        insert_item(db, thread_id, turn_id, item_id, ordinal).await;
    }

    let first_page = store
        .list_turns(turn_params(
            thread_id,
            /*cursor*/ None,
            /*page_size*/ 2,
            SortDirection::Asc,
            StoredTurnItemsView::Summary,
        ))
        .await
        .expect("first turns page");
    assert_eq!(turn_ids(&first_page), vec!["turn-1", "turn-2"]);
    assert_eq!(
        first_page.turns[0].items,
        vec![
            expected_item("turn-1", "user-1", /*rollout_ordinal*/ 11),
            expected_item("turn-1", "agent-1", /*rollout_ordinal*/ 13),
        ]
    );
    assert_eq!(
        first_page.turns[1].error,
        Some(StoredTurnError {
            message: "turn failed".to_string(),
            codex_error_info: Some(CodexErrorInfo::ServerOverloaded),
            additional_details: Some("retry later".to_string()),
        })
    );
    let second_page = store
        .list_turns(turn_params(
            thread_id,
            first_page.next_cursor,
            /*page_size*/ 2,
            SortDirection::Asc,
            StoredTurnItemsView::NotLoaded,
        ))
        .await
        .expect("second turns page");
    assert_eq!(turn_ids(&second_page), vec!["turn-3"]);
    assert_eq!(second_page.turns[0].items, Vec::new());
    assert_eq!(second_page.turns[0].status, StoredTurnStatus::InProgress);
    let backwards_page = store
        .list_turns(turn_params(
            thread_id,
            second_page.backwards_cursor,
            /*page_size*/ 2,
            SortDirection::Desc,
            StoredTurnItemsView::NotLoaded,
        ))
        .await
        .expect("backwards turns page");
    assert_eq!(turn_ids(&backwards_page), vec!["turn-3", "turn-2"]);
}

#[tokio::test]
async fn list_items_pages_whole_thread_and_per_turn_rows() {
    let (_home, store, thread_id) = store_with_mode(ThreadHistoryMode::Paginated).await;
    let db = history_db(&store).await;
    for (turn_id, ordinal) in [("turn-1", 10), ("turn-2", 20)] {
        insert_turn(
            db,
            thread_id,
            turn_id,
            ordinal,
            "completed",
            /*error_json*/ None,
            /*first_user_item_id*/ None,
            /*final_agent_item_id*/ None,
        )
        .await;
    }
    for (turn_id, item_id, ordinal) in [
        ("turn-1", "item-1", 11),
        ("turn-1", "item-2", 12),
        ("turn-2", "item-3", 21),
        ("turn-2", "item-4", 22),
        ("turn-2", "item-5", 23),
    ] {
        insert_item(db, thread_id, turn_id, item_id, ordinal).await;
    }

    let first_page = store
        .list_items(item_params(
            thread_id,
            /*turn_id*/ None,
            /*cursor*/ None,
            /*page_size*/ 2,
            SortDirection::Asc,
        ))
        .await
        .expect("first item page");
    assert_eq!(
        first_page.items,
        vec![
            expected_item("turn-1", "item-1", /*rollout_ordinal*/ 11),
            expected_item("turn-1", "item-2", /*rollout_ordinal*/ 12),
        ]
    );
    let second_page = store
        .list_items(item_params(
            thread_id,
            /*turn_id*/ None,
            first_page.next_cursor,
            /*page_size*/ 2,
            SortDirection::Asc,
        ))
        .await
        .expect("second item page");
    assert_eq!(item_ids(&second_page), vec!["item-3", "item-4"]);
    let backwards_page = store
        .list_items(item_params(
            thread_id,
            /*turn_id*/ None,
            second_page.backwards_cursor,
            /*page_size*/ 2,
            SortDirection::Desc,
        ))
        .await
        .expect("backwards item page");
    assert_eq!(item_ids(&backwards_page), vec!["item-3", "item-2"]);

    let turn_page = store
        .list_items(item_params(
            thread_id,
            Some("turn-2"),
            /*cursor*/ None,
            /*page_size*/ 2,
            SortDirection::Desc,
        ))
        .await
        .expect("turn item page");
    assert_eq!(item_ids(&turn_page), vec!["item-5", "item-4"]);
    let whole_thread_from_turn_cursor = store
        .list_items(item_params(
            thread_id,
            /*turn_id*/ None,
            turn_page.backwards_cursor.clone(),
            /*page_size*/ 2,
            SortDirection::Desc,
        ))
        .await
        .expect("whole-thread page from turn cursor");
    assert_eq!(
        item_ids(&whole_thread_from_turn_cursor),
        vec!["item-5", "item-4"]
    );
    let next_turn_page = store
        .list_items(item_params(
            thread_id,
            Some("turn-2"),
            turn_page.next_cursor,
            /*page_size*/ 2,
            SortDirection::Desc,
        ))
        .await
        .expect("next turn item page");
    assert_eq!(item_ids(&next_turn_page), vec!["item-3"]);
}

#[tokio::test]
async fn selected_rollout_id_drives_history_queries_and_logical_cursors() {
    let (home, store, thread_id) = store_with_mode(ThreadHistoryMode::Paginated).await;
    let rollout_id = ThreadId::new();
    let uuid = Uuid::parse_str(&thread_id.to_string()).expect("thread UUID");
    let temporary_path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T13-00-00",
        uuid,
        ThreadHistoryMode::Paginated,
    )
    .expect("replacement rollout");
    let selected_path = temporary_path.with_file_name(format!(
        "rollout-2025-01-03T13-00-00-{thread_id}_{rollout_id}.jsonl"
    ));
    std::fs::rename(temporary_path, &selected_path).expect("select replacement rollout");
    let state_db = store.state_db().await.expect("state runtime");
    let mut metadata = state_db
        .get_thread(thread_id)
        .await
        .expect("read metadata")
        .expect("thread metadata");
    metadata.rollout_path = selected_path;
    state_db
        .upsert_thread(&metadata)
        .await
        .expect("persist selected rollout");
    let db = history_db(&store).await;
    insert_turn(
        db,
        thread_id,
        "old-turn",
        1,
        "completed",
        /*error_json*/ None,
        Some("old-item"),
        /*final_agent_item_id*/ None,
    )
    .await;
    insert_item(db, thread_id, "old-turn", "old-item", 2).await;
    insert_turn(
        db,
        rollout_id,
        "new-turn",
        1,
        "completed",
        /*error_json*/ None,
        Some("new-item"),
        /*final_agent_item_id*/ None,
    )
    .await;
    insert_item(db, rollout_id, "new-turn", "new-item", 2).await;
    sqlx::query(
        "UPDATE thread_items SET item_json = ?, item_type = 'userMessage' WHERE thread_id = ? AND item_id = ?",
    )
        .bind(r#"{"type":"userMessage","id":"new-item","content":[{"type":"text","text":"selected needle"}]}"#)
        .bind(rollout_id.to_string())
        .bind("new-item")
        .execute(db)
        .await
        .expect("update searchable selected item");

    let page = store
        .list_items(item_params(
            thread_id,
            /*turn_id*/ None,
            /*cursor*/ None,
            /*page_size*/ 1,
            SortDirection::Asc,
        ))
        .await
        .expect("selected rollout page");

    assert_eq!(item_ids(&page), vec!["new-item"]);
    let cursor: serde_json::Value = serde_json::from_str(
        page.backwards_cursor
            .as_deref()
            .expect("backwards logical cursor"),
    )
    .expect("parse logical cursor");
    assert_eq!(cursor["threadId"], thread_id.to_string());

    let turns = store
        .list_turns(turn_params(
            thread_id,
            /*cursor*/ None,
            /*page_size*/ 2,
            SortDirection::Asc,
            StoredTurnItemsView::Summary,
        ))
        .await
        .expect("selected rollout turns");
    assert_eq!(turn_ids(&turns), vec!["new-turn"]);
    assert_eq!(turns.turns[0].items[0].item_id, "new-item");

    let occurrences = store
        .search_thread_occurrences(SearchThreadOccurrencesParams {
            thread_id,
            search_term: "needle".to_string(),
            cursor: None,
            page_size: 2,
        })
        .await
        .expect("search selected rollout");
    assert_eq!(occurrences.items[0].item_id, "new-item");
    let turn_cursor: serde_json::Value =
        serde_json::from_str(occurrences.items[0].turn_cursor.as_str())
            .expect("parse logical turn cursor");
    assert_eq!(turn_cursor["threadId"], thread_id.to_string());
}

#[tokio::test]
async fn list_history_keeps_legacy_threads_unsupported() {
    let (_home, store, thread_id) = store_with_mode(ThreadHistoryMode::Legacy).await;

    let error = store
        .list_turns(turn_params(
            thread_id,
            /*cursor*/ None,
            /*page_size*/ 1,
            SortDirection::Asc,
            StoredTurnItemsView::Summary,
        ))
        .await
        .expect_err("legacy turns remain unsupported");
    assert!(matches!(
        error,
        ThreadStoreError::Unsupported {
            operation: "list_turns"
        }
    ));

    let error = store
        .list_turns(turn_params(
            ThreadId::default(),
            /*cursor*/ None,
            /*page_size*/ 1,
            SortDirection::Asc,
            StoredTurnItemsView::Summary,
        ))
        .await
        .expect_err("unindexed threads remain unsupported");
    assert!(matches!(
        error,
        ThreadStoreError::Unsupported {
            operation: "list_turns"
        }
    ));
}

#[test]
fn history_pages_and_cursors_are_bounded() {
    let error = page_limit(MAX_THREAD_HISTORY_PAGE_SIZE + 1).expect_err("reject oversized page");
    assert_eq!(
        error.to_string(),
        "invalid thread-store request: page size cannot exceed 100"
    );

    let cursor = "x".repeat(MAX_THREAD_HISTORY_INPUT_BYTES + 1);
    let error = match parse_cursor(
        Some(cursor.as_str()),
        ThreadId::default(),
        &CursorScope::Items,
    ) {
        Ok(_) => panic!("oversized cursor should fail"),
        Err(error) => error,
    };
    assert_eq!(
        error.to_string(),
        "invalid thread-store request: cursor cannot exceed 65536 bytes"
    );
}

async fn store_with_mode(history_mode: ThreadHistoryMode) -> (TempDir, LocalThreadStore, ThreadId) {
    let home = TempDir::new().expect("temp dir");
    let config = test_config(home.path());
    let uuid = Uuid::new_v4();
    let thread_id = ThreadId::from_string(&uuid.to_string()).expect("valid thread id");
    let rollout_path = write_session_file_with_history_mode(
        home.path(),
        "2025-01-03T12-00-00",
        uuid,
        history_mode,
    )
    .expect("rollout fixture");
    let runtime = codex_state::StateRuntime::init(
        config.sqlite_home.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .expect("state runtime");
    let mut builder = codex_state::ThreadMetadataBuilder::new(
        thread_id,
        rollout_path,
        Utc::now(),
        SessionSource::Cli,
    );
    builder.history_mode = history_mode;
    runtime
        .upsert_thread(&builder.build(config.default_model_provider_id.as_str()))
        .await
        .expect("seed thread metadata");
    let store = LocalThreadStore::new(config, Some(runtime));
    (home, store, thread_id)
}

async fn history_db(store: &LocalThreadStore) -> &sqlx::SqlitePool {
    store
        .thread_history_db()
        .await
        .expect("open history fixture database")
}

#[allow(clippy::too_many_arguments)]
async fn insert_turn(
    db: &sqlx::SqlitePool,
    thread_id: ThreadId,
    turn_id: &str,
    rollout_ordinal: i64,
    status: &str,
    error_json: Option<&str>,
    first_user_item_id: Option<&str>,
    final_agent_item_id: Option<&str>,
) {
    sqlx::query(
        r#"
INSERT INTO thread_turns (
    thread_id,
    turn_id,
    rollout_ordinal,
    status,
    error_json,
    first_user_item_id,
    final_agent_item_id
) VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(thread_id.to_string())
    .bind(turn_id)
    .bind(rollout_ordinal)
    .bind(status)
    .bind(error_json)
    .bind(first_user_item_id)
    .bind(final_agent_item_id)
    .execute(db)
    .await
    .expect("insert turn fixture");
}

async fn insert_item(
    db: &sqlx::SqlitePool,
    thread_id: ThreadId,
    turn_id: &str,
    item_id: &str,
    rollout_ordinal: i64,
) {
    sqlx::query(
        "INSERT INTO thread_items (thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(thread_id.to_string())
    .bind(turn_id)
    .bind(item_id)
    .bind(rollout_ordinal)
    .bind(rollout_ordinal * 1_000)
    .bind(format!(r#"{{"type":"userMessage","id":"{item_id}","content":[]}}"#))
    .execute(db)
    .await
    .expect("insert item fixture");
}

fn turn_params(
    thread_id: ThreadId,
    cursor: Option<String>,
    page_size: usize,
    sort_direction: SortDirection,
    items_view: StoredTurnItemsView,
) -> ListTurnsParams {
    ListTurnsParams {
        thread_id,
        include_archived: false,
        cursor,
        page_size,
        sort_direction,
        items_view,
    }
}

fn item_params(
    thread_id: ThreadId,
    turn_id: Option<&str>,
    cursor: Option<String>,
    page_size: usize,
    sort_direction: SortDirection,
) -> ListItemsParams {
    ListItemsParams {
        thread_id,
        turn_id: turn_id.map(str::to_owned),
        include_archived: false,
        cursor,
        page_size,
        sort_direction,
    }
}

fn expected_item(turn_id: &str, item_id: &str, rollout_ordinal: u64) -> StoredThreadItem {
    StoredThreadItem {
        turn_id: turn_id.to_string(),
        item_id: item_id.to_string(),
        created_at_ms: i64::try_from(rollout_ordinal).expect("fixture ordinal fits i64") * 1_000,
        item_json: format!(r#"{{"type":"userMessage","id":"{item_id}","content":[]}}"#)
            .into_bytes(),
    }
}

fn turn_ids(page: &TurnPage) -> Vec<&str> {
    page.turns
        .iter()
        .map(|turn| turn.turn_id.as_str())
        .collect()
}

fn item_ids(page: &ItemPage) -> Vec<&str> {
    page.items
        .iter()
        .map(|item| item.item_id.as_str())
        .collect()
}
