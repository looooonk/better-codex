use super::*;
use crate::SortDirection;
use crate::SortKey;
use crate::ThreadFilterOptions;
use crate::ThreadMetadataBuilder;
use crate::runtime::test_support::unique_temp_dir;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn legacy_chats_page_move_clear_and_respect_migration_checkpoints() -> anyhow::Result<()> {
    let home = unique_temp_dir();
    let runtime = StateRuntime::init(
        crate::SqliteConfig::new_for_testing(home.as_path().abs()),
        "test".into(),
    )
    .await?;
    let mut projects = Vec::new();
    for name in ["First", "Second"] {
        projects.push(
            runtime
                .create_project(name.into(), Vec::new(), Default::default(), &[], name)
                .await?
                .project
                .id,
        );
    }
    let mut threads = Vec::new();
    for index in 0..3 {
        let id = ThreadId::default();
        let mut metadata = ThreadMetadataBuilder::new(
            id,
            home.join(format!("{id}.jsonl")),
            chrono::Utc::now(),
            SessionSource::Cli,
        )
        .build("test");
        metadata.preview = Some(format!("Old chat {index}"));
        runtime.upsert_thread(&metadata).await?;
        threads.push(id);
    }
    let mut conn = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(home.join(DATABASE))
            .create_if_missing(true),
    )
    .await?;
    sqlx::query("CREATE TABLE settings(scope TEXT, key TEXT, value TEXT, PRIMARY KEY(scope,key))")
        .execute(&mut conn)
        .await?;
    let assignments = json!({
        threads[0].to_string(): {"projectKind":"local","projectId":"a"},
        threads[1].to_string(): {"projectKind":"local","projectId":"a"},
        threads[2].to_string(): {"projectKind":"remote","projectId":"a","hostId":"other"}
    });
    for (key, value) in [
        (
            PROJECT_MAP,
            json!({format!("local:{}",home.display()): {"a": projects[0], "b": projects[1]}}),
        ),
        (ASSIGNMENTS, assignments.clone()),
        (PROJECTLESS, json!([])),
    ] {
        sqlx::query("INSERT INTO settings VALUES ('global',?,?)")
            .bind(key)
            .bind(value.to_string())
            .execute(&mut conn)
            .await?;
    }
    let filter = |project_id| ThreadFilterOptions {
        archived_only: false,
        allowed_sources: &[],
        model_providers: None,
        cwd_filters: None,
        section: None,
        project_id: Some(project_id),
        anchor: None,
        sort_key: SortKey::RecencyAt,
        sort_direction: SortDirection::Desc,
        search_term: None,
    };
    let page = runtime.list_threads(1, filter(Some(&projects[0]))).await?;
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].project_id.as_ref(), Some(&projects[0]));
    let second = runtime
        .list_threads(
            1,
            ThreadFilterOptions {
                anchor: page.next_anchor.as_ref(),
                ..filter(Some(&projects[0]))
            },
        )
        .await?;
    assert_eq!(second.items.len(), 1);
    assert_ne!(second.items[0].id, page.items[0].id);
    assert_eq!(
        runtime.list_threads(3, filter(None)).await?.items[0].id,
        threads[2]
    );
    let id = threads[0].to_string();
    runtime.set_thread_project(&id, Some(&projects[1])).await?;
    assert_eq!(
        runtime
            .list_threads(3, filter(Some(&projects[1])))
            .await?
            .items[0]
            .id,
        threads[0]
    );
    runtime.set_thread_project(&id, None).await?;
    runtime.set_thread_project(&id, None).await?;
    assert_eq!(runtime.list_threads(3, filter(None)).await?.items.len(), 2);
    assert!(
        runtime
            .set_thread_project(&threads[2].to_string(), Some(&projects[0]))
            .await
            .is_err()
    );
    let read =
        |key| sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key=?").bind(key);
    let remaining: serde_json::Value =
        serde_json::from_str(&read(ASSIGNMENTS).fetch_one(&mut conn).await?)?;
    let mut expected = assignments;
    expected.as_object_mut().unwrap().remove(&id);
    assert_eq!(remaining, expected);
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&read(PROJECTLESS).fetch_one(&mut conn).await?)?,
        vec![id]
    );
    let checkpoint = json!({format!("local:{}",home.display()): {
        "threadAssignmentsReadMigrated":true,"threadAssignmentsMigrated":true,
        "pendingThreadAssignmentIds":[]
    }});
    sqlx::query("INSERT INTO settings VALUES ('global','electron-owner-value:app-server-projects-migration-by-host:test',?)")
        .bind(checkpoint.to_string()).execute(&mut conn).await?;
    assert_eq!(runtime.desktop_thread_project(threads[1]).await?, None);
    Ok(())
}
