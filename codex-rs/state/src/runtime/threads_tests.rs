use super::*;
use crate::Anchor;
use crate::DirectionalThreadSpawnEdgeStatus;
use crate::runtime::test_support::test_thread_metadata;
use crate::runtime::test_support::unique_temp_dir;
use anyhow::Result;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::GitInfo;
use codex_protocol::protocol::SessionMeta;
use codex_protocol::protocol::SessionMetaLine;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::path::PathBuf;

#[tokio::test]
async fn rollout_path_compare_and_swap_rejects_stale_and_replayed_updates() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id = ThreadId::new();
    let original_path = codex_home.join("original.jsonl");
    let replacement_path = codex_home.join("replacement.jsonl");
    let replay_path = codex_home.join("replay.jsonl");
    let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
    metadata.rollout_path = original_path.clone();
    runtime
        .upsert_thread(&metadata)
        .await
        .expect("insert thread metadata");

    assert!(
        !runtime
            .replace_rollout_path_if_current(
                thread_id,
                replacement_path.as_path(),
                replay_path.as_path(),
            )
            .await
            .expect("reject stale swap")
    );
    assert!(
        runtime
            .replace_rollout_path_if_current(
                thread_id,
                original_path.as_path(),
                replacement_path.as_path(),
            )
            .await
            .expect("apply current swap")
    );
    assert!(
        !runtime
            .replace_rollout_path_if_current(
                thread_id,
                original_path.as_path(),
                replay_path.as_path(),
            )
            .await
            .expect("reject replayed swap")
    );
    assert_eq!(
        runtime
            .get_thread(thread_id)
            .await
            .expect("read metadata")
            .expect("thread metadata")
            .rollout_path,
        replacement_path
    );
}

#[tokio::test]
async fn upsert_thread_keeps_creation_memory_mode_for_existing_rows() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000123").expect("valid thread id");
    let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());

    runtime
        .upsert_thread_with_creation_memory_mode(&metadata, Some("disabled"))
        .await
        .expect("initial insert should succeed");

    let memory_mode: String = sqlx::query_scalar("SELECT memory_mode FROM threads WHERE id = ?")
        .bind(thread_id.to_string())
        .fetch_one(runtime.pool.as_ref())
        .await
        .expect("memory mode should be readable");
    assert_eq!(memory_mode, "disabled");

    metadata.title = "updated title".to_string();
    runtime
        .upsert_thread(&metadata)
        .await
        .expect("upsert should succeed");

    let memory_mode: String = sqlx::query_scalar("SELECT memory_mode FROM threads WHERE id = ?")
        .bind(thread_id.to_string())
        .fetch_one(runtime.pool.as_ref())
        .await
        .expect("memory mode should remain readable");
    assert_eq!(memory_mode, "disabled");
}

#[tokio::test]
async fn thread_metadata_round_trips_history_mode() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000124").expect("valid thread id");
    let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
    metadata.history_mode = ThreadHistoryMode::Paginated;

    runtime
        .upsert_thread(&metadata)
        .await
        .expect("upsert should succeed");

    let metadata = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(metadata.history_mode, ThreadHistoryMode::Paginated);
}

#[tokio::test]
async fn delete_thread_cleans_associated_state() -> Result<()> {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string()).await?;
    let thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000401")?;
    let child_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000402")?;
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.clone(),
        ))
        .await?;
    seed_thread_cleanup_state(&runtime, thread_id, child_thread_id).await?;
    sqlx::query("INSERT INTO thread_dynamic_tools (thread_id, position, name, description, input_schema) VALUES (?, ?, ?, ?, ?)")
    .bind(thread_id.to_string())
    .bind(0_i64)
    .bind("test_tool")
    .bind("test dynamic tool")
    .bind("{}")
    .execute(runtime.pool.as_ref())
    .await?;
    runtime
        .create_agent_job(
            &AgentJobCreateParams {
                id: "job-1".to_string(),
                name: "test-job".to_string(),
                instruction: "Return a result".to_string(),
                auto_export: true,
                max_runtime_seconds: None,
                output_schema_json: None,
                input_headers: vec!["path".to_string()],
                input_csv_path: "/tmp/in.csv".to_string(),
                output_csv_path: "/tmp/out.csv".to_string(),
            },
            &[AgentJobItemCreateParams {
                item_id: "item-1".to_string(),
                row_index: 0,
                source_id: None,
                row_json: json!({"path": "file-1"}),
            }],
        )
        .await?;
    runtime.mark_agent_job_running("job-1").await?;
    runtime
        .mark_agent_job_item_running_with_thread("job-1", "item-1", &child_thread_id.to_string())
        .await?;

    let rows = runtime
        .delete_threads_strict(&[thread_id, child_thread_id])
        .await?;

    assert_eq!(rows, 1);
    assert!(runtime.get_thread(thread_id).await?.is_none());
    let dynamic_tool_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM thread_dynamic_tools WHERE thread_id = ?")
            .bind(thread_id.to_string())
            .fetch_one(runtime.pool.as_ref())
            .await?;
    assert_eq!(dynamic_tool_count, 0);
    assert_thread_cleanup_state(&runtime, thread_id).await?;
    let job_item = runtime
        .get_agent_job_item("job-1", "item-1")
        .await?
        .expect("job item should exist");
    assert_eq!(job_item.status, AgentJobItemStatus::Pending);
    assert_eq!(job_item.assigned_thread_id, None);
    assert_eq!(
        job_item.last_error,
        Some("assigned thread was deleted".to_string())
    );
    let job = runtime
        .get_agent_job("job-1")
        .await?
        .expect("job should exist");
    assert_eq!(job.status, AgentJobStatus::Cancelled);

    let missing_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000403")?;
    let missing_child_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000404")?;
    seed_thread_cleanup_state(&runtime, missing_thread_id, missing_child_thread_id).await?;

    assert_eq!(runtime.delete_thread(missing_thread_id).await?, 0);
    assert_thread_cleanup_state(&runtime, missing_thread_id).await?;
    Ok(())
}

