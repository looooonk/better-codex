use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::create_fake_rollout;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::to_response;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadQueueAddParams;
use codex_app_server_protocol::ThreadQueueAddResponse;
use codex_app_server_protocol::ThreadQueueDeleteParams;
use codex_app_server_protocol::ThreadQueueDeleteResponse;
use codex_app_server_protocol::ThreadQueueListParams;
use codex_app_server_protocol::ThreadQueueListResponse;
use codex_app_server_protocol::ThreadQueueReorderParams;
use codex_app_server_protocol::ThreadQueueReorderResponse;
use codex_app_server_protocol::ThreadQueueUpdateParams;
use codex_app_server_protocol::ThreadQueueUpdateResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::UserInput;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn queue_crud_is_durable_for_an_unloaded_thread() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    write_config(codex_home.path(), &server.uri())?;
    let thread_id = create_fake_rollout(
        codex_home.path(),
        "2025-01-06T08-30-00",
        "2025-01-06T08:30:00Z",
        "Stored thread preview",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build()
        .await?;
    timeout(DEFAULT_TIMEOUT, app.initialize()).await??;

    let first = add(&mut app, &thread_id, "first", "client-1").await?;
    let repeated = add(&mut app, &thread_id, "first", "client-1").await?;
    assert_eq!(repeated, first);
    let second = add(&mut app, &thread_id, "second", "client-2").await?;

    let update_id = app
        .send_raw_request(
            "thread/queue/update",
            Some(serde_json::to_value(ThreadQueueUpdateParams {
                thread_id: thread_id.clone(),
                queued_submission_id: first.queued_submission.id.clone(),
                input: text_input("first updated"),
            })?),
        )
        .await?;
    let update: ThreadQueueUpdateResponse = response(&mut app, update_id).await?;
    assert_eq!(update.queued_submission.input, text_input("first updated"));

    let reorder_id = app
        .send_raw_request(
            "thread/queue/reorder",
            Some(serde_json::to_value(ThreadQueueReorderParams {
                thread_id: thread_id.clone(),
                queued_submission_ids: vec![
                    second.queued_submission.id.clone(),
                    first.queued_submission.id.clone(),
                ],
            })?),
        )
        .await?;
    let _: ThreadQueueReorderResponse = response(&mut app, reorder_id).await?;

    let list_id = app
        .send_raw_request(
            "thread/queue/list",
            Some(serde_json::to_value(ThreadQueueListParams {
                thread_id: thread_id.clone(),
                cursor: None,
                limit: Some(1),
            })?),
        )
        .await?;
    let first_page: ThreadQueueListResponse = response(&mut app, list_id).await?;
    assert_eq!(first_page.data, vec![second.queued_submission.clone()]);
    assert_eq!(first_page.next_cursor.as_deref(), Some("1"));

    let delete_id = app
        .send_raw_request(
            "thread/queue/delete",
            Some(serde_json::to_value(ThreadQueueDeleteParams {
                thread_id,
                queued_submission_id: second.queued_submission.id,
            })?),
        )
        .await?;
    let deleted: ThreadQueueDeleteResponse = response(&mut app, delete_id).await?;
    assert!(deleted.deleted);
    Ok(())
}

#[tokio::test]
async fn idle_thread_dispatches_a_durable_queued_submission() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    write_config(codex_home.path(), &server.uri())?;
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(DEFAULT_TIMEOUT, app.initialize()).await??;
    let start_id = app
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let started: ThreadStartResponse = response(&mut app, start_id).await?;

    let queued = add(&mut app, &started.thread.id, "queued", "client-idle").await?;
    assert_eq!(queued.queued_submission.input, text_input("queued"));
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("thread/queue/changed"),
    )
    .await??;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;

    let list_id = app
        .send_raw_request(
            "thread/queue/list",
            Some(serde_json::to_value(ThreadQueueListParams {
                thread_id: started.thread.id,
                cursor: None,
                limit: None,
            })?),
        )
        .await?;
    let listed: ThreadQueueListResponse = response(&mut app, list_id).await?;
    assert_eq!(listed.data, Vec::new());
    Ok(())
}

async fn add(
    app: &mut TestAppServer,
    thread_id: &str,
    text: &str,
    client_user_message_id: &str,
) -> Result<ThreadQueueAddResponse> {
    let request_id = app
        .send_raw_request(
            "thread/queue/add",
            Some(serde_json::to_value(ThreadQueueAddParams {
                thread_id: thread_id.to_string(),
                input: text_input(text),
                client_user_message_id: client_user_message_id.to_string(),
            })?),
        )
        .await?;
    response(app, request_id).await
}

async fn response<T: serde::de::DeserializeOwned>(
    app: &mut TestAppServer,
    request_id: i64,
) -> Result<T> {
    let response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    to_response(response)
}

fn text_input(text: &str) -> Vec<UserInput> {
    vec![UserInput::Text {
        text: text.to_string(),
        text_elements: Vec::new(),
    }]
}

fn write_config(codex_home: &Path, server_uri: &str) -> std::io::Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"
model = "mock-model"
approval_policy = "never"
sandbox_mode = "read-only"
model_provider = "mock_provider"
suppress_unstable_features_warning = true

[features]
sqlite = true

[model_providers.mock_provider]
name = "Mock provider"
base_url = "{server_uri}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#
        ),
    )
}
