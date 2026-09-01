use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::create_fake_paginated_rollout;
use app_test_support::create_fake_rollout;
use app_test_support::create_final_assistant_message_sse_response;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::create_mock_responses_server_sequence;
use app_test_support::create_shell_command_sse_response;
use app_test_support::rollout_path;
use app_test_support::to_response;
use app_test_support::write_models_cache;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::InitializeCapabilities;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadGoalSetResponse;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadLoadedListParams;
use codex_app_server_protocol::ThreadLoadedListResponse;
use codex_app_server_protocol::ThreadQueueAddParams;
use codex_app_server_protocol::ThreadQueueAddResponse;
use codex_app_server_protocol::ThreadQueueDeleteParams;
use codex_app_server_protocol::ThreadQueueDeleteResponse;
use codex_app_server_protocol::ThreadQueueListParams;
use codex_app_server_protocol::ThreadQueueListResponse;
use codex_app_server_protocol::ThreadQueueReorderParams;
use codex_app_server_protocol::ThreadQueueReorderResponse;
use codex_app_server_protocol::ThreadQueueStartParams;
use codex_app_server_protocol::ThreadQueueUpdateParams;
use codex_app_server_protocol::ThreadQueueUpdateResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadRevertParams;
use codex_app_server_protocol::ThreadRevertResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_protocol::ThreadId;
use codex_protocol::items::TurnItem as CoreTurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::user_input::UserInput as CoreUserInput;
use codex_rollout::RolloutItem;
use codex_rollout::append_rollout_item_to_path;
use codex_state::QueueClaimResult;
use codex_state::StateRuntime;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::sleep;
use tokio::time::timeout;