#[tokio::test]
async fn delete_thread_keeps_retry_graph_on_cleanup_failure() -> Result<()> {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string()).await?;
    let thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000405")?;
    let child_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000406")?;
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.clone(),
        ))
        .await?;
    seed_thread_cleanup_state(&runtime, thread_id, child_thread_id).await?;

    runtime.logs_pool.close().await;
    runtime
        .delete_thread(thread_id)
        .await
        .expect_err("closed log db should fail deletion");

    assert!(runtime.get_thread(thread_id).await?.is_some());
    assert_eq!(
        runtime.list_thread_spawn_descendants(thread_id).await?,
        vec![child_thread_id]
    );
    Ok(())
}

#[tokio::test]
async fn delete_thread_keeps_auxiliary_state_when_a_transaction_cannot_start() -> Result<()> {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string()).await?;
    let thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000407")?;
    let child_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000408")?;
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.clone(),
        ))
        .await?;
    seed_thread_cleanup_state(&runtime, thread_id, child_thread_id).await?;

    runtime.memories().close().await;
    runtime
        .delete_thread(thread_id)
        .await
        .expect_err("closed memories db should fail deletion");

    assert!(runtime.get_thread(thread_id).await?.is_some());
    assert_eq!(
        runtime.list_thread_spawn_descendants(thread_id).await?,
        vec![child_thread_id]
    );
    assert!(
        runtime
            .thread_goals()
            .get_thread_goal(thread_id)
            .await?
            .is_some()
    );
    assert_eq!(
        runtime
            .query_logs(&LogQuery {
                thread_ids: vec![thread_id.to_string()],
                ..Default::default()
            })
            .await?
            .len(),
        1
    );
    Ok(())
}

async fn seed_thread_cleanup_state(
    runtime: &StateRuntime,
    thread_id: ThreadId,
    child_thread_id: ThreadId,
) -> Result<()> {
    runtime
        .upsert_thread_spawn_edge(
            thread_id,
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Closed,
        )
        .await?;
    runtime
        .thread_goals()
        .replace_thread_goal(
            thread_id,
            "test goal",
            crate::ThreadGoalStatus::Active,
            /*token_budget*/ None,
        )
        .await?;
    sqlx::query("INSERT INTO logs (ts, ts_nanos, level, target, feedback_log_body, thread_id) VALUES (1, 0, 'INFO', 'test', 'feedback log', ?)")
        .bind(thread_id.to_string())
        .execute(runtime.logs_pool.as_ref())
        .await?;
    Ok(())
}

async fn assert_thread_cleanup_state(runtime: &StateRuntime, thread_id: ThreadId) -> Result<()> {
    let spawn_edge_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM thread_spawn_edges WHERE parent_thread_id = ? OR child_thread_id = ?",
    )
    .bind(thread_id.to_string())
    .bind(thread_id.to_string())
    .fetch_one(runtime.pool.as_ref())
    .await?;
    assert_eq!(spawn_edge_count, 0);
    assert_eq!(
        runtime.thread_goals().get_thread_goal(thread_id).await?,
        None
    );
    let logs = runtime
        .query_logs(&LogQuery {
            thread_ids: vec![thread_id.to_string()],
            ..Default::default()
        })
        .await?;
    assert!(logs.is_empty());
    Ok(())
}

#[tokio::test]
async fn list_threads_updated_after_returns_oldest_changes_first() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let older_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000001").expect("valid thread id");
    let middle_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000002").expect("valid thread id");
    let newer_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000003").expect("valid thread id");
    let older_updated_at =
        DateTime::<Utc>::from_timestamp(1_700_000_100, 0).expect("valid older timestamp");
    let newer_updated_at =
        DateTime::<Utc>::from_timestamp(1_700_000_200, 0).expect("valid newer timestamp");

    for (thread_id, updated_at) in [
        (older_id, older_updated_at),
        (newer_id, newer_updated_at),
        (middle_id, newer_updated_at),
    ] {
        let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
        metadata.updated_at = updated_at;
        metadata.first_user_message = Some("hello".to_string());
        runtime
            .upsert_thread(&metadata)
            .await
            .expect("thread insert should succeed");
    }

    let anchor = Anchor {
        ts: older_updated_at,
        id: None,
    };
    let model_providers = ["test-provider".to_string()];
    let page = runtime
        .list_threads(
            /*page_size*/ 1,
            ThreadFilterOptions {
                archived_only: false,
                allowed_sources: &[],
                model_providers: Some(&model_providers),
                cwd_filters: None,
                section: ThreadSectionFilter::All,
                anchor: Some(&anchor),
                sort_key: SortKey::UpdatedAt,
                sort_direction: SortDirection::Asc,
                search_term: None,
            },
        )
        .await
        .expect("list should succeed");

    let ids = page.items.iter().map(|item| item.id).collect::<Vec<_>>();
    assert_eq!(ids, vec![newer_id]);
    assert_eq!(
        page.next_anchor,
        Some(Anchor {
            ts: DateTime::<Utc>::from_timestamp_millis(1_700_000_200_000).expect("valid timestamp"),
            id: None,
        })
    );

    let page = runtime
        .list_threads(
            /*page_size*/ 1,
            ThreadFilterOptions {
                archived_only: false,
                allowed_sources: &[],
                model_providers: Some(&model_providers),
                cwd_filters: None,
                section: ThreadSectionFilter::All,
                anchor: page.next_anchor.as_ref(),
                sort_key: SortKey::UpdatedAt,
                sort_direction: SortDirection::Asc,
                search_term: None,
            },
        )
        .await
        .expect("second page should succeed");

    let ids = page.items.iter().map(|item| item.id).collect::<Vec<_>>();
    assert_eq!(ids, vec![middle_id]);
    assert_eq!(page.next_anchor, None);
}

