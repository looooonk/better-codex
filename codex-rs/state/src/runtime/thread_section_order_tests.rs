use super::*;
use crate::MAX_THREAD_SECTION_ORDERING_IDS;
use crate::SortDirection;
use crate::SortKey;
use crate::ThreadFilterOptions;
use crate::ThreadSectionFilter;
use crate::runtime::test_support::test_thread_metadata;
use crate::runtime::test_support::unique_temp_dir;
use pretty_assertions::assert_eq;
use std::path::Path;

async fn insert_test_thread(
    runtime: &StateRuntime,
    codex_home: &Path,
    thread_id: ThreadId,
) {
    runtime
        .upsert_thread(&test_thread_metadata(
            codex_home,
            thread_id,
            codex_home.to_path_buf(),
        ))
        .await
        .expect("thread should insert");
}

#[tokio::test]
async fn section_moves_are_sparse_ordered_and_invalid_moves_roll_back() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let section = runtime
        .create_thread_section("Projects", /*appearance*/ None)
        .await
        .expect("section should create");
    let first =
        ThreadId::from_string("00000000-0000-0000-0000-000000000071").expect("valid thread id");
    let second =
        ThreadId::from_string("00000000-0000-0000-0000-000000000072").expect("valid thread id");
    let middle =
        ThreadId::from_string("00000000-0000-0000-0000-000000000073").expect("valid thread id");
    let unsectioned =
        ThreadId::from_string("00000000-0000-0000-0000-000000000074").expect("valid thread id");
    for thread_id in [first, second, middle, unsectioned] {
        runtime
            .upsert_thread(&test_thread_metadata(
                &codex_home,
                thread_id,
                codex_home.clone(),
            ))
            .await
            .expect("thread should insert");
    }

    for thread_id in [first, second] {
        assert!(
            runtime
                .move_thread_to_section(
                    thread_id,
                    ThreadSectionMove::Append {
                        section_id: &section.id,
                    },
                )
                .await
                .expect("thread should append")
        );
    }
    assert!(
        runtime
            .move_thread_to_section(
                middle,
                ThreadSectionMove::Before {
                    section_id: &section.id,
                    before_thread_id: second,
                },
            )
            .await
            .expect("thread should insert before another")
    );

    let ordering = runtime
        .get_thread_section_ordering(&[first, middle, second, unsectioned])
        .await
        .expect("ordering should load");
    let first_order = ordering[&first];
    let middle_order = ordering[&middle];
    let second_order = ordering[&second];
    assert!(first_order.0 < middle_order.0 && middle_order.0 < second_order.0);
    assert!(
        [first_order.1, middle_order.1, second_order.1]
            .into_iter()
            .all(|entered_at| entered_at.is_some())
    );
    assert_eq!(ordering[&unsectioned], (None, None));

    for destination in [
        ThreadSectionMove::Before {
            section_id: &section.id,
            before_thread_id: unsectioned,
        },
        ThreadSectionMove::Before {
            section_id: &section.id,
            before_thread_id: middle,
        },
    ] {
        assert!(
            runtime
                .move_thread_to_section(middle, destination)
                .await
                .is_err()
        );
    }
    assert!(
        runtime
            .move_thread_to_section(
                unsectioned,
                ThreadSectionMove::Append {
                    section_id: "missing-section",
                },
            )
            .await
            .is_err()
    );
    assert_eq!(
        runtime
            .get_thread_section_ordering(&[first, middle, second, unsectioned])
            .await
            .expect("ordering should remain readable"),
        ordering,
        "failed moves must not partially update section state"
    );

    assert!(
        runtime
            .move_thread_to_section(
                middle,
                ThreadSectionMove::Before {
                    section_id: &section.id,
                    before_thread_id: first,
                },
            )
            .await
            .expect("existing member should reorder")
    );
    let reordered = runtime
        .get_thread_section_ordering(&[middle])
        .await
        .expect("reordered state should load");
    assert_eq!(reordered[&middle].1, middle_order.1);

    let page = runtime
        .list_threads(
            /*page_size*/ 10,
            ThreadFilterOptions {
                archived_only: false,
                allowed_sources: &[],
                model_providers: None,
                cwd_filters: None,
                section: ThreadSectionFilter::Section(&section.id),
                anchor: None,
                sort_key: SortKey::SectionPosition,
                sort_direction: SortDirection::Asc,
                search_term: None,
            },
        )
        .await
        .expect("section should list in manual order");
    assert_eq!(
        page.items
            .into_iter()
            .map(|thread| thread.id)
            .collect::<Vec<_>>(),
        vec![middle, first, second]
    );

    assert!(
        runtime
            .move_thread_to_section(first, ThreadSectionMove::Clear)
            .await
            .expect("thread should leave section")
    );
    assert_eq!(
        runtime
            .get_thread_section_ordering(&[first])
            .await
            .expect("cleared ordering should load")[&first],
        (None, None)
    );
}