use super::connection_handling_websocket::WsClient;
use super::connection_handling_websocket::connect_websocket;
use super::connection_handling_websocket::read_error_for_id;
use super::connection_handling_websocket::read_response_for_id;
use super::connection_handling_websocket::send_request;
use super::connection_handling_websocket::spawn_websocket_server;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const INVALID_REQUEST_ERROR_CODE: i64 = -32600;

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
async fn cold_recovery_keeps_consumed_indeterminate_work_visible_until_delete() -> Result<()> {
    const CLIENT_ID: &str = "client-indeterminate";
    const OWNER_TEXT: &str = "indeterminate queued input";
    const FOLLOWER_TEXT: &str = "queued after acknowledged recovery";
    const TURN_ID: &str = "turn-indeterminate";

    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    write_config(codex_home.path(), &server.uri())?;
    let filename_ts = "2025-01-06T09-30-00";
    let thread_id = create_fake_paginated_rollout(
        codex_home.path(),
        filename_ts,
        "2025-01-06T09:30:00Z",
        "Stored thread preview",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let parsed_thread_id = ThreadId::from_string(&thread_id)?;
    let path = rollout_path(codex_home.path(), filename_ts, &thread_id);
    append_rollout_item_to_path(
        &path,
        &RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: TURN_ID.to_string(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        })),
    )
    .await?;
    append_rollout_item_to_path(
        &path,
        &RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id: parsed_thread_id,
            turn_id: TURN_ID.to_string(),
            item: CoreTurnItem::UserMessage(UserMessageItem {
                id: "user-indeterminate".to_string(),
                client_id: Some(CLIENT_ID.to_string()),
                content: vec![CoreUserInput::Text {
                    text: OWNER_TEXT.to_string(),
                    text_elements: Vec::new(),
                }],
            }),
            completed_at_ms: 1,
        })),
    )
    .await?;

    let state =
        StateRuntime::init(codex_home.path().to_path_buf(), "mock_provider".to_string()).await?;
    let owner = state
        .enqueue_queued_submission(
            parsed_thread_id,
            &serde_json::to_string(&vec![CoreUserInput::Text {
                text: OWNER_TEXT.to_string(),
                text_elements: Vec::new(),
            }])?,
            CLIENT_ID,
        )
        .await?;
    let follower = state
        .enqueue_queued_submission(
            parsed_thread_id,
            &serde_json::to_string(&vec![CoreUserInput::Text {
                text: FOLLOWER_TEXT.to_string(),
                text_elements: Vec::new(),
            }])?,
            "client-follower",
        )
        .await?;
    assert!(matches!(
        state
            .claim_queued_submission(parsed_thread_id, Some(&owner.id), TURN_ID)
            .await?,
        QueueClaimResult::Claimed(_)
    ));
    assert!(
        state
            .mark_queued_submission_inflight(parsed_thread_id, TURN_ID)
            .await?
    );
    state.close().await;

    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build()
        .await?;
    timeout(DEFAULT_TIMEOUT, app.initialize()).await??;

    let list_id = app
        .send_raw_request(
            "thread/queue/list",
            Some(serde_json::to_value(ThreadQueueListParams {
                thread_id: thread_id.clone(),
                cursor: None,
                limit: None,
            })?),
        )
        .await?;
    let listed: ThreadQueueListResponse = response(&mut app, list_id).await?;
    assert_eq!(
        listed.data,
        vec![
            codex_app_server_protocol::QueuedSubmission {
                id: owner.id.clone(),
                input: text_input(OWNER_TEXT),
                client_user_message_id: CLIENT_ID.to_string(),
            },
            codex_app_server_protocol::QueuedSubmission {
                id: follower.id.clone(),
                input: text_input(FOLLOWER_TEXT),
                client_user_message_id: "client-follower".to_string(),
            },
        ]
    );
    assert!(
        server
            .received_requests()
            .await
            .expect("response requests")
            .is_empty()
    );

    let update_id = app
        .send_raw_request(
            "thread/queue/update",
            Some(serde_json::to_value(ThreadQueueUpdateParams {
                thread_id: thread_id.clone(),
                queued_submission_id: owner.id.clone(),
                input: text_input("replacement must be rejected"),
            })?),
        )
        .await?;
    let update_error: JSONRPCError = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(update_id)),
    )
    .await??;
    assert_eq!(
        update_error.error.message,
        "queued input is already durable and the blocked submission can only be deleted"
    );

    let resume_id = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread_id.clone(),
            ..Default::default()
        })
        .await?;
    let _: ThreadResumeResponse = response(&mut app, resume_id).await?;
    sleep(Duration::from_millis(/*millis*/ 100)).await;
    assert!(
        server
            .received_requests()
            .await
            .expect("response requests")
            .is_empty()
    );

    let start_id = app
        .send_raw_request(
            "thread/queue/start",
            Some(serde_json::to_value(ThreadQueueStartParams {
                thread_id: thread_id.clone(),
                queued_submission_id: None,
            })?),
        )
        .await?;
    let start_error: JSONRPCError = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(start_id)),
    )
    .await??;
    assert_eq!(
        start_error.error.message,
        format!(
            "queued submission {} is blocked because its input is already durable; delete it to acknowledge and discard it",
            owner.id
        )
    );

    let delete_id = app
        .send_raw_request(
            "thread/queue/delete",
            Some(serde_json::to_value(ThreadQueueDeleteParams {
                thread_id: thread_id.clone(),
                queued_submission_id: owner.id,
            })?),
        )
        .await?;
    let deleted: ThreadQueueDeleteResponse = response(&mut app, delete_id).await?;
    assert!(deleted.deleted);
    timeout(DEFAULT_TIMEOUT, async {
        loop {
            if server.received_requests().await.is_some_and(|requests| {
                requests
                    .iter()
                    .any(|request| request_body_contains(request, FOLLOWER_TEXT))
            }) {
                return;
            }
            sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await?;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let final_list_id = app
        .send_raw_request(
            "thread/queue/list",
            Some(serde_json::to_value(ThreadQueueListParams {
                thread_id,
                cursor: None,
                limit: None,
            })?),
        )
        .await?;
    let final_list: ThreadQueueListResponse = response(&mut app, final_list_id).await?;
    assert!(final_list.data.is_empty());
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
            history_mode: Some(ThreadHistoryMode::Paginated),
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
        app.read_stream_until_notification_message("thread/queue/changed"),
    )
    .await??;
    timeout(
        DEFAULT_TIMEOUT,
        turn_completed_before_queue_changed(&mut app),
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

#[tokio::test]
async fn revert_dispatches_surviving_queue_follower_once_after_notification() -> Result<()> {
    let server = create_mock_responses_server_sequence(vec![
        create_shell_command_sse_response(
            vec!["sleep".to_string(), "30".to_string()],
            /*workdir*/ None,
            Some(30_000),
            "call_blocked",
        )?,
        create_final_assistant_message_sse_response("pending resumed")?,
    ])
    .await;
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
            history_mode: Some(ThreadHistoryMode::Paginated),
            ..Default::default()
        })
        .await?;
    let started_thread: ThreadStartResponse = response(&mut app, start_id).await?;

    add(
        &mut app,
        &started_thread.thread.id,
        "active queued",
        "client-active",
    )
    .await?;
    let started_notification: JSONRPCNotification = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/started"),
    )
    .await??;
    let started: TurnStartedNotification = serde_json::from_value(
        started_notification
            .params
            .expect("turn/started params should be present"),
    )?;
    wait_for_command_start(&mut app).await?;
    let goal_id = app
        .send_raw_request(
            "thread/goal/set",
            Some(json!({
                "threadId": started_thread.thread.id,
                "objective": "do not restart after revert",
                "status": "active",
            })),
        )
        .await?;
    let _: ThreadGoalSetResponse = response(&mut app, goal_id).await?;
    let pending = add(
        &mut app,
        &started_thread.thread.id,
        "pending queued",
        "client-pending",
    )
    .await?;

    let revert_id = app
        .send_thread_revert_request(ThreadRevertParams {
            thread_id: started_thread.thread.id.clone(),
            before_turn_id: started.turn.id,
        })
        .await?;
    let revert_response: JSONRPCResponse = timeout(
        Duration::from_secs(/*secs*/ 2),
        app.read_stream_until_response_message(RequestId::Integer(revert_id)),
    )
    .await??;
    let _: ThreadRevertResponse = to_response(revert_response)?;
    let completed_notification: JSONRPCNotification = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let completed: TurnCompletedNotification = serde_json::from_value(
        completed_notification
            .params
            .expect("turn/completed params should be present"),
    )?;
    assert_eq!(completed.turn.status, TurnStatus::Interrupted);

    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("thread/reverted"),
    )
    .await??;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/started"),
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
                thread_id: started_thread.thread.id,
                cursor: None,
                limit: None,
            })?),
        )
        .await?;
    let listed: ThreadQueueListResponse = response(&mut app, list_id).await?;
    assert_eq!(listed.data, Vec::new());
    let requests = server.received_requests().await.expect("response requests");
    assert_eq!(requests.len(), 2);
    assert!(String::from_utf8(requests[1].body.clone())?.contains("pending queued"));
    assert_eq!(
        pending.queued_submission.input,
        text_input("pending queued")
    );
    Ok(())
}