#[tokio::test]
async fn list_threads_filters_by_cwd() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let first_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000101").expect("valid thread id");
    let second_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000102").expect("valid thread id");
    let other_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000103").expect("valid thread id");
    let first_cwd = codex_home.join("first");
    let second_cwd = codex_home.join("second");
    let other_cwd = codex_home.join("other");

    for (thread_id, cwd, updated_at) in [
        (first_id, first_cwd.clone(), 1_700_000_100),
        (second_id, second_cwd.clone(), 1_700_000_300),
        (other_id, other_cwd, 1_700_000_500),
    ] {
        let mut metadata = test_thread_metadata(&codex_home, thread_id, cwd);
        metadata.updated_at =
            DateTime::<Utc>::from_timestamp(updated_at, 0).expect("valid timestamp");
        runtime
            .upsert_thread(&metadata)
            .await
            .expect("thread insert should succeed");
    }

    let cwd_filters = vec![first_cwd, second_cwd];
    let first_page = runtime
        .list_threads(
            /*page_size*/ 1,
            ThreadFilterOptions {
                archived_only: false,
                allowed_sources: &[],
                model_providers: None,
                cwd_filters: Some(cwd_filters.as_slice()),
                section: ThreadSectionFilter::All,
                anchor: None,
                sort_key: SortKey::UpdatedAt,
                sort_direction: SortDirection::Desc,
                search_term: None,
            },
        )
        .await
        .expect("list should succeed");

    let ids = first_page
        .items
        .iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![second_id]);
    assert_eq!(
        first_page.next_anchor,
        Some(Anchor {
            ts: DateTime::<Utc>::from_timestamp_millis(1_700_000_300_000).expect("valid timestamp"),
            id: None,
        })
    );

    let second_page = runtime
        .list_threads(
            /*page_size*/ 1,
            ThreadFilterOptions {
                archived_only: false,
                allowed_sources: &[],
                model_providers: None,
                cwd_filters: Some(cwd_filters.as_slice()),
                section: ThreadSectionFilter::All,
                anchor: first_page.next_anchor.as_ref(),
                sort_key: SortKey::UpdatedAt,
                sort_direction: SortDirection::Desc,
                search_term: None,
            },
        )
        .await
        .expect("second page should succeed");

    let ids = second_page
        .items
        .iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![first_id]);
    assert_eq!(second_page.next_anchor, None);

    let page = runtime
        .list_threads(
            /*page_size*/ 10,
            ThreadFilterOptions {
                archived_only: false,
                allowed_sources: &[],
                model_providers: None,
                cwd_filters: Some(&[]),
                section: ThreadSectionFilter::All,
                anchor: None,
                sort_key: SortKey::UpdatedAt,
                sort_direction: SortDirection::Desc,
                search_term: None,
            },
        )
        .await
        .expect("list with empty cwd filters should succeed");

    assert_eq!(page.items, Vec::new());
}

#[tokio::test]
async fn list_threads_uses_indexes_matching_cwd_filters() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home, "test-provider".to_string())
        .await
        .expect("state db should initialize");

    let model_providers = ["test-provider".to_string()];
    let cwd_filters = [
        PathBuf::from("/workspace/one"),
        PathBuf::from("/workspace/two"),
    ];
    let anchor = Anchor {
        ts: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("valid timestamp"),
        id: None,
    };
    for (sort_key, visible_index, cwd_index) in [
        (
            SortKey::CreatedAt,
            "idx_threads_visible_created_at_ms",
            "idx_threads_archived_cwd_created_at_ms",
        ),
        (
            SortKey::UpdatedAt,
            "idx_threads_visible_updated_at_ms",
            "idx_threads_archived_cwd_updated_at_ms",
        ),
        (
            SortKey::RecencyAt,
            "idx_threads_visible_recency_at_ms",
            "idx_threads_archived_cwd_recency_at_ms",
        ),
    ] {
        for (cwd_filters, anchor, expected_index, expect_temp_sort) in [
            (None, None, visible_index, false),
            (Some(&cwd_filters[..1]), None, cwd_index, false),
            (
                Some(&cwd_filters[..]),
                None,
                "idx_threads_archived_cwd_",
                true,
            ),
            (Some(&cwd_filters[..]), Some(&anchor), cwd_index, true),
        ] {
            let mut builder = QueryBuilder::<Sqlite>::new("EXPLAIN QUERY PLAN ");
            push_list_threads_query(
                &mut builder,
                ThreadFilterOptions {
                    archived_only: false,
                    allowed_sources: &[],
                    model_providers: Some(&model_providers),
                    cwd_filters,
                    section: ThreadSectionFilter::All,
                    anchor,
                    sort_key,
                    sort_direction: SortDirection::Desc,
                    search_term: None,
                },
                /*relation_filter*/ None,
                /*limit*/ 201,
            );
            let plan_details = builder
                .build()
                .fetch_all(runtime.pool.as_ref())
                .await
                .expect("query plan should load")
                .into_iter()
                .map(|row| row.get::<String, _>("detail"))
                .collect::<Vec<_>>();

            assert!(
                plan_details
                    .iter()
                    .any(|detail| detail.contains(expected_index)),
                "query plan did not use {expected_index}: {plan_details:?}"
            );
            assert_eq!(
                plan_details
                    .iter()
                    .any(|detail| detail.contains("TEMP B-TREE")),
                expect_temp_sort,
                "unexpected sorting plan: {plan_details:?}"
            );
        }
    }
}