#[tokio::test]
async fn section_ordering_batch_enforces_size_and_uniqueness_bounds() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home, "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let thread_ids = (0..MAX_THREAD_SECTION_ORDERING_IDS)
        .map(|_| ThreadId::new())
        .collect::<Vec<_>>();
    assert!(
        runtime
            .get_thread_section_ordering(&thread_ids)
            .await
            .expect("maximum ordering batch should load")
            .is_empty()
    );

    let mut oversized = thread_ids;
    oversized.push(ThreadId::new());
    assert_eq!(
        runtime
            .get_thread_section_ordering(&oversized)
            .await
            .expect_err("oversized ordering batch must fail")
            .to_string(),
        format!(
            "thread section ordering batch exceeds limit of {MAX_THREAD_SECTION_ORDERING_IDS}; got {}",
            oversized.len()
        )
    );

    let duplicate = ThreadId::new();
    assert_eq!(
        runtime
            .get_thread_section_ordering(&[duplicate, duplicate])
            .await
            .expect_err("duplicate ordering id must fail")
            .to_string(),
        format!("duplicate thread id in section ordering batch: {duplicate}")
    );
}

#[tokio::test]
async fn concurrent_section_appends_assign_unique_positions() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let section = runtime
        .create_thread_section("Concurrent", /*appearance*/ None)
        .await
        .expect("section should create");
    let first = ThreadId::new();
    let second = ThreadId::new();
    insert_test_thread(&runtime, &codex_home, first).await;
    insert_test_thread(&runtime, &codex_home, second).await;

    let (first_move, second_move) = tokio::join!(
        runtime.move_thread_to_section(
            first,
            ThreadSectionMove::Append {
                section_id: &section.id,
            },
        ),
        runtime.move_thread_to_section(
            second,
            ThreadSectionMove::Append {
                section_id: &section.id,
            },
        )
    );
    assert!(first_move.expect("first append should finish"));
    assert!(second_move.expect("second append should finish"));
    let ordering = runtime
        .get_thread_section_ordering(&[first, second])
        .await
        .expect("concurrent order should load");
    assert!(ordering[&first].0.is_some());
    assert!(ordering[&second].0.is_some());
    assert_ne!(ordering[&first].0, ordering[&second].0);
}

#[tokio::test]
async fn concurrent_section_delete_and_move_leave_no_dangling_order() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let section = runtime
        .create_thread_section("Transient", /*appearance*/ None)
        .await
        .expect("section should create");
    let thread_id = ThreadId::new();
    insert_test_thread(&runtime, &codex_home, thread_id).await;

    let (move_result, delete_result) = tokio::join!(
        runtime.move_thread_to_section(
            thread_id,
            ThreadSectionMove::Append {
                section_id: &section.id,
            },
        ),
        runtime.delete_thread_section(&section.id)
    );
    assert!(delete_result.expect("section delete should finish"));
    match move_result {
        Ok(moved) => assert!(moved),
        Err(error) => assert_eq!(
            error.to_string(),
            format!("section {} does not exist", section.id)
        ),
    }
    assert_eq!(
        runtime
            .get_thread_section(&section.id)
            .await
            .expect("deleted section lookup should succeed"),
        None
    );
    let metadata = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should remain");
    assert_eq!(
        (
            metadata.section,
            metadata.section_position,
            metadata.section_entered_at,
        ),
        (None, None, None)
    );
}

