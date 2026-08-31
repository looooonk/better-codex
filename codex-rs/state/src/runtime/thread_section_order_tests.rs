use super::*;
use crate::SortDirection;
use crate::SortKey;
use crate::ThreadFilterOptions;
use crate::ThreadSectionFilter;
use crate::runtime::test_support::test_thread_metadata;
use crate::runtime::test_support::unique_temp_dir;
use pretty_assertions::assert_eq;

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