#[tokio::test]
async fn list_threads_by_relation_filters_spawn_graph_with_keyset_pagination() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let parent_id = ThreadId::new();
    let first_child_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000001").expect("valid thread id");
    let second_child_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000002").expect("valid thread id");
    let grandchild_id = ThreadId::new();

    for (thread_id, created_at) in [
        (parent_id, 1_700_000_000),
        (first_child_id, 1_700_000_200),
        (second_child_id, 1_700_000_200),
        (grandchild_id, 1_700_000_300),
    ] {
        let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
        metadata.created_at =
            DateTime::<Utc>::from_timestamp(created_at, 0).expect("valid timestamp");
        metadata.updated_at = metadata.created_at;
        runtime
            .upsert_thread(&metadata)
            .await
            .expect("thread insert should succeed");
    }
    for (parent_thread_id, child_thread_id, status) in [
        (
            parent_id,
            first_child_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        ),
        (
            parent_id,
            second_child_id,
            DirectionalThreadSpawnEdgeStatus::Closed,
        ),
        (
            first_child_id,
            grandchild_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        ),
    ] {
        runtime
            .upsert_thread_spawn_edge(parent_thread_id, child_thread_id, status)
            .await
            .expect("spawn edge insert should succeed");
    }

    let mut builder = QueryBuilder::<Sqlite>::new("EXPLAIN QUERY PLAN ");
    push_list_threads_query(
        &mut builder,
        ThreadFilterOptions {
            archived_only: false,
            allowed_sources: &[],
            model_providers: None,
            cwd_filters: None,
            section: ThreadSectionFilter::All,
            anchor: None,
            sort_key: SortKey::CreatedAt,
            sort_direction: SortDirection::Desc,
            search_term: None,
        },
        Some(crate::ThreadRelationFilter::DescendantsOf(parent_id)),
        /*limit*/ 10,
    );
    let plan_details = builder
        .build()
        .fetch_all(runtime.pool.as_ref())
        .await
        .expect("relationship query plan should load")
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>();
    assert!(
        plan_details
            .iter()
            .any(|detail| detail.contains("idx_thread_spawn_edges_parent_status")),
        "spawn relationship query did not use the parent index: {plan_details:?}"
    );

    let filters = |anchor| ThreadFilterOptions {
        archived_only: false,
        allowed_sources: &[],
        model_providers: None,
        cwd_filters: None,
        section: ThreadSectionFilter::All,
        anchor,
        sort_key: SortKey::CreatedAt,
        sort_direction: SortDirection::Desc,
        search_term: None,
    };
    let first_page = runtime
        .list_threads_by_parent(/*page_size*/ 1, parent_id, filters(None))
        .await
        .expect("first page should succeed");
    let second_page = runtime
        .list_threads_by_parent(
            /*page_size*/ 1,
            parent_id,
            filters(first_page.next_anchor.as_ref()),
        )
        .await
        .expect("second page should succeed");

    assert_eq!(
        first_page
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![second_child_id]
    );
    assert_eq!(
        second_page
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![first_child_id]
    );
    assert_eq!(second_page.next_anchor, None);

    let first_descendant_page = runtime
        .list_threads_by_relation(
            /*page_size*/ 2,
            crate::ThreadRelationFilter::DescendantsOf(parent_id),
            filters(None),
        )
        .await
        .expect("first descendant page should succeed");
    let second_descendant_page = runtime
        .list_threads_by_relation(
            /*page_size*/ 2,
            crate::ThreadRelationFilter::DescendantsOf(parent_id),
            filters(first_descendant_page.next_anchor.as_ref()),
        )
        .await
        .expect("second descendant page should succeed");
    assert_eq!(
        (
            first_descendant_page
                .items
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            second_descendant_page
                .items
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            first_descendant_page.parent_thread_ids,
            second_descendant_page.parent_thread_ids,
            second_descendant_page.next_anchor,
        ),
        (
            vec![grandchild_id, second_child_id],
            vec![first_child_id],
            [
                (grandchild_id, first_child_id),
                (second_child_id, parent_id)
            ]
            .into(),
            [(first_child_id, parent_id)].into(),
            None,
        )
    );

    runtime
        .upsert_thread_spawn_edge(
            grandchild_id,
            parent_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("cycle-closing spawn edge insert should succeed");
    let cyclic_descendants = runtime
        .list_threads_by_relation(
            /*page_size*/ 10,
            crate::ThreadRelationFilter::DescendantsOf(parent_id),
            filters(None),
        )
        .await
        .expect("cyclic descendant graph should terminate");
    assert_eq!(
        cyclic_descendants
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![grandchild_id, second_child_id, first_child_id]
    );
}

