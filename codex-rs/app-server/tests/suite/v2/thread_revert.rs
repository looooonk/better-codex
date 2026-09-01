use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::create_final_assistant_message_sse_response;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::create_mock_responses_server_sequence;
use app_test_support::create_request_user_input_sse_response;
use app_test_support::to_response;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItemsListParams;
use codex_app_server_protocol::ThreadItemsListResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadRevertParams;
use codex_app_server_protocol::ThreadRevertResponse;
use codex_app_server_protocol::ThreadRevertedNotification;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::path::Path;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const ACTIVE_REVERT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[tokio::test]
async fn thread_revert_replaces_paginated_history_before_turn() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), &server.uri())?;
    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let start_id = mcp
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            history_mode: Some(ThreadHistoryMode::Paginated),
            ..Default::default()
        })
        .await?;
    let start_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(start_id)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response(start_response)?;
    let stale_rollout_path = thread.path.clone().expect("thread rollout path");
    let mut turn_ids = Vec::new();
    for text in ["first", "second"] {
        let completed = mcp
            .start_turn_and_wait_for_completion(TurnStartParams {
                thread_id: thread.id.clone(),
                input: vec![UserInput::Text {
                    text: text.to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
        turn_ids.push(completed.turn.id);
    }
    mcp.clear_message_buffer();

    let revert_id = mcp
        .send_thread_revert_request(ThreadRevertParams {
            thread_id: thread.id.clone(),
            before_turn_id: turn_ids[1].clone(),
        })
        .await?;
    let revert_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(revert_id)),
    )
    .await??;
    let ThreadRevertResponse {
        thread: reverted_thread,
        turns_backwards_cursor,
        items_backwards_cursor,
    } = to_response(revert_response)?;
    let reverted_notification = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("thread/reverted"),
    )
    .await??;
    let reverted: ThreadRevertedNotification = serde_json::from_value(
        reverted_notification
            .params
            .expect("thread/reverted params"),
    )?;
    assert_eq!(reverted.thread_id, thread.id);

    assert_eq!(reverted_thread.id, thread.id);
    assert!(reverted_thread.turns.is_empty());
    assert!(items_backwards_cursor.is_some());
    assert_eq!(
        turn_ids_from_cursor(
            &mut mcp,
            thread.id.as_str(),
            turns_backwards_cursor,
            /*sort_direction*/ None,
        )
        .await?,
        turn_ids[..1]
    );
    let items_id = mcp
        .send_thread_items_list_request(ThreadItemsListParams {
            thread_id: thread.id.clone(),
            turn_id: None,
            cursor: items_backwards_cursor,
            limit: None,
            sort_direction: None,
        })
        .await?;
    let items_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(items_id)),
    )
    .await??;
    let ThreadItemsListResponse {
        data: reverted_items,
        ..
    } = to_response(items_response)?;
    assert!(!reverted_items.is_empty());
    assert!(
        reverted_items
            .iter()
            .all(|item| item.turn_id == turn_ids[0])
    );

    mcp.shutdown_gracefully().await?;
    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;
    let stale_resume_id = mcp
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.id.clone(),
            path: Some(stale_rollout_path),
            ..Default::default()
        })
        .await?;
    let stale_resume_error: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(stale_resume_id)),
    )
    .await??;
    assert!(
        stale_resume_error.error.message.contains("stale path")
            && stale_resume_error
                .error
                .message
                .contains("omit path and resume by thread id"),
        "unexpected resume error: {}",
        stale_resume_error.error.message,
    );
    let resume_id = mcp
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.id.clone(),
            ..Default::default()
        })
        .await?;
    let resume_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(resume_id)),
    )
    .await??;
    let _: ThreadResumeResponse = to_response(resume_response)?;

    let invalid_revert_id = mcp
        .send_thread_revert_request(ThreadRevertParams {
            thread_id: thread.id.clone(),
            before_turn_id: "missing-turn".to_string(),
        })
        .await?;
    let invalid_revert_error: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(invalid_revert_id)),
    )
    .await??;
    assert_eq!(
        invalid_revert_error.error.message,
        "turn not found: missing-turn"
    );

    let third_turn = mcp
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.id.clone(),
            input: vec![UserInput::Text {
                text: "third".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let requests = server.received_requests().await.expect("response requests");
    let model_input = requests
        .iter()
        .rev()
        .find(|request| request.url.path().ends_with("/responses"))
        .expect("third turn response request")
        .body_json::<Value>()?["input"]
        .clone();
    let model_input = serde_json::to_string(&model_input)?;
    assert!(model_input.contains("first"));
    assert!(!model_input.contains("second"));
    assert!(model_input.contains("third"));
    assert_eq!(
        turn_ids_from_cursor(
            &mut mcp,
            thread.id.as_str(),
            /*cursor*/ None,
            Some(SortDirection::Asc),
        )
        .await?,
        vec![turn_ids[0].clone(), third_turn.turn.id]
    );
    Ok(())
}

#[tokio::test]
async fn thread_revert_interrupts_active_turn_and_keeps_subscription() -> Result<()> {
    let codex_home = TempDir::new()?;
    let server = create_mock_responses_server_sequence(vec![
        create_final_assistant_message_sse_response("first")?,
        create_request_user_input_sse_response("call_blocked")?,
        create_final_assistant_message_sse_response("third")?,
    ])
    .await;
    create_config_toml(codex_home.path(), &server.uri())?;
    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let start_id = mcp
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            history_mode: Some(ThreadHistoryMode::Paginated),
            ..Default::default()
        })
        .await?;
    let start_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(start_id)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response(start_response)?;
    let first_turn = mcp
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.id.clone(),
            input: vec![UserInput::Text {
                text: "first".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let active_id = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            input: vec![UserInput::Text {
                text: "sleep".to_string(),
                text_elements: Vec::new(),
            }],
            collaboration_mode: Some(CollaborationMode {
                mode: ModeKind::Plan,
                settings: Settings {
                    model: "mock-model".to_string(),
                    reasoning_effort: Some(ReasoningEffort::Medium),
                    developer_instructions: None,
                },
            }),
            approval_policy: Some(AskForApproval::Never),
            ..Default::default()
        })
        .await?;
    let active_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(active_id)),
    )
    .await??;
    let TurnStartResponse { turn: active_turn } = to_response(active_response)?;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_request_message(),
    )
    .await??;

    let revert_id = mcp
        .send_thread_revert_request(ThreadRevertParams {
            thread_id: thread.id.clone(),
            before_turn_id: active_turn.id,
        })
        .await?;
    let revert_response: JSONRPCResponse = timeout(
        ACTIVE_REVERT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(revert_id)),
    )
    .await??;
    let ThreadRevertResponse {
        thread: reverted_thread,
        turns_backwards_cursor,
        items_backwards_cursor,
    } = to_response(revert_response)?;
    let completed_notification = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let completed: TurnCompletedNotification = serde_json::from_value(
        completed_notification
            .params
            .expect("turn/completed params"),
    )?;
    assert_eq!(completed.thread_id, thread.id);
    assert_eq!(completed.turn.status, TurnStatus::Interrupted);
    assert!(reverted_thread.turns.is_empty());
    assert!(items_backwards_cursor.is_some());
    assert_eq!(
        turn_ids_from_cursor(
            &mut mcp,
            thread.id.as_str(),
            turns_backwards_cursor,
            /*sort_direction*/ None,
        )
        .await?,
        vec![first_turn.turn.id]
    );

    let resume_id = mcp
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.id.clone(),
            ..Default::default()
        })
        .await?;
    let resume_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(resume_id)),
    )
    .await??;
    let resumed: ThreadResumeResponse = to_response(resume_response)?;
    assert_eq!(resumed.approval_policy, AskForApproval::Never);

    let third_turn = mcp
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.id,
            input: vec![UserInput::Text {
                text: "third".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    assert_eq!(third_turn.turn.status, TurnStatus::Completed);
    Ok(())
}

async fn turn_ids_from_cursor(
    mcp: &mut TestAppServer,
    thread_id: &str,
    cursor: Option<String>,
    sort_direction: Option<SortDirection>,
) -> Result<Vec<String>> {
    let request_id = mcp
        .send_thread_turns_list_request(ThreadTurnsListParams {
            thread_id: thread_id.to_string(),
            cursor,
            limit: None,
            sort_direction,
            items_view: None,
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let ThreadTurnsListResponse { data, .. } = to_response(response)?;
    Ok(data.into_iter().map(|turn| turn.id).collect())
}

fn create_config_toml(codex_home: &Path, server_uri: &str) -> std::io::Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"
model = "mock-model"
approval_policy = "never"
sandbox_mode = "read-only"
model_provider = "mock_provider"

[model_providers.mock_provider]
name = "Mock provider for test"
base_url = "{server_uri}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#
        ),
    )
}
