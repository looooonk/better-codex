use std::time::Duration;

use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::create_fake_paginated_rollout;
use app_test_support::create_final_assistant_message_sse_response;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::create_mock_responses_server_sequence;
use app_test_support::create_shell_command_sse_response;
use app_test_support::rollout_path;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::QueuedSubmission;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadQueueListParams;
use codex_app_server_protocol::ThreadQueueListResponse;
use codex_app_server_protocol::ThreadQueueStartParams;
use codex_app_server_protocol::ThreadQueueStartResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadRevertParams;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnInterruptResponse;
use codex_protocol::ThreadId;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::user_input::UserInput;
use codex_rollout::RolloutItem;
use codex_rollout::append_rollout_item_to_path;
use codex_state::BlockedSubmissionRetryPolicy;
use codex_state::QueueClaimResult;
use codex_state::QueuedSubmissionRecord;
use codex_state::QueuedSubmissionState;
use codex_state::QueuedSubmissionTerminalStatus;
use codex_state::StateRuntime;
use codex_state::ThreadQueuePauseReason;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

use super::thread_queue::add;
use super::thread_queue::response;
use super::thread_queue::text_input;
use super::thread_queue::wait_for_command_start;
use super::thread_queue::write_config;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

struct SeededQueue {
    thread_id: ThreadId,
    raw_thread_id: String,
    turn_id: String,
    owner: QueuedSubmissionRecord,
    follower: QueuedSubmissionRecord,
    pause_reason: Option<ThreadQueuePauseReason>,
}

#[tokio::test]
async fn failed_revert_errors_before_surviving_follower_dispatch() -> Result<()> {
    let server = create_mock_responses_server_sequence(vec![
        create_shell_command_sse_response(
            vec!["sleep".to_string(), "30".to_string()],
            /*workdir*/ None,
            Some(30_000),
            "hold-failed-revert",
        )?,
        create_final_assistant_message_sse_response("follower completed")?,
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
            history_mode: Some(codex_app_server_protocol::ThreadHistoryMode::Paginated),
            ..Default::default()
        })
        .await?;
    let started: ThreadStartResponse = response(&mut app, start_id).await?;

    add(
        &mut app,
        &started.thread.id,
        "active queued owner",
        "client-failed-revert-owner",
    )
    .await?;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/started"),
    )
    .await??;
    wait_for_command_start(&mut app).await?;
    add(
        &mut app,
        &started.thread.id,
        "surviving follower",
        "client-failed-revert-follower",
    )
    .await?;

    let revert_id = app
        .send_thread_revert_request(ThreadRevertParams {
            thread_id: started.thread.id.clone(),
            before_turn_id: "missing-turn".to_string(),
        })
        .await?;
    let revert_error = timeout(DEFAULT_TIMEOUT, async {
        loop {
            match app.read_next_message().await? {
                JSONRPCMessage::Error(error) if error.id == RequestId::Integer(revert_id) => {
                    return Ok::<_, anyhow::Error>(error);
                }
                JSONRPCMessage::Notification(notification)
                    if notification.method == "turn/started" =>
                {
                    anyhow::bail!("queued follower started before the revert error response");
                }
                JSONRPCMessage::Request(_)
                | JSONRPCMessage::Notification(_)
                | JSONRPCMessage::Response(_)
                | JSONRPCMessage::Error(_) => {}
            }
        }
    })
    .await??;
    assert_eq!(revert_error.error.message, "turn not found: missing-turn");
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

    let requests = server.received_requests().await.expect("response requests");
    assert_eq!(requests.len(), 2);
    assert!(String::from_utf8(requests[1].body.clone())?.contains("surviving follower"));
    Ok(())
}