#[tokio::test]
async fn apply_rollout_items_restores_memory_mode_from_session_meta() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000456").expect("valid thread id");
    let metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());

    runtime
        .upsert_thread(&metadata)
        .await
        .expect("initial upsert should succeed");

    let builder = ThreadMetadataBuilder::new(
        thread_id,
        metadata.rollout_path.clone(),
        metadata.created_at,
        SessionSource::Cli,
    );
    let items = vec![RolloutItem::SessionMeta(SessionMetaLine {
        meta: SessionMeta {
            session_id: thread_id.into(),
            id: thread_id,
            rollout_id: None,
            forked_from_id: None,
            parent_thread_id: None,
            timestamp: metadata.created_at.to_rfc3339(),
            cwd: PathBuf::new(),
            originator: String::new(),
            cli_version: String::new(),
            source: SessionSource::Cli,
            thread_source: None,
            agent_path: None,
            agent_nickname: None,
            agent_role: None,
            model_provider: None,
            base_instructions: None,
            dynamic_tools: None,
            selected_capability_roots: Vec::new(),
            memory_mode: Some("polluted".to_string()),
            history_mode: Default::default(),
            history_base: None,
            subagent_history_start_ordinal: None,
            multi_agent_version: None,
            context_window: None,
        },
        git: None,
    })];

    runtime
        .apply_rollout_items(
            &builder, &items, /*new_thread_memory_mode*/ None,
            /*updated_at_override*/ None,
        )
        .await
        .expect("apply_rollout_items should succeed");

    let memory_mode = runtime
        .get_thread_memory_mode(thread_id)
        .await
        .expect("memory mode should load");
    assert_eq!(memory_mode.as_deref(), Some("polluted"));
}

#[tokio::test]
async fn apply_rollout_items_preserves_existing_git_branch_and_fills_missing_git_fields() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000457").expect("valid thread id");
    let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
    metadata.git_branch = Some("sqlite-branch".to_string());

    runtime
        .upsert_thread(&metadata)
        .await
        .expect("initial upsert should succeed");

    let created_at = metadata.created_at.to_rfc3339();
    let builder = ThreadMetadataBuilder::new(
        thread_id,
        metadata.rollout_path.clone(),
        metadata.created_at,
        SessionSource::Cli,
    );
    let items = vec![RolloutItem::SessionMeta(SessionMetaLine {
        meta: SessionMeta {
            session_id: thread_id.into(),
            id: thread_id,
            rollout_id: None,
            forked_from_id: None,
            parent_thread_id: None,
            timestamp: created_at,
            cwd: PathBuf::new(),
            originator: String::new(),
            cli_version: String::new(),
            source: SessionSource::Cli,
            thread_source: None,
            agent_path: None,
            agent_nickname: None,
            agent_role: None,
            model_provider: None,
            base_instructions: None,
            dynamic_tools: None,
            selected_capability_roots: Vec::new(),
            memory_mode: None,
            history_mode: Default::default(),
            history_base: None,
            subagent_history_start_ordinal: None,
            multi_agent_version: None,
            context_window: None,
        },
        git: Some(GitInfo {
            commit_hash: Some(codex_git_utils::GitSha::new("rollout-sha")),
            branch: Some("rollout-branch".to_string()),
            repository_url: Some("git@example.com:openai/codex.git".to_string()),
        }),
    })];

    runtime
        .apply_rollout_items(
            &builder, &items, /*new_thread_memory_mode*/ None,
            /*updated_at_override*/ None,
        )
        .await
        .expect("apply_rollout_items should succeed");

    let persisted = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(persisted.git_sha.as_deref(), Some("rollout-sha"));
    assert_eq!(persisted.git_branch.as_deref(), Some("sqlite-branch"));
    assert_eq!(
        persisted.git_origin_url.as_deref(),
        Some("git@example.com:openai/codex.git")
    );
}

#[tokio::test]
async fn upsert_thread_preserves_existing_git_fields_atomically() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000458").expect("valid thread id");
    let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
    metadata.git_sha = Some("sqlite-sha".to_string());
    metadata.git_branch = Some("sqlite-branch".to_string());
    metadata.git_origin_url = Some("git@example.com:openai/codex.git".to_string());

    runtime
        .upsert_thread(&metadata)
        .await
        .expect("initial upsert should succeed");

    let mut rollout_metadata = metadata.clone();
    rollout_metadata.git_sha = Some("rollout-sha".to_string());
    rollout_metadata.git_branch = Some("rollout-branch".to_string());
    rollout_metadata.git_origin_url = Some("https://example.com/repo.git".to_string());

    runtime
        .upsert_thread(&rollout_metadata)
        .await
        .expect("rollout upsert should succeed");

    let persisted = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(persisted.git_sha.as_deref(), Some("sqlite-sha"));
    assert_eq!(persisted.git_branch.as_deref(), Some("sqlite-branch"));
    assert_eq!(
        persisted.git_origin_url.as_deref(),
        Some("git@example.com:openai/codex.git")
    );
}

#[tokio::test]
async fn upsert_thread_preserves_existing_preview_when_incoming_preview_is_empty() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000459").expect("valid thread id");
    let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
    metadata.first_user_message = None;
    metadata.preview = Some("migrated goal preview".to_string());

    runtime
        .upsert_thread(&metadata)
        .await
        .expect("initial upsert should succeed");

    let mut rollout_metadata = metadata.clone();
    rollout_metadata.preview = None;

    runtime
        .upsert_thread(&rollout_metadata)
        .await
        .expect("rollout upsert should succeed");

    let persisted = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(persisted.preview.as_deref(), Some("migrated goal preview"));
}

#[tokio::test]
async fn set_thread_preview_if_empty_only_fills_blank_preview() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000460").expect("valid thread id");
    let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
    metadata.first_user_message = None;
    metadata.preview = None;

    runtime
        .upsert_thread(&metadata)
        .await
        .expect("initial upsert should succeed");

    let empty_updated = runtime
        .set_thread_preview_if_empty(thread_id, "  ")
        .await
        .expect("empty preview update should succeed");
    assert!(!empty_updated);
    let goal_updated = runtime
        .set_thread_preview_if_empty(thread_id, "  goal preview  ")
        .await
        .expect("goal preview update should succeed");
    assert!(goal_updated);
    let overwrite_updated = runtime
        .set_thread_preview_if_empty(thread_id, "new preview")
        .await
        .expect("overwrite preview update should succeed");
    assert!(!overwrite_updated);

    let persisted = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(persisted.preview.as_deref(), Some("goal preview"));
}