#[tokio::test]
async fn queued_user_input_reserves_idle_before_goal_continuation() -> Result<()> {
    let codex_home = TempDir::new()?;
    let release_path = codex_home.path().join("release-active");
    let release_command = format!(
        "while [ ! -f '{}' ]; do sleep 0.01; done",
        release_path.display()
    );
    let server = create_mock_responses_server_sequence(vec![
        create_shell_command_sse_response(
            vec!["sh".to_string(), "-c".to_string(), release_command],
            Some(codex_home.path()),
            Some(30_000),
            "hold-active",
        )?,
        create_final_assistant_message_sse_response("active done")?,
        create_shell_command_sse_response(
            vec!["sleep".to_string(), "30".to_string()],
            /*workdir*/ None,
            Some(30_000),
            "hold-priority",
        )?,
    ])
    .await;
    write_config(codex_home.path(), &server.uri())?;
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_managed_config()
        .build()
        .await?;
    timeout(DEFAULT_TIMEOUT, app.initialize()).await??;
    let start_id = app
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let started_thread: ThreadStartResponse = response(&mut app, start_id).await?;
    let active_id = app
        .send_turn_start_request(TurnStartParams {
            thread_id: started_thread.thread.id.clone(),
            input: text_input("active"),
            ..Default::default()
        })
        .await?;
    let _: TurnStartResponse = response(&mut app, active_id).await?;
    wait_for_command_start(&mut app).await?;

    let goal_id = app
        .send_raw_request(
            "thread/goal/set",
            Some(json!({
                "threadId": started_thread.thread.id,
                "objective": "continue the goal after queued user work",
                "status": "active",
            })),
        )
        .await?;
    let _: ThreadGoalSetResponse = response(&mut app, goal_id).await?;
    add(
        &mut app,
        &started_thread.thread.id,
        "queued priority",
        "client-priority",
    )
    .await?;

    std::fs::write(release_path, "release")?;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/started"),
    )
    .await??;
    wait_for_command_start(&mut app).await?;

    let requests = server.received_requests().await.expect("response requests");
    assert_eq!(requests.len(), 3);
    let priority_body = String::from_utf8(requests[2].body.clone())?;
    assert!(
        priority_body.contains("queued priority"),
        "queued user input should reserve the idle turn before goal continuation"
    );
    Ok(())
}

