use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::ThreadQueueListParams;
use codex_app_server_protocol::ThreadQueueListResponse;
use codex_app_server_protocol::ThreadQueueStartParams;
use codex_app_server_protocol::ThreadQueueStartResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;
use codex_state::QueuedSubmissionAdmissionRejection;
use codex_state::QueuedSubmissionRecord;
use codex_state::QueuedSubmissionState;
use codex_state::QueuedSubmissionTerminalStatus;
use codex_state::StateRuntime;
use codex_state::ThreadQueuePauseReason;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::sleep;
use tokio::time::timeout;

use super::thread_queue::add;
use super::thread_queue::response;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
const HOOK_REJECTED_PROMPT: &str = "reject this queued prompt";
const INTERRUPTED_PROMPT: &str = "interrupt this queued prompt";

#[tokio::test]
async fn hook_rejected_queue_owner_is_durable_across_restart_without_model_request() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let hook_script = write_rejecting_hook(codex_home.path())?;
    write_config(codex_home.path(), &server.uri(), &hook_script)?;

    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(INITIALIZE_TIMEOUT, app.initialize()).await??;
    let start_id = app
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let started: ThreadStartResponse = response(&mut app, start_id).await?;
    let thread_id = ThreadId::from_string(&started.thread.id)?;

    let rejected = add(
        &mut app,
        &started.thread.id,
        HOOK_REJECTED_PROMPT,
        "client-hook-rejected",
    )
    .await?;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("thread/queue/changed"),
    )
    .await??;
    let turn_started = read_turn_started(&mut app).await?;
    let completed = read_turn_completed(&mut app).await?;
    assert_eq!(completed.turn.id, turn_started.turn.id);
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("thread/queue/changed"),
    )
    .await??;

    let state =
        StateRuntime::init(codex_home.path().to_path_buf(), "mock_provider".to_string()).await?;
    let terminal = wait_for_queued_submission(
        &state,
        thread_id,
        &rejected.queued_submission.id,
        |record| record.state == QueuedSubmissionState::Terminal,
    )
    .await?;
    assert_eq!(
        terminal.turn_id.as_deref(),
        Some(turn_started.turn.id.as_str())
    );
    assert_eq!(
        terminal.admission_rejection,
        Some(QueuedSubmissionAdmissionRejection::Hook)
    );
    assert_eq!(
        terminal.terminal_status,
        Some(QueuedSubmissionTerminalStatus::Failed)
    );
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );
    state.close().await;
    drop(app);

    let reopened =
        StateRuntime::init(codex_home.path().to_path_buf(), "mock_provider".to_string()).await?;
    assert_eq!(
        reopened
            .queued_submission(thread_id, &rejected.queued_submission.id)
            .await?,
        Some(terminal)
    );
    reopened.close().await;

    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(INITIALIZE_TIMEOUT, app.initialize()).await??;
    resume(&mut app, &started.thread.id).await?;
    add(
        &mut app,
        &started.thread.id,
        "accepted after rejected owner restart",
        "client-after-restart",
    )
    .await?;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("thread/queue/changed"),
    )
    .await??;
    let follower_started = read_turn_started(&mut app).await?;
    let follower_completed = read_turn_completed(&mut app).await?;
    assert_eq!(follower_completed.turn.id, follower_started.turn.id);
    assert_eq!(follower_completed.turn.status, TurnStatus::Completed);

    let requests = server.received_requests().await.expect("response request");
    assert_eq!(requests.len(), 1);
    let request = String::from_utf8(requests[0].body.clone())?;
    assert!(request.contains("accepted after rejected owner restart"));
    assert!(!request.contains(HOOK_REJECTED_PROMPT));
    Ok(())
}