#[tokio::test]
async fn update_thread_git_info_preserves_newer_non_git_metadata() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000789").expect("valid thread id");
    let metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());

    runtime
        .upsert_thread(&metadata)
        .await
        .expect("initial upsert should succeed");

    let updated_at = datetime_to_epoch_millis(
        DateTime::<Utc>::from_timestamp(1_700_000_100, 0).expect("timestamp"),
    );
    sqlx::query(
        "UPDATE threads SET updated_at = ?, updated_at_ms = ?, tokens_used = ?, first_user_message = ?, preview = ? WHERE id = ?",
    )
    .bind(updated_at / 1000)
    .bind(updated_at)
    .bind(123_i64)
    .bind("newer preview")
    .bind("newer preview")
    .bind(thread_id.to_string())
    .execute(runtime.pool.as_ref())
    .await
    .expect("concurrent metadata write should succeed");

    let updated = runtime
        .update_thread_git_info(
            thread_id,
            Some(Some("abc123")),
            Some(Some("feature/branch")),
            Some(Some("git@example.com:openai/codex.git")),
        )
        .await
        .expect("git info update should succeed");
    assert!(updated, "git info update should touch the thread row");

    let persisted = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(persisted.tokens_used, 123);
    assert_eq!(
        persisted.first_user_message.as_deref(),
        Some("newer preview")
    );
    assert_eq!(persisted.preview.as_deref(), Some("newer preview"));
    assert_eq!(datetime_to_epoch_millis(persisted.updated_at), updated_at);
    assert_eq!(persisted.git_sha.as_deref(), Some("abc123"));
    assert_eq!(persisted.git_branch.as_deref(), Some("feature/branch"));
    assert_eq!(
        persisted.git_origin_url.as_deref(),
        Some("git@example.com:openai/codex.git")
    );
}

#[tokio::test]
async fn insert_thread_if_absent_preserves_existing_metadata() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000791").expect("valid thread id");

    let mut existing = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
    existing.tokens_used = 123;
    existing.first_user_message = Some("newer preview".to_string());
    existing.preview = Some("newer preview".to_string());
    existing.updated_at = DateTime::<Utc>::from_timestamp(1_700_000_100, 0).expect("timestamp");
    runtime
        .upsert_thread(&existing)
        .await
        .expect("initial upsert should succeed");

    let mut fallback = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
    fallback.tokens_used = 0;
    fallback.first_user_message = None;
    fallback.preview = None;
    fallback.updated_at = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("timestamp");

    let inserted = runtime
        .insert_thread_if_absent(&fallback)
        .await
        .expect("insert should succeed");
    assert!(!inserted, "existing rows should not be overwritten");

    let persisted = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(persisted.tokens_used, 123);
    assert_eq!(
        persisted.first_user_message.as_deref(),
        Some("newer preview")
    );
    assert_eq!(persisted.preview.as_deref(), Some("newer preview"));
    assert_eq!(
        datetime_to_epoch_millis(persisted.updated_at),
        datetime_to_epoch_millis(existing.updated_at)
    );
}

#[tokio::test]
async fn update_thread_git_info_can_clear_fields() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000790").expect("valid thread id");
    let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
    metadata.git_sha = Some("abc123".to_string());
    metadata.git_branch = Some("feature/branch".to_string());
    metadata.git_origin_url = Some("git@example.com:openai/codex.git".to_string());

    runtime
        .upsert_thread(&metadata)
        .await
        .expect("initial upsert should succeed");

    let updated = runtime
        .update_thread_git_info(thread_id, Some(None), Some(None), Some(None))
        .await
        .expect("git info clear should succeed");
    assert!(updated, "git info clear should touch the thread row");

    let persisted = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(persisted.git_sha, None);
    assert_eq!(persisted.git_branch, None);
    assert_eq!(persisted.git_origin_url, None);
}

#[tokio::test]
async fn touch_thread_updated_at_updates_only_updated_at() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000791").expect("valid thread id");
    let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
    metadata.title = "original title".to_string();
    metadata.first_user_message = Some("first-user-message".to_string());
    metadata.preview = None;

    runtime
        .upsert_thread(&metadata)
        .await
        .expect("initial upsert should succeed");

    let touched_at = DateTime::<Utc>::from_timestamp(1_700_001_111, 0).expect("timestamp");
    let touched = runtime
        .touch_thread_updated_at(thread_id, touched_at)
        .await
        .expect("touch should succeed");
    assert!(touched);

    let persisted = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(persisted.updated_at, touched_at);
    assert_eq!(persisted.title, "original title");
    assert_eq!(
        persisted.first_user_message.as_deref(),
        Some("first-user-message")
    );
    assert_eq!(persisted.preview.as_deref(), Some("first-user-message"));
}

#[tokio::test]
async fn touch_thread_recency_at_is_monotonic_and_survives_stale_upsert() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000792").expect("valid thread id");
    let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
    let original_recency_at = metadata.recency_at;
    runtime
        .upsert_thread(&metadata)
        .await
        .expect("initial upsert should succeed");

    let touched_at = DateTime::<Utc>::from_timestamp_millis(1_700_001_111_123).expect("timestamp");
    assert!(
        runtime
            .touch_thread_recency_at(thread_id, touched_at)
            .await
            .expect("touch should succeed")
    );

    metadata.updated_at =
        DateTime::<Utc>::from_timestamp_millis(1_700_001_222_456).expect("timestamp");
    metadata.title = "updated metadata".to_string();
    assert_eq!(metadata.recency_at, original_recency_at);
    runtime
        .upsert_thread(&metadata)
        .await
        .expect("stale metadata upsert should succeed");

    let persisted = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(persisted.recency_at, touched_at);
    assert_eq!(persisted.updated_at, metadata.updated_at);
    assert_eq!(persisted.title, "updated metadata");

    assert!(
        runtime
            .touch_thread_recency_at(thread_id, original_recency_at)
            .await
            .expect("older touch should succeed")
    );
    let persisted = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(
        datetime_to_epoch_millis(persisted.recency_at),
        datetime_to_epoch_millis(touched_at) + 1
    );
}

