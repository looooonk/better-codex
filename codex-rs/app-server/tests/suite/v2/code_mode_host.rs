use std::collections::BTreeMap;
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use app_test_support::write_mock_responses_config_toml;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use core_test_support::skip_if_remote;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::time::timeout;

#[cfg(any(target_os = "macos", windows))]
const READ_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 60);
#[cfg(not(any(target_os = "macos", windows)))]
const READ_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 10);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn app_server_shares_grpc_code_mode_host_across_threads() -> Result<()> {
    skip_if_remote!(
        Ok(()),
        "the code-mode host fixture and app-server must share loopback"
    );
    let host_program = codex_utils_cargo_bin::cargo_bin("codex-code-mode-host")?;
    let mut code_mode_host = Command::new(host_program)
        .args(["--listen", "grpc://127.0.0.1:0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .context("failed to start gRPC code-mode host")?;
    let stdout = code_mode_host
        .stdout
        .take()
        .context("gRPC code-mode host stdout was not captured")?;
    let host_url = timeout(READ_TIMEOUT, BufReader::new(stdout).lines().next_line())
        .await
        .context("timed out waiting for gRPC code-mode host URL")??
        .context("gRPC code-mode host exited before publishing its URL")?;

    let model_server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_sequence(
        &model_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("resp-1"),
                responses::ev_custom_tool_call(
                    "first-remote-cell",
                    "exec",
                    "text('remote app-server host')",
                ),
                responses::ev_completed("resp-1"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("msg-1", "Done"),
                responses::ev_completed("resp-2"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-3"),
                responses::ev_custom_tool_call(
                    "second-remote-cell",
                    "exec",
                    "text('remote app-server host')",
                ),
                responses::ev_completed("resp-3"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("msg-2", "Done"),
                responses::ev_completed("resp-4"),
            ]),
        ],
    )
    .await;

    let codex_home = TempDir::new()?;
    write_mock_responses_config_toml(
        codex_home.path(),
        &model_server.uri(),
        &BTreeMap::from([(Feature::CodeModeOnly, true)]),
        /*auto_compact_limit*/ 1_000_000,
        /*requires_openai_auth*/ None,
        "mock_provider",
        "compact",
    )?;
    let original_config = std::fs::read_to_string(codex_home.path().join("config.toml"))?;
    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .with_args(&["--code-mode-host", &host_url])
        .build()
        .await?;
    timeout(READ_TIMEOUT, app_server.initialize()).await??;

    for prompt in ["run the first remote cell", "run the second remote cell"] {
        let request_id = app_server
            .send_thread_start_request_with_auto_env(ThreadStartParams::default())
            .await?;
        let response = timeout(
            READ_TIMEOUT,
            app_server.read_stream_until_response_message(RequestId::Integer(request_id)),
        )
        .await??;
        let ThreadStartResponse { thread, .. } = to_response(response)?;
        let completed = timeout(
            READ_TIMEOUT,
            app_server.start_turn_and_wait_for_completion(TurnStartParams {
                thread_id: thread.id,
                input: vec![UserInput::Text {
                    text: prompt.to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            }),
        )
        .await??;
        assert_eq!(completed.turn.status, TurnStatus::Completed);
    }

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 4);
    for (request, call_id) in [
        (&requests[1], "first-remote-cell"),
        (&requests[3], "second-remote-cell"),
    ] {
        let output = request.custom_tool_call_output(call_id);
        assert_eq!(
            output["output"]
                .as_array()
                .and_then(|items| items.last())
                .cloned(),
            Some(json!({
                "type": "input_text",
                "text": "remote app-server host",
            }))
        );
    }
    assert_eq!(
        std::fs::read_to_string(codex_home.path().join("config.toml"))?,
        original_config
    );
    Ok(())
}