#[tokio::test]
async fn exhausted_section_rank_space_is_renumbered_before_append() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let section = runtime
        .create_thread_section("Full", /*appearance*/ None)
        .await
        .expect("section should create");
    let first = ThreadId::new();
    let second = ThreadId::new();
    insert_test_thread(&runtime, &codex_home, first).await;
    insert_test_thread(&runtime, &codex_home, second).await;
    assert!(
        runtime
            .move_thread_to_section(
                first,
                ThreadSectionMove::Append {
                    section_id: &section.id,
                },
            )
            .await
            .expect("first thread should append")
    );
    sqlx::query("UPDATE threads SET section_position = ? WHERE id = ?")
        .bind(i64::MAX)
        .bind(first.to_string())
        .execute(runtime.pool.as_ref())
        .await
        .expect("rank should be exhausted");

    assert!(
        runtime
            .move_thread_to_section(
                second,
                ThreadSectionMove::Append {
                    section_id: &section.id,
                },
            )
            .await
            .expect("append should renumber exhausted ranks")
    );
    let ordering = runtime
        .get_thread_section_ordering(&[first, second])
        .await
        .expect("renumbered ordering should load");
    assert_eq!(
        (ordering[&first].0, ordering[&second].0),
        (Some(1_000_000), Some(2_000_000))
    );
}

#[tokio::test]
async fn cross_section_move_refreshes_entered_timestamp() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let first_section = runtime
        .create_thread_section("First", /*appearance*/ None)
        .await
        .expect("first section should create");
    let second_section = runtime
        .create_thread_section("Second", /*appearance*/ None)
        .await
        .expect("second section should create");
    let thread_id = ThreadId::new();
    insert_test_thread(&runtime, &codex_home, thread_id).await;
    assert!(
        runtime
            .move_thread_to_section(
                thread_id,
                ThreadSectionMove::Append {
                    section_id: &first_section.id,
                },
            )
            .await
            .expect("first move should succeed")
    );
    sqlx::query("UPDATE threads SET section_entered_at_ms = 1 WHERE id = ?")
        .bind(thread_id.to_string())
        .execute(runtime.pool.as_ref())
        .await
        .expect("entered timestamp should reset");

    assert!(
        runtime
            .move_thread_to_section(
                thread_id,
                ThreadSectionMove::Append {
                    section_id: &second_section.id,
                },
            )
            .await
            .expect("cross-section move should succeed")
    );
    let metadata = runtime
        .get_thread(thread_id)
        .await
        .expect("thread should load")
        .expect("thread should exist");
    assert_eq!(metadata.section, Some(second_section));
    assert!(
        metadata
            .section_entered_at
            .expect("entered timestamp should exist")
            .timestamp_millis()
            > 1
    );
}

#[tokio::test]
async fn moving_a_missing_thread_returns_false_without_section_changes() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home, "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let section = runtime
        .create_thread_section("Stable", /*appearance*/ None)
        .await
        .expect("section should create");
    let missing = ThreadId::new();
    assert_eq!(
        (
            runtime
                .move_thread_to_section(
                    missing,
                    ThreadSectionMove::Append {
                        section_id: &section.id,
                    },
                )
                .await
                .expect("missing append should not fail"),
            runtime
                .move_thread_to_section(missing, ThreadSectionMove::Clear)
                .await
                .expect("missing clear should not fail"),
        ),
        (false, false)
    );
    assert_eq!(
        runtime
            .get_thread_section(&section.id)
            .await
            .expect("section should load"),
        Some(section)
    );
}