#[tokio::test]
async fn list_threads_orders_and_pages_by_recency_at() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let first_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000793").expect("valid thread id");
    let second_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000794").expect("valid thread id");
    let third_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000795").expect("valid thread id");
    let recency_at = DateTime::<Utc>::from_timestamp_millis(1_700_002_000_456).expect("timestamp");

    for thread_id in [first_id, second_id, third_id] {
        let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
        metadata.recency_at = recency_at;
        runtime
            .upsert_thread(&metadata)
            .await
            .expect("thread insert should succeed");
    }
    sqlx::query("UPDATE threads SET recency_at = ?, recency_at_ms = ?")
        .bind(datetime_to_epoch_seconds(recency_at))
        .bind(datetime_to_epoch_millis(recency_at))
        .execute(runtime.pool.as_ref())
        .await
        .expect("recency timestamps should match");

    let first_page = runtime
        .list_threads(
            /*page_size*/ 1,
            ThreadFilterOptions {
                archived_only: false,
                allowed_sources: &[],
                model_providers: None,
                cwd_filters: None,
                section: ThreadSectionFilter::All,
                anchor: None,
                sort_key: SortKey::RecencyAt,
                sort_direction: SortDirection::Desc,
                search_term: None,
            },
        )
        .await
        .expect("list should succeed");
    assert_eq!(
        first_page
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![third_id]
    );
    assert_eq!(
        first_page.next_anchor,
        Some(Anchor {
            ts: recency_at,
            id: Some(third_id),
        })
    );

    let second_page = runtime
        .list_threads(
            /*page_size*/ 1,
            ThreadFilterOptions {
                archived_only: false,
                allowed_sources: &[],
                model_providers: None,
                cwd_filters: None,
                section: ThreadSectionFilter::All,
                anchor: first_page.next_anchor.as_ref(),
                sort_key: SortKey::RecencyAt,
                sort_direction: SortDirection::Desc,
                search_term: None,
            },
        )
        .await
        .expect("second list should succeed");
    assert_eq!(
        second_page
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![second_id]
    );
    assert_eq!(
        second_page.next_anchor,
        Some(Anchor {
            ts: recency_at,
            id: Some(second_id),
        })
    );

    let third_page = runtime
        .list_threads(
            /*page_size*/ 1,
            ThreadFilterOptions {
                archived_only: false,
                allowed_sources: &[],
                model_providers: None,
                cwd_filters: None,
                section: ThreadSectionFilter::All,
                anchor: second_page.next_anchor.as_ref(),
                sort_key: SortKey::RecencyAt,
                sort_direction: SortDirection::Desc,
                search_term: None,
            },
        )
        .await
        .expect("third list should succeed");
    assert_eq!(
        third_page
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![first_id]
    );
    assert_eq!(third_page.next_anchor, None);
}

