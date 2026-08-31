use super::*;
use crate::PINNED_THREAD_SECTION_ID;
use crate::PINNED_THREAD_SECTION_NAME;
use crate::SortDirection;
use crate::SortKey;
use crate::ThreadFilterOptions;
use crate::ThreadSection;
use crate::ThreadSectionAppearance;
use crate::ThreadSectionFilter;
use crate::ThreadSectionsPage;
use crate::runtime::test_support::test_thread_metadata;
use crate::runtime::test_support::unique_temp_dir;
use codex_protocol::ThreadId;
use pretty_assertions::assert_eq;

const CUSTOM_SECTION_ID: &str = "01984de2-8f74-7c91-a3b2-5c5e937cf317";

#[tokio::test]
async fn sections_list_with_persisted_appearance_and_stable_pagination() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home, "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let custom = ThreadSection {
        id: CUSTOM_SECTION_ID.to_string(),
        name: "Projects".to_string(),
        appearance: Some(ThreadSectionAppearance {
            icon: Some("folder".to_string()),
            color: Some("blue".to_string()),
        }),
    };
    sqlx::query("INSERT INTO thread_sections (id, name, appearance) VALUES (?, ?, ?)")
        .bind(&custom.id)
        .bind(&custom.name)
        .bind(
            serde_json::to_string(&custom.appearance).expect("section appearance should serialize"),
        )
        .execute(runtime.pool.as_ref())
        .await
        .expect("custom section should insert");
    let pinned = ThreadSection {
        id: PINNED_THREAD_SECTION_ID.to_string(),
        name: PINNED_THREAD_SECTION_NAME.to_string(),
        appearance: None,
    };

    assert_eq!(
        runtime
            .get_thread_section(CUSTOM_SECTION_ID)
            .await
            .expect("custom section should load"),
        Some(custom.clone())
    );
    assert_eq!(
        runtime
            .list_thread_sections(/*cursor*/ None, /*limit*/ 1)
            .await
            .expect("first page should load"),
        ThreadSectionsPage {
            sections: vec![custom.clone()],
            next_cursor: Some(custom.id.clone()),
        }
    );
    assert_eq!(
        runtime
            .list_thread_sections(Some(&custom.id), /*limit*/ 1)
            .await
            .expect("second page should load"),
        ThreadSectionsPage {
            sections: vec![pinned],
            next_cursor: None,
        }
    );
}

#[tokio::test]
async fn section_metadata_filters_and_orders_threads_without_reconciliation_loss() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let pinned = ThreadSection {
        id: PINNED_THREAD_SECTION_ID.to_string(),
        name: PINNED_THREAD_SECTION_NAME.to_string(),
        appearance: None,
    };
    let first =
        ThreadId::from_string("00000000-0000-0000-0000-000000000061").expect("valid thread id");
    let tied =
        ThreadId::from_string("00000000-0000-0000-0000-000000000062").expect("valid thread id");
    let second =
        ThreadId::from_string("00000000-0000-0000-0000-000000000063").expect("valid thread id");
    let unsectioned =
        ThreadId::from_string("00000000-0000-0000-0000-000000000064").expect("valid thread id");
    for (thread_id, position) in [(second, 2_000_000), (tied, 1_000_000), (first, 1_000_000)] {
        let mut metadata = test_thread_metadata(&codex_home, thread_id, codex_home.clone());
        metadata.section = Some(pinned.clone());
        metadata.section_position = Some(position);
        metadata.section_entered_at = Some(metadata.updated_at);
        runtime
            .upsert_thread(&metadata)
            .await
            .expect("sectioned thread should insert");
    }
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            unsectioned,
            codex_home.clone(),
        ))
        .await
        .expect("unsectioned thread should insert");

    let filters = |anchor| ThreadFilterOptions {
        archived_only: false,
        allowed_sources: &[],
        model_providers: None,
        cwd_filters: None,
        section: ThreadSectionFilter::Section(PINNED_THREAD_SECTION_ID),
        anchor,
        sort_key: SortKey::SectionPosition,
        sort_direction: SortDirection::Asc,
        search_term: None,
    };
    let first_page = runtime
        .list_threads(/*page_size*/ 1, filters(/*anchor*/ None))
        .await
        .expect("first ordered page should load");
    let second_page = runtime
        .list_threads(
            /*page_size*/ 1,
            filters(first_page.next_anchor.as_ref()),
        )
        .await
        .expect("second ordered page should load");
    let third_page = runtime
        .list_threads(
            /*page_size*/ 1,
            filters(second_page.next_anchor.as_ref()),
        )
        .await
        .expect("third ordered page should load");
    assert_eq!(
        [
            first_page.items[0].id,
            second_page.items[0].id,
            third_page.items[0].id,
        ],
        [first, tied, second]
    );
    assert_eq!(third_page.next_anchor, None);
    assert_eq!(
        first_page.items[0].section,
        Some(pinned.clone()),
        "the persisted section presentation should be authoritative"
    );

    let original = test_thread_metadata(&codex_home, first, codex_home.clone());
    runtime
        .upsert_thread(&original)
        .await
        .expect("rollout reconciliation should succeed");
    assert_eq!(
        runtime
            .get_thread(first)
            .await
            .expect("thread should load")
            .expect("thread should exist")
            .section,
        Some(pinned)
    );

    let unsectioned_page = runtime
        .list_threads(
            /*page_size*/ 10,
            ThreadFilterOptions {
                archived_only: false,
                allowed_sources: &[],
                model_providers: None,
                cwd_filters: None,
                section: ThreadSectionFilter::Unsectioned,
                anchor: None,
                sort_key: SortKey::RecencyAt,
                sort_direction: SortDirection::Desc,
                search_term: None,
            },
        )
        .await
        .expect("unsectioned threads should list");
    assert_eq!(
        unsectioned_page
            .items
            .into_iter()
            .map(|thread| thread.id)
            .collect::<Vec<_>>(),
        vec![unsectioned]
    );
}
