use std::time::Duration;

use anyhow::Context;
use codex_core::NewThread;
use codex_exec_server::ExecutorFileSystem;
use codex_exec_server::RemoveOptions;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use codex_utils_path_uri::PathUri;
use core_test_support::responses;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::mount_sse_once;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_wine_exec;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::rmcp_client::remote_aware_environment_id;
use super::rmcp_client::remote_aware_stdio_server_bin;

const SERVER_NAME: &str = "cached_rmcp";
const NAMESPACE: &str = "mcp__cached_rmcp";

fn user_turn(prompt: &str) -> Op {
    Op::UserInput {
        items: vec![UserInput::Text {
            text: prompt.to_string(),
            text_elements: Vec::new(),
        }],
        final_output_json_schema: None,
        responsesapi_client_metadata: None,
        additional_context: Default::default(),
        thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
            approval_policy: Some(AskForApproval::Never),
            permission_profile: Some(PermissionProfile::Disabled),
            ..Default::default()
        },
    }
}

fn process_label(pid: &str) -> String {
    format!("rmcp-test-process-{pid}")
}

fn assert_definition(response: &ResponseMock, namespace_description: &str, tool_description: &str) {
    let body = response.single_request().body_json();
    let namespace = body
        .get("tools")
        .and_then(Value::as_array)
        .and_then(|tools| {
            tools
                .iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some(NAMESPACE))
        })
        .expect("request should contain the MCP namespace");
    assert_eq!(
        namespace.get("description").and_then(Value::as_str),
        Some(namespace_description)
    );
    assert_eq!(
        responses::namespace_child_tool(&body, NAMESPACE, "echo")
            .and_then(|tool| tool.get("description"))
            .and_then(Value::as_str),
        Some(tool_description)
    );
}

async fn wait_for_new_pid(
    fs: &dyn ExecutorFileSystem,
    path: &PathUri,
    previous_pid: Option<&str>,
) -> anyhow::Result<String> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(contents) = fs.read_file_text(path, /*sandbox*/ None).await {
                let pid = contents.trim();
                if !pid.is_empty() && Some(pid) != previous_pid {
                    return pid.to_string();
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("timed out waiting for a new MCP server process")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn regular_mcp_catalog_is_reused_before_a_new_session_finishes_starting() -> anyhow::Result<()>
{
    skip_if_wine_exec!(
        Ok(()),
        "requires a Windows test_stdio_server in the Wine-exec environment"
    );
    skip_if_no_network!(Ok(()));

    let responses_server = responses::start_mock_server().await;
    let command = remote_aware_stdio_server_bin()?;
    let environment_id = remote_aware_environment_id();
    let fixture = test_codex()
        .with_model_info_override("gpt-5.4", |model| model.supports_search_tool = false)
        .with_config(move |config| {
            let barrier_file = config.cwd.join("allow-initialize");
            let pid_file = config.cwd.join("mcp.pid");
            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                SERVER_NAME.to_string(),
                serde_json::from_value(json!({
                    "command": command,
                    "environment_id": environment_id,
                    "env": {
                        "MCP_TEST_DYNAMIC_SERVER_METADATA": "1",
                        "MCP_TEST_INITIALIZE_BARRIER_FILE": barrier_file,
                        "MCP_TEST_PID_FILE": pid_file,
                    },
                    "enabled_tools": ["echo"],
                    "startup_timeout_sec": 10,
                }))
                .expect("test MCP server configuration"),
            );
            config
                .mcp_servers
                .set(servers)
                .expect("test MCP server configuration");
        })
        .build_with_auto_env(&responses_server)
        .await?;
    let fs = fixture.fs();
    let barrier_file = PathUri::from_host_native_path(fixture.config.cwd.join("allow-initialize"))?;
    let pid_file = PathUri::from_host_native_path(fixture.config.cwd.join("mcp.pid"))?;
    fs.write_file(&barrier_file, b"ready".to_vec(), /*sandbox*/ None)
        .await?;
    let cold_response = mount_sse_once(
        &responses_server,
        responses::sse(vec![
            responses::ev_response_created("cold"),
            responses::ev_assistant_message("cold-message", "done"),
            responses::ev_completed("cold"),
        ]),
    )
    .await;
    fixture.codex.submit(user_turn("inspect the tools")).await?;
    wait_for_event(&fixture.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let first_pid = wait_for_new_pid(fs.as_ref(), &pid_file, /*previous_pid*/ None).await?;
    let first_process = process_label(&first_pid);
    assert_definition(
        &cold_response,
        &format!("Use the tools from {first_process}."),
        &format!("Echo from {first_process}."),
    );
    fs.remove(
        &barrier_file,
        RemoveOptions {
            recursive: false,
            force: false,
        },
        /*sandbox*/ None,
    )
    .await?;
    let NewThread {
        thread: second_thread,
        ..
    } = fixture
        .thread_manager
        .start_thread(fixture.config.clone())
        .await?;
    let second_pid = wait_for_new_pid(fs.as_ref(), &pid_file, Some(&first_pid)).await?;
    assert_ne!(second_pid, first_pid);
    let cached_response = mount_sse_once(
        &responses_server,
        responses::sse(vec![
            responses::ev_response_created("cached"),
            responses::ev_assistant_message("cached-message", "done"),
            responses::ev_completed("cached"),
        ]),
    )
    .await;
    second_thread.submit(user_turn("inspect the tools")).await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while cached_response.requests().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("cached definitions should reach inference before initialization")?;
    assert_definition(
        &cached_response,
        &format!("Tools in the {NAMESPACE} namespace."),
        &format!("Echo from {first_process}."),
    );
    fs.write_file(&barrier_file, b"ready".to_vec(), /*sandbox*/ None)
        .await?;
    wait_for_event(&second_thread, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    fixture.codex.shutdown_and_wait().await?;
    second_thread.shutdown_and_wait().await?;
    responses_server.verify().await;
    Ok(())
}