#[tokio::test]
async fn loaded_v1_child_waits_for_listener_before_queue_claim() -> Result<()> {
    const CHILD_PROMPT: &str = "child: finish setup";
    const PARENT_PROMPT: &str = "spawn a child";
    const QUEUED_PROMPT: &str = "queued after listener";
    const SPAWN_CALL_ID: &str = "spawn-queue-child";

    let server = responses::start_mock_server().await;
    let spawn_args = serde_json::to_string(&json!({ "message": CHILD_PROMPT }))?;
    let parent_turn = responses::mount_response_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, PARENT_PROMPT),
        responses::sse_response(responses::sse(vec![
            responses::ev_response_created("resp-parent-spawn"),
            responses::ev_function_call_with_namespace(
                SPAWN_CALL_ID,
                "multi_agent_v1",
                "spawn_agent",
                &spawn_args,
            ),
            responses::ev_completed("resp-parent-spawn"),
        ]))
        .set_delay(Duration::from_secs(/*secs*/ 2)),
    )
    .await;
    let child_turn = responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            request_body_contains(request, CHILD_PROMPT)
                && !request_body_contains(request, SPAWN_CALL_ID)
        },
        responses::sse(vec![
            responses::ev_response_created("resp-child"),
            responses::ev_assistant_message("msg-child", "child ready"),
            responses::ev_completed("resp-child"),
        ]),
    )
    .await;
    let parent_follow_up = responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, SPAWN_CALL_ID),
        responses::sse(vec![
            responses::ev_response_created("resp-parent-finish"),
            responses::ev_assistant_message("msg-parent", "parent done"),
            responses::ev_completed("resp-parent-finish"),
        ]),
    )
    .await;
    let queued_child_turn = responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, QUEUED_PROMPT),
        responses::sse(vec![
            responses::ev_response_created("resp-child-queued"),
            responses::ev_assistant_message("msg-child-queued", "queued done"),
            responses::ev_completed("resp-child-queued"),
        ]),
    )
    .await;

    let codex_home = TempDir::new()?;
    write_config(codex_home.path(), &server.uri())?;
    write_models_cache(codex_home.path())?;
    let (mut process, bind_addr) = spawn_websocket_server(codex_home.path()).await?;

    let mut owner = connect_websocket(bind_addr).await?;
    initialize_queue_websocket(&mut owner, /*id*/ 1, "queue_owner").await?;
    send_request(
        &mut owner,
        "thread/start",
        /*id*/ 2,
        Some(serde_json::to_value(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })?),
    )
    .await?;
    let parent: ThreadStartResponse = websocket_response(&mut owner, /*id*/ 2).await?;
    send_request(
        &mut owner,
        "turn/start",
        /*id*/ 3,
        Some(serde_json::to_value(TurnStartParams {
            thread_id: parent.thread.id.clone(),
            input: text_input(PARENT_PROMPT),
            ..Default::default()
        })?),
    )
    .await?;
    let _: TurnStartResponse = websocket_response(&mut owner, /*id*/ 3).await?;
    timeout(DEFAULT_TIMEOUT, async {
        while parent_turn.requests().is_empty() {
            sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await?;
    owner
        .close(None)
        .await
        .context("failed to close owner websocket")?;
    drop(owner);
    sleep(Duration::from_millis(/*millis*/ 100)).await;

    timeout(DEFAULT_TIMEOUT, async {
        while child_turn.requests().is_empty() || parent_follow_up.requests().is_empty() {
            sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await?;
    assert_eq!(parent_turn.requests().len(), 1);
    assert_eq!(child_turn.requests().len(), 1);
    assert_eq!(parent_follow_up.requests().len(), 1);
    // Keep the server connectionless while it consumes the child-created event.
    sleep(Duration::from_millis(/*millis*/ 100)).await;

    let mut client = connect_websocket(bind_addr).await?;
    initialize_queue_websocket(&mut client, /*id*/ 1, "queue_client").await?;
    let mut request_id = 2;
    let child_thread_id = timeout(DEFAULT_TIMEOUT, async {
        loop {
            send_request(
                &mut client,
                "thread/loaded/list",
                request_id,
                Some(serde_json::to_value(ThreadLoadedListParams::default())?),
            )
            .await?;
            let loaded: ThreadLoadedListResponse =
                websocket_response(&mut client, request_id).await?;
            request_id += 1;
            if loaded.data.len() == 2 {
                return loaded
                    .data
                    .into_iter()
                    .find(|thread_id| thread_id != &parent.thread.id)
                    .ok_or_else(|| anyhow::anyhow!("spawn should return a child thread"));
            }
            sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await??;
    timeout(DEFAULT_TIMEOUT, async {
        loop {
            send_request(
                &mut client,
                "thread/read",
                request_id,
                Some(serde_json::to_value(ThreadReadParams {
                    thread_id: child_thread_id.clone(),
                    include_turns: false,
                })?),
            )
            .await?;
            let read: ThreadReadResponse = websocket_response(&mut client, request_id).await?;
            request_id += 1;
            if read.thread.status == ThreadStatus::Idle {
                return Ok::<(), anyhow::Error>(());
            }
            sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await??;

    send_request(
        &mut client,
        "thread/queue/add",
        request_id,
        Some(serde_json::to_value(ThreadQueueAddParams {
            thread_id: child_thread_id.clone(),
            input: text_input(QUEUED_PROMPT),
            client_user_message_id: "client-child".to_string(),
        })?),
    )
    .await?;
    let queued: ThreadQueueAddResponse = websocket_response(&mut client, request_id).await?;
    request_id += 1;
    assert_eq!(
        websocket_list(&mut client, request_id, &child_thread_id)
            .await?
            .data,
        vec![queued.queued_submission.clone()]
    );
    assert!(queued_child_turn.requests().is_empty());
    request_id += 1;
    send_request(
        &mut client,
        "thread/queue/start",
        request_id,
        Some(serde_json::to_value(ThreadQueueStartParams {
            thread_id: child_thread_id.clone(),
            queued_submission_id: Some(queued.queued_submission.id.clone()),
        })?),
    )
    .await?;
    let explicit_error: JSONRPCError =
        timeout(DEFAULT_TIMEOUT, read_error_for_id(&mut client, request_id)).await??;
    request_id += 1;
    assert_eq!(explicit_error.error.code, INVALID_REQUEST_ERROR_CODE);
    assert_eq!(
        explicit_error.error.message,
        "resume/subscribe the thread before starting a queued message"
    );
    assert_eq!(
        websocket_list(&mut client, request_id, &child_thread_id)
            .await?
            .data,
        vec![queued.queued_submission.clone()]
    );
    assert!(queued_child_turn.requests().is_empty());
    request_id += 1;

    send_request(
        &mut client,
        "thread/resume",
        request_id,
        Some(serde_json::to_value(ThreadResumeParams {
            thread_id: child_thread_id.clone(),
            ..Default::default()
        })?),
    )
    .await?;
    let _: ThreadResumeResponse = websocket_response(&mut client, request_id).await?;
    request_id += 1;
    timeout(DEFAULT_TIMEOUT, async {
        while queued_child_turn.requests().is_empty() {
            sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await?;
    assert_eq!(queued_child_turn.requests().len(), 1);
    timeout(DEFAULT_TIMEOUT, async {
        loop {
            let listed = websocket_list(&mut client, request_id, &child_thread_id).await?;
            request_id += 1;
            if listed.data.is_empty() {
                return Ok::<(), anyhow::Error>(());
            }
            sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await??;

    process
        .kill()
        .await
        .context("failed to stop websocket app-server process")?;
    Ok(())
}

async fn initialize_queue_websocket(
    stream: &mut WsClient,
    id: i64,
    client_name: &str,
) -> Result<()> {
    send_request(
        stream,
        "initialize",
        id,
        Some(serde_json::to_value(InitializeParams {
            client_info: ClientInfo {
                name: client_name.to_string(),
                title: Some("Queue Test Client".to_string()),
                version: "0.1.0".to_string(),
            },
            capabilities: Some(InitializeCapabilities {
                experimental_api: true,
                ..Default::default()
            }),
        })?),
    )
    .await?;
    read_response_for_id(stream, id).await?;
    Ok(())
}

async fn websocket_response<T: serde::de::DeserializeOwned>(
    stream: &mut WsClient,
    id: i64,
) -> Result<T> {
    to_response(read_response_for_id(stream, id).await?)
}

async fn websocket_list(
    stream: &mut WsClient,
    id: i64,
    thread_id: &str,
) -> Result<ThreadQueueListResponse> {
    send_request(
        stream,
        "thread/queue/list",
        id,
        Some(serde_json::to_value(ThreadQueueListParams {
            thread_id: thread_id.to_string(),
            cursor: None,
            limit: None,
        })?),
    )
    .await?;
    websocket_response(stream, id).await
}

pub(super) async fn add(
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

pub(super) async fn response<T: serde::de::DeserializeOwned>(
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

pub(super) fn text_input(text: &str) -> Vec<UserInput> {
    vec![UserInput::Text {
        text: text.to_string(),
        text_elements: Vec::new(),
    }]
}

pub(super) async fn wait_for_command_start(app: &mut TestAppServer) -> Result<()> {
    timeout(DEFAULT_TIMEOUT, async {
        loop {
            let notification = app
                .read_stream_until_notification_message("item/started")
                .await?;
            let started: ItemStartedNotification = serde_json::from_value(
                notification
                    .params
                    .expect("item/started params should be present"),
            )?;
            if matches!(started.item, ThreadItem::CommandExecution { .. }) {
                return Ok::<(), anyhow::Error>(());
            }
        }
    })
    .await??;
    Ok(())
}

async fn turn_completed_before_queue_changed(app: &mut TestAppServer) -> Result<()> {
    let mut turn_completed = false;
    loop {
        let JSONRPCMessage::Notification(notification) = app.read_next_message().await? else {
            continue;
        };
        match notification.method.as_str() {
            "turn/completed" => turn_completed = true,
            "thread/queue/changed" => {
                anyhow::ensure!(
                    turn_completed,
                    "thread/queue/changed arrived before turn/completed"
                );
                return Ok(());
            }
            _ => {}
        }
    }
}

fn request_body_contains(request: &wiremock::Request, text: &str) -> bool {
    String::from_utf8(request.body.clone())
        .ok()
        .is_some_and(|body| body.contains(text))
}

pub(super) fn write_config(codex_home: &Path, server_uri: &str) -> std::io::Result<()> {
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
goals = true

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