#[tokio::test]
async fn thread_updated_at_uses_unique_epoch_millis_and_reads_legacy_seconds() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let first_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000901").expect("valid thread id");
    let second_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000902").expect("valid thread id");
    let older_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000903").expect("valid thread id");
    let updated_at =
        DateTime::<Utc>::from_timestamp_millis(1_700_001_111_123).expect("timestamp millis");
    let mut first = test_thread_metadata(&codex_home, first_id, codex_home.clone());
    first.updated_at = updated_at;
    first.recency_at = updated_at;
    let mut second = test_thread_metadata(&codex_home, second_id, codex_home.clone());
    second.updated_at = updated_at;
    second.recency_at = updated_at;

    runtime
        .upsert_thread(&first)
        .await
        .expect("first upsert should succeed");
    runtime
        .upsert_thread(&second)
        .await
        .expect("second upsert should succeed");

    let first = runtime
        .get_thread(first_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    let second = runtime
        .get_thread(second_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(
        datetime_to_epoch_millis(first.updated_at),
        1_700_001_111_123
    );
    assert_eq!(
        datetime_to_epoch_millis(second.updated_at),
        1_700_001_111_124
    );
    assert_eq!(
        datetime_to_epoch_millis(first.recency_at),
        1_700_001_111_123
    );
    assert_eq!(
        datetime_to_epoch_millis(second.recency_at),
        1_700_001_111_124
    );
    let second_row: (i64, i64, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT created_at, updated_at, created_at_ms, updated_at_ms FROM threads WHERE id = ?",
    )
    .bind(second_id.to_string())
    .fetch_one(runtime.pool.as_ref())
    .await
    .expect("thread timestamp row should load");
    assert_eq!(
        second_row,
        (
            datetime_to_epoch_seconds(second.created_at),
            1_700_001_111,
            Some(datetime_to_epoch_millis(second.created_at)),
            Some(1_700_001_111_124)
        )
    );

    let older_updated_at =
        DateTime::<Utc>::from_timestamp_millis(1_700_001_100_123).expect("timestamp millis");
    let mut older = test_thread_metadata(&codex_home, older_id, codex_home.clone());
    older.updated_at = older_updated_at;
    runtime
        .upsert_thread(&older)
        .await
        .expect("older upsert should succeed");
    let older = runtime
        .get_thread(older_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(
        datetime_to_epoch_millis(older.updated_at),
        1_700_001_100_123
    );

    sqlx::query("UPDATE threads SET updated_at = ? WHERE id = ?")
        .bind(1_700_001_112_i64)
        .bind(first_id.to_string())
        .execute(runtime.pool.as_ref())
        .await
        .expect("legacy timestamp write should succeed");
    let legacy = runtime
        .get_thread(first_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(
        datetime_to_epoch_millis(legacy.updated_at),
        1_700_001_112_000
    );
}

#[tokio::test]
async fn apply_rollout_items_uses_override_updated_at_when_provided() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000792").expect("valid thread id");
    let metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());

    runtime
        .upsert_thread(&metadata)
        .await
        .expect("initial upsert should succeed");

    let builder = ThreadMetadataBuilder::new(
        thread_id,
        metadata.rollout_path.clone(),
        metadata.created_at,
        SessionSource::Cli,
    );
    let items = vec![RolloutItem::EventMsg(EventMsg::TokenCount(
        codex_protocol::protocol::TokenCountEvent {
            info: Some(codex_protocol::protocol::TokenUsageInfo {
                total_token_usage: codex_protocol::protocol::TokenUsage {
                    input_tokens: 0,
                    cached_input_tokens: 0,
                    cache_write_input_tokens: 0,
                    output_tokens: 0,
                    reasoning_output_tokens: 0,
                    total_tokens: 321,
                },
                last_token_usage: codex_protocol::protocol::TokenUsage::default(),
                model_context_window: None,
            }),
            rate_limits: None,
        },
    ))];
    let override_updated_at = DateTime::<Utc>::from_timestamp(1_700_001_234, 0).expect("timestamp");

    runtime
        .apply_rollout_items(
            &builder,
            &items,
            /*new_thread_memory_mode*/ None,
            Some(override_updated_at),
        )
        .await
        .expect("apply_rollout_items should succeed");

    let persisted = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(persisted.tokens_used, 321);
    assert_eq!(persisted.updated_at, override_updated_at);
}

#[tokio::test]
async fn thread_spawn_edges_track_directional_status() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home, "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let parent_thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000900").expect("valid thread id");
    let child_thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000901").expect("valid thread id");
    let grandchild_thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000902").expect("valid thread id");

    runtime
        .upsert_thread_spawn_edge(
            parent_thread_id,
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("child edge insert should succeed");
    runtime
        .upsert_thread_spawn_edge(
            child_thread_id,
            grandchild_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("grandchild edge insert should succeed");

    let children = runtime
        .list_thread_spawn_children_with_status(
            parent_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("open child list should load");
    assert_eq!(children, vec![child_thread_id]);

    let descendants = runtime
        .list_thread_spawn_descendants_with_status(
            parent_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("open descendants should load");
    assert_eq!(descendants, vec![child_thread_id, grandchild_thread_id]);

    runtime
        .set_thread_spawn_edge_status(child_thread_id, DirectionalThreadSpawnEdgeStatus::Closed)
        .await
        .expect("edge close should succeed");

    let open_children = runtime
        .list_thread_spawn_children_with_status(
            parent_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("open child list should load");
    assert_eq!(open_children, Vec::<ThreadId>::new());

    let closed_children = runtime
        .list_thread_spawn_children_with_status(
            parent_thread_id,
            DirectionalThreadSpawnEdgeStatus::Closed,
        )
        .await
        .expect("closed child list should load");
    assert_eq!(closed_children, vec![child_thread_id]);

    let closed_descendants = runtime
        .list_thread_spawn_descendants_with_status(
            parent_thread_id,
            DirectionalThreadSpawnEdgeStatus::Closed,
        )
        .await
        .expect("closed descendants should load");
    assert_eq!(closed_descendants, vec![child_thread_id]);

    let open_descendants_from_child = runtime
        .list_thread_spawn_descendants_with_status(
            child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("open descendants from child should load");
    assert_eq!(open_descendants_from_child, vec![grandchild_thread_id]);

    let all_descendants = runtime
        .list_thread_spawn_descendants(parent_thread_id)
        .await
        .expect("all descendants should load");
    assert_eq!(all_descendants, vec![child_thread_id, grandchild_thread_id]);
}

#[tokio::test]
async fn thread_spawn_children_without_status_filter_lists_all_statuses() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home, "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let parent_thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000910").expect("valid thread id");
    let open_child_thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000911").expect("valid thread id");
    let closed_child_thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000912").expect("valid thread id");
    let future_child_thread_id =
        ThreadId::from_string("00000000-0000-0000-0000-000000000913").expect("valid thread id");

    runtime
        .upsert_thread_spawn_edge(
            parent_thread_id,
            open_child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("open child edge insert should succeed");
    runtime
        .upsert_thread_spawn_edge(
            parent_thread_id,
            closed_child_thread_id,
            DirectionalThreadSpawnEdgeStatus::Closed,
        )
        .await
        .expect("closed child edge insert should succeed");
    sqlx::query(
        r#"
INSERT INTO thread_spawn_edges (
parent_thread_id,
child_thread_id,
status
) VALUES (?, ?, ?)
        "#,
    )
    .bind(parent_thread_id.to_string())
    .bind(future_child_thread_id.to_string())
    .bind("future")
    .execute(runtime.pool.as_ref())
    .await
    .expect("future-status child edge insert should succeed");

    let children = runtime
        .list_thread_spawn_children(parent_thread_id)
        .await
        .expect("all children should load");
    assert_eq!(
        children,
        vec![
            open_child_thread_id,
            closed_child_thread_id,
            future_child_thread_id,
        ]
    );
}
