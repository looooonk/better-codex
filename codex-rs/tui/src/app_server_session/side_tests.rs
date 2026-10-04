use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_config::LoaderOverrides;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn native_side_fork_is_ephemeral_and_leaves_the_parent_available() -> Result<()> {
    let home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    let mut app_server = crate::start_embedded_app_server_for_picker(&config).await?;
    let parent = app_server.start_thread(&config).await?;
    let _: codex_app_server_protocol::ThreadInjectItemsResponse = app_server.request_handle().request_typed(ClientRequest::ThreadInjectItems {
        request_id: RequestId::String("seed-parent-history".to_string()),
        params: codex_app_server_protocol::ThreadInjectItemsParams { thread_id: parent.session.thread_id.to_string(), items: vec![serde_json::json!({"type":"message", "role":"user", "content":[{"type":"input_text", "text":"Parent conversation context"}]})] },
    }).await?;
    let side = app_server
        .fork_side_thread_in_background(config, parent.session.thread_id)
        .await?;
    assert!(side.turns.is_empty());
    let side_thread = app_server
        .thread_read(side.session.thread_id, /*include_turns*/ false)
        .await?;
    assert_eq!(
        (side_thread.ephemeral, side.session.daybreak_enabled),
        (true, false)
    );
    assert_ne!(side_thread.id, parent.session.thread_id.to_string());
    assert_eq!((side_thread.path, side_thread.forked_from_id), (None, None));
    let parent_thread = app_server
        .thread_read(parent.session.thread_id, /*include_turns*/ false)
        .await?;
    assert_eq!(
        (parent_thread.id, parent_thread.ephemeral),
        (parent.session.thread_id.to_string(), false)
    );
    app_server
        .thread_unsubscribe(side.session.thread_id)
        .await?;
    app_server.shutdown().await?;
    Ok(())
}
