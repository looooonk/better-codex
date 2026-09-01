use super::*;
use crate::PINNED_THREAD_SECTION_ID;
use crate::PINNED_THREAD_SECTION_NAME;
use crate::ThreadSection;
use crate::ThreadSectionAppearance;
use crate::ThreadSectionMove;
use crate::runtime::test_support::test_thread_metadata;
use crate::runtime::test_support::unique_temp_dir;
use codex_protocol::ThreadId;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn section_management_updates_appearance_and_protects_pinned_state() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("state db should initialize");
    let appearance = ThreadSectionAppearance {
        icon: Some("folder".to_string()),
        color: Some("blue".to_string()),
    };
    let created = runtime
        .create_thread_section("Projects", Some(appearance.clone()))
        .await
        .expect("section should be created");
    assert_eq!(
        runtime
            .rename_thread_section(
                &created.id,
                "Active projects",
                ThreadSectionAppearanceUpdate::Preserve,
            )
            .await
            .expect("section should be renamed"),
        Some(ThreadSection {
            id: created.id.clone(),
            name: "Active projects".to_string(),
            appearance: Some(appearance),
        })
    );
    let cleared = runtime
        .rename_thread_section(
            &created.id,
            "Active projects",
            ThreadSectionAppearanceUpdate::Replace(None),
        )
        .await
        .expect("appearance should be cleared")
        .expect("section should exist");
    assert_eq!(cleared.appearance, None);

    assert!(
        runtime
            .rename_thread_section(
                PINNED_THREAD_SECTION_ID,
                "Changed",
                ThreadSectionAppearanceUpdate::Preserve,
            )
            .await
            .is_err()
    );
    assert!(
        runtime
            .delete_thread_section(PINNED_THREAD_SECTION_ID)
            .await
            .is_err()
    );
    assert_eq!(
        runtime
            .get_thread_section(PINNED_THREAD_SECTION_ID)
            .await
            .expect("pinned section should load")
            .expect("pinned section should remain")
            .name,
        PINNED_THREAD_SECTION_NAME
    );

    let thread_id = ThreadId::new();
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.clone(),
        ))
        .await
        .expect("thread should insert");
    assert!(
        runtime
            .move_thread_to_section(
                thread_id,
                ThreadSectionMove::Append {
                    section_id: &created.id,
                },
            )
            .await
            .expect("thread should move")
    );
    assert!(
        runtime
            .delete_thread_section(&created.id)
            .await
            .expect("custom section should delete")
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