#[tokio::test]
async fn pre_persistence_abort_retries_owner_before_follower_once_after_restart() -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let hook_started = codex_home.path().join("slow-hook-started");
    let hook_script = write_slow_first_hook(codex_home.path(), &hook_started)?;
    write_config(codex_home.path(), &server.uri(), &hook_script)?;

    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(INITIALIZE_TIMEOUT, app.initialize()).await??;
    let start_id = app
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let started: ThreadStartResponse = response(&mut app, start_id).await?;
    let thread_id = ThreadId::from_string(&started.thread.id)?;

    let owner = add(
        &mut app,
        &started.thread.id,
        INTERRUPTED_PROMPT,
        "client-interrupted-owner",
    )
    .await?;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("thread/queue/changed"),
    )
    .await??;
    let interrupted_turn = read_turn_started(&mut app).await?;
    timeout(DEFAULT_TIMEOUT, async {
        while !hook_started.exists() {
            sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await?;

    app.interrupt_turn_and_wait_for_aborted(
        started.thread.id.clone(),
        interrupted_turn.turn.id,
        DEFAULT_TIMEOUT,
    )
    .await?;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("thread/queue/changed"),
    )
    .await??;

    let follower = add(
        &mut app,
        &started.thread.id,
        "queued follower after retry",
        "client-retry-follower",
    )
    .await?;
    timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("thread/queue/changed"),
    )
    .await??;

    let state =
        StateRuntime::init(codex_home.path().to_path_buf(), "mock_provider".to_string()).await?;
    let pending_owner =
        wait_for_queued_submission(&state, thread_id, &owner.queued_submission.id, |record| {
            record.state == QueuedSubmissionState::Pending && record.turn_id.is_none()
        })
        .await?;
    assert_eq!(pending_owner.admission_rejection, None);
    assert_eq!(pending_owner.terminal_status, None);
    assert_eq!(
        state.thread_queue_pause_reason(thread_id).await?,
        Some(ThreadQueuePauseReason::Interrupted)
    );
    state.close().await;

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
    let listed: ThreadQueueListResponse = response(&mut app, list_id).await?;
    assert_eq!(
        listed.data,
        vec![
            owner.queued_submission.clone(),
            follower.queued_submission.clone()
        ]
    );
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );
    drop(app);

    let reopened =
        StateRuntime::init(codex_home.path().to_path_buf(), "mock_provider".to_string()).await?;
    assert_eq!(
        reopened
            .queued_submission(thread_id, &owner.queued_submission.id)
            .await?,
        Some(pending_owner)
    );
    reopened.close().await;

    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(INITIALIZE_TIMEOUT, app.initialize()).await??;
    resume(&mut app, &started.thread.id).await?;
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );

    let queue_start_id = app
        .send_raw_request(
            "thread/queue/start",
            Some(serde_json::to_value(ThreadQueueStartParams {
                thread_id: started.thread.id.clone(),
                queued_submission_id: Some(owner.queued_submission.id),
            })?),
        )
        .await?;
    let retry: ThreadQueueStartResponse = response(&mut app, queue_start_id).await?;
    assert_eq!(retry.turn.status, TurnStatus::InProgress);
    let owner_started = read_turn_started(&mut app).await?;
    assert_eq!(owner_started.turn.id, retry.turn.id);
    let owner_completed = read_turn_completed(&mut app).await?;
    assert_eq!(owner_completed.turn.id, retry.turn.id);
    assert_eq!(owner_completed.turn.status, TurnStatus::Completed);
    let follower_started = read_turn_started(&mut app).await?;
    let follower_completed = read_turn_completed(&mut app).await?;
    assert_eq!(follower_completed.turn.id, follower_started.turn.id);
    assert_eq!(follower_completed.turn.status, TurnStatus::Completed);

    let final_list_id = app
        .send_raw_request(
            "thread/queue/list",
            Some(serde_json::to_value(ThreadQueueListParams {
                thread_id: started.thread.id,
                cursor: None,
                limit: None,
            })?),
        )
        .await?;
    let final_list: ThreadQueueListResponse = response(&mut app, final_list_id).await?;
    assert!(final_list.data.is_empty());

    let requests = server.received_requests().await.expect("response requests");
    assert_eq!(requests.len(), 2);
    let bodies = requests
        .iter()
        .map(|request| String::from_utf8(request.body.clone()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert!(bodies[0].contains(INTERRUPTED_PROMPT));
    assert!(!bodies[0].contains("queued follower after retry"));
    assert!(bodies[1].contains("queued follower after retry"));
    Ok(())
}

async fn resume(app: &mut TestAppServer, thread_id: &str) -> Result<()> {
    let request_id = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread_id.to_string(),
            ..Default::default()
        })
        .await?;
    let _: ThreadResumeResponse = response(app, request_id).await?;
    Ok(())
}

async fn read_turn_started(app: &mut TestAppServer) -> Result<TurnStartedNotification> {
    let notification = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/started"),
    )
    .await??;
    Ok(serde_json::from_value(
        notification.params.expect("turn/started params"),
    )?)
}

async fn read_turn_completed(app: &mut TestAppServer) -> Result<TurnCompletedNotification> {
    let notification = timeout(
        DEFAULT_TIMEOUT,
        app.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    Ok(serde_json::from_value(
        notification.params.expect("turn/completed params"),
    )?)
}

async fn wait_for_queued_submission(
    state: &StateRuntime,
    thread_id: ThreadId,
    queued_submission_id: &str,
    predicate: impl Fn(&QueuedSubmissionRecord) -> bool,
) -> Result<QueuedSubmissionRecord> {
    timeout(DEFAULT_TIMEOUT, async {
        loop {
            if let Some(record) = state
                .queued_submission(thread_id, queued_submission_id)
                .await?
                && predicate(&record)
            {
                return Ok::<QueuedSubmissionRecord, anyhow::Error>(record);
            }
            sleep(Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await?
}

fn write_rejecting_hook(codex_home: &Path) -> Result<std::path::PathBuf> {
    let script_path = codex_home.join("reject_queue_hook.py");
    let blocked_prompt = serde_json::to_string(HOOK_REJECTED_PROMPT)?;
    std::fs::write(
        &script_path,
        format!(
            r#"import json
import sys

payload = json.load(sys.stdin)
if payload.get("prompt") == {blocked_prompt}:
    print(json.dumps({{"decision": "block", "reason": "queue test rejection"}}))
"#,
        ),
    )?;
    Ok(script_path)
}

fn write_slow_first_hook(codex_home: &Path, marker: &Path) -> Result<std::path::PathBuf> {
    let script_path = codex_home.join("slow_queue_hook.py");
    let interrupted_prompt = serde_json::to_string(INTERRUPTED_PROMPT)?;
    std::fs::write(
        &script_path,
        format!(
            r#"import json
from pathlib import Path
import sys
import time

payload = json.load(sys.stdin)
marker = Path({marker:?})
if payload.get("prompt") == {interrupted_prompt} and not marker.exists():
    marker.write_text("started", encoding="utf-8")
    time.sleep(60)
"#,
            marker = marker.display().to_string(),
        ),
    )?;
    Ok(script_path)
}

fn write_config(codex_home: &Path, server_uri: &str, hook_script: &Path) -> Result<()> {
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
hooks = true

[model_providers.mock_provider]
name = "Mock provider"
base_url = "{server_uri}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#,
        ),
    )?;
    let command = format!("python3 {}", hook_script.display());
    std::fs::write(
        codex_home.join("managed_config.toml"),
        format!(
            r#"
[[hooks.UserPromptSubmit]]

[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = {command}
"#,
            command = json!(command),
        ),
    )?;
    Ok(())
}