#[tokio::test]
async fn immediate_loaded_rejoin_list_recovers_crash_left_owner() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    write_config(codex_home.path(), &server.uri())?;
    let raw_thread_id = create_fake_paginated_rollout(
        codex_home.path(),
        "2025-01-07T00-30-00",
        "2025-01-07T00:30:00Z",
        "Loaded rejoin queue recovery",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let thread_id = ThreadId::from_string(&raw_thread_id)?;
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build()
        .await?;
    timeout(DEFAULT_TIMEOUT, app.initialize()).await??;
    let resume_id = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: raw_thread_id.clone(),
            ..Default::default()
        })
        .await?;
    let _: ThreadResumeResponse = response(&mut app, resume_id).await?;

    let state =
        StateRuntime::init(codex_home.path().to_path_buf(), "mock_provider".to_string()).await?;
    let owner = state
        .enqueue_queued_submission(
            thread_id,
            &serde_json::to_string(&text_input_core("crash owner"))?,
            "client-crash-owner",
        )
        .await?;
    let follower = state
        .enqueue_queued_submission(
            thread_id,
            &serde_json::to_string(&text_input_core("crash follower"))?,
            "client-crash-follower",
        )
        .await?;
    assert!(matches!(
        state
            .claim_queued_submission(thread_id, Some(&owner.id), "turn-crash")
            .await?,
        QueueClaimResult::Claimed(_)
    ));
    assert!(
        state
            .mark_queued_submission_inflight(thread_id, "turn-crash")
            .await?
    );

    let resume_id = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: raw_thread_id.clone(),
            ..Default::default()
        })
        .await?;
    let _: ThreadResumeResponse = response(&mut app, resume_id).await?;
    let list_id = app
        .send_raw_request(
            "thread/queue/list",
            Some(serde_json::to_value(ThreadQueueListParams {
                thread_id: raw_thread_id,
                cursor: None,
                limit: None,
            })?),
        )
        .await?;
    let listed: ThreadQueueListResponse =
        timeout(Duration::from_secs(2), response(&mut app, list_id)).await??;
    assert_eq!(
        listed.data,
        vec![
            QueuedSubmission {
                id: owner.id,
                input: text_input("crash owner"),
                client_user_message_id: owner.client_user_message_id,
            },
            QueuedSubmission {
                id: follower.id,
                input: text_input("crash follower"),
                client_user_message_id: follower.client_user_message_id,
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
    Ok(())
}

#[tokio::test]
async fn list_repair_returns_follower_before_dispatching_it_once() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("follower done").await;
    let codex_home = TempDir::new()?;
    write_config(codex_home.path(), &server.uri())?;
    let filename_ts = "2025-01-07T00-45-00";
    let raw_thread_id = create_fake_paginated_rollout(
        codex_home.path(),
        filename_ts,
        "2025-01-07T00:45:00Z",
        "List repair queue continuation",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let thread_id = ThreadId::from_string(&raw_thread_id)?;
    let owner_turn_id = "turn-completed-owner";
    let path = rollout_path(codex_home.path(), filename_ts, &raw_thread_id);
    append_rollout_item_to_path(&path, &turn_started(owner_turn_id)).await?;
    append_rollout_item_to_path(
        &path,
        &RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id,
            turn_id: owner_turn_id.to_string(),
            item: TurnItem::UserMessage(UserMessageItem {
                id: "owner-user-message".to_string(),
                client_id: Some("client-completed-owner".to_string()),
                content: text_input_core("already completed owner"),
            }),
            completed_at_ms: 1,
        })),
    )
    .await?;
    append_rollout_item_to_path(
        &path,
        &RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: owner_turn_id.to_string(),
            last_agent_message: None,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        })),
    )
    .await?;
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build()
        .await?;
    timeout(DEFAULT_TIMEOUT, app.initialize()).await??;
    let resume_id = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: raw_thread_id.clone(),
            ..Default::default()
        })
        .await?;
    let _: ThreadResumeResponse = response(&mut app, resume_id).await?;

    let state =
        StateRuntime::init(codex_home.path().to_path_buf(), "mock_provider".to_string()).await?;
    let owner = state
        .enqueue_queued_submission(
            thread_id,
            &serde_json::to_string(&text_input_core("already completed owner"))?,
            "client-completed-owner",
        )
        .await?;
    let follower = state
        .enqueue_queued_submission(
            thread_id,
            &serde_json::to_string(&text_input_core("dispatch after list"))?,
            "client-dispatch-after-list",
        )
        .await?;
    assert!(matches!(
        state
            .claim_queued_submission(thread_id, Some(&owner.id), owner_turn_id)
            .await?,
        QueueClaimResult::Claimed(_)
    ));
    assert!(
        state
            .mark_queued_submission_inflight(thread_id, owner_turn_id)
            .await?
    );

    let list_id = app
        .send_raw_request(
            "thread/queue/list",
            Some(serde_json::to_value(ThreadQueueListParams {
                thread_id: raw_thread_id.clone(),
                cursor: None,
                limit: None,
            })?),
        )
        .await?;
    let listed: ThreadQueueListResponse = response(&mut app, list_id).await?;
    assert_eq!(
        listed.data,
        vec![QueuedSubmission {
            id: follower.id.clone(),
            input: text_input("dispatch after list"),
            client_user_message_id: follower.client_user_message_id.clone(),
        }]
    );
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let requests = server.received_requests().await.expect("response requests");
    assert_eq!(requests.len(), 1);
    assert!(String::from_utf8(requests[0].body.clone())?.contains("dispatch after list"));

    let list_id = app
        .send_raw_request(
            "thread/queue/list",
            Some(serde_json::to_value(ThreadQueueListParams {
                thread_id: raw_thread_id,
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
async fn immediate_list_after_queued_start_returns_followers_before_terminal() -> Result<()> {
    let server = create_mock_responses_server_sequence(vec![create_shell_command_sse_response(
        vec!["sleep".to_string(), "30".to_string()],
        /*workdir*/ None,
        Some(30_000),
        "hold-queued-turn",
    )?])
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
            history_mode: Some(codex_app_server_protocol::ThreadHistoryMode::Paginated),
            ..Default::default()
        })
        .await?;
    let started: ThreadStartResponse = response(&mut app, start_id).await?;
    let thread_id = ThreadId::from_string(&started.thread.id)?;

    let state =
        StateRuntime::init(codex_home.path().to_path_buf(), "mock_provider".to_string()).await?;
    let owner = state
        .enqueue_queued_submission(
            thread_id,
            &serde_json::to_string(&text_input_core("held owner"))?,
            "client-held-owner",
        )
        .await?;
    let follower = state
        .enqueue_queued_submission(
            thread_id,
            &serde_json::to_string(&text_input_core("visible follower"))?,
            "client-visible-follower",
        )
        .await?;
    assert!(matches!(
        state
            .claim_queued_submission(thread_id, Some(&owner.id), "turn-seed")
            .await?,
        QueueClaimResult::Claimed(_)
    ));
    assert!(
        state
            .block_indeterminate_queued_submission(
                thread_id,
                &owner.id,
                "turn-seed",
                BlockedSubmissionRetryPolicy::Allowed,
            )
            .await?
    );

    let queue_start_id = app
        .send_raw_request(
            "thread/queue/start",
            Some(serde_json::to_value(ThreadQueueStartParams {
                thread_id: started.thread.id.clone(),
                queued_submission_id: None,
            })?),
        )
        .await?;
    let queue_start: ThreadQueueStartResponse = response(&mut app, queue_start_id).await?;
    let list_id = app
        .send_raw_request(
            "thread/queue/list",
            Some(serde_json::to_value(ThreadQueueListParams {
                thread_id: started.thread.id.clone(),
                cursor: None,
                limit: None,
            })?),
        )
        .await?;
    let listed: ThreadQueueListResponse =
        timeout(Duration::from_secs(2), response(&mut app, list_id)).await??;
    assert_eq!(
        listed.data,
        vec![QueuedSubmission {
            id: follower.id,
            input: text_input("visible follower"),
            client_user_message_id: follower.client_user_message_id,
        }]
    );
    wait_for_command_start(&mut app).await?;

    let interrupt_id = app
        .send_turn_interrupt_request(TurnInterruptParams {
            thread_id: started.thread.id,
            turn_id: queue_start.turn.id,
        })
        .await?;
    let _: TurnInterruptResponse = response(&mut app, interrupt_id).await?;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    Ok(())
}

#[tokio::test]
async fn paginated_abort_recovery_preserves_exact_reason_across_restart() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    write_config(codex_home.path(), &server.uri())?;
    let state =
        StateRuntime::init(codex_home.path().to_path_buf(), "mock_provider".to_string()).await?;
    let mut seeded = Vec::new();
    for (index, reason, pause_reason) in [
        (
            0,
            TurnAbortReason::Interrupted,
            Some(ThreadQueuePauseReason::Interrupted),
        ),
        (
            1,
            TurnAbortReason::BudgetLimited,
            Some(ThreadQueuePauseReason::BudgetLimited),
        ),
        (2, TurnAbortReason::Replaced, None),
        (3, TurnAbortReason::ReviewEnded, None),
    ] {
        let filename_ts = format!("2025-01-07T0{}-00-00", index + 1);
        let meta_rfc3339 = format!("2025-01-07T0{}:00:00Z", index + 1);
        let raw_thread_id = create_fake_paginated_rollout(
            codex_home.path(),
            &filename_ts,
            &meta_rfc3339,
            "Stored queue recovery thread",
            Some("mock_provider"),
            /*git_info*/ None,
        )?;
        let thread_id = ThreadId::from_string(&raw_thread_id)?;
        let turn_id = format!("turn-queued-{index}");
        let client_id = format!("client-queued-{index}");
        let queued_text = format!("queued owner {index}");
        let path = rollout_path(codex_home.path(), &filename_ts, &raw_thread_id);
        append_rollout_item_to_path(&path, &turn_started(&turn_id)).await?;
        append_rollout_item_to_path(
            &path,
            &RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
                thread_id,
                turn_id: turn_id.clone(),
                item: TurnItem::UserMessage(UserMessageItem {
                    id: format!("user-{index}"),
                    client_id: Some(client_id.clone()),
                    content: text_input_core(&queued_text),
                }),
                completed_at_ms: 1,
            })),
        )
        .await?;
        append_rollout_item_to_path(
            &path,
            &RolloutItem::EventMsg(EventMsg::TurnAborted(TurnAbortedEvent {
                turn_id: Some(turn_id.clone()),
                reason: reason.clone(),
                started_at: None,
                completed_at: None,
                duration_ms: None,
            })),
        )
        .await?;
        if reason == TurnAbortReason::Replaced {
            append_rollout_item_to_path(&path, &turn_started("turn-newer")).await?;
            append_rollout_item_to_path(
                &path,
                &RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
                    turn_id: "turn-newer".to_string(),
                    last_agent_message: None,
                    error: None,
                    started_at: None,
                    completed_at: None,
                    duration_ms: None,
                    time_to_first_token_ms: None,
                })),
            )
            .await?;
        }

        let owner = state
            .enqueue_queued_submission(
                thread_id,
                &serde_json::to_string(&text_input_core(&queued_text))?,
                &client_id,
            )
            .await?;
        let follower = state
            .enqueue_queued_submission(
                thread_id,
                &serde_json::to_string(&text_input_core(&format!("follower {index}")))?,
                &format!("client-follower-{index}"),
            )
            .await?;
        assert!(matches!(
            state
                .claim_queued_submission(thread_id, Some(&owner.id), &turn_id)
                .await?,
            QueueClaimResult::Claimed(_)
        ));
        assert!(
            state
                .mark_queued_submission_inflight(thread_id, &turn_id)
                .await?
        );
        seeded.push(SeededQueue {
            thread_id,
            raw_thread_id,
            turn_id,
            owner,
            follower,
            pause_reason,
        });
    }
    state.close().await;

    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build()
        .await?;
    timeout(DEFAULT_TIMEOUT, app.initialize()).await??;
    for queue in &seeded {
        let request_id = app
            .send_raw_request(
                "thread/queue/list",
                Some(serde_json::to_value(ThreadQueueListParams {
                    thread_id: queue.raw_thread_id.clone(),
                    cursor: None,
                    limit: None,
                })?),
            )
            .await?;
        let listed: ThreadQueueListResponse = response(&mut app, request_id).await?;
        assert_eq!(
            listed.data,
            vec![codex_app_server_protocol::QueuedSubmission {
                id: queue.follower.id.clone(),
                input: serde_json::from_str(&queue.follower.payload)?,
                client_user_message_id: queue.follower.client_user_message_id.clone(),
            }]
        );
    }
    assert!(
        server
            .received_requests()
            .await
            .expect("response requests")
            .is_empty()
    );

    let state =
        StateRuntime::init(codex_home.path().to_path_buf(), "mock_provider".to_string()).await?;
    for queue in seeded {
        assert_eq!(
            state.thread_queue_pause_reason(queue.thread_id).await?,
            queue.pause_reason
        );
        assert_eq!(
            state
                .queued_submission(queue.thread_id, &queue.owner.id)
                .await?
                .expect("terminal owner tombstone"),
            QueuedSubmissionRecord {
                payload: "[]".to_string(),
                state: QueuedSubmissionState::Terminal,
                turn_id: Some(queue.turn_id),
                terminal_status: Some(QueuedSubmissionTerminalStatus::Interrupted),
                ..queue.owner
            }
        );
    }
    Ok(())
}

fn turn_started(turn_id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: turn_id.to_string(),
        trace_id: None,
        started_at: None,
        model_context_window: None,
        collaboration_mode_kind: Default::default(),
    }))
}

fn text_input_core(text: &str) -> Vec<UserInput> {
    vec![UserInput::Text {
        text: text.to_string(),
        text_elements: Vec::new(),
    }]
}
