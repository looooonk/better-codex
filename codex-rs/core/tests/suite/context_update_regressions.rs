use anyhow::Context;
use anyhow::Result;
use codex_features::Feature;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::protocol::COLLABORATION_MODE_CLOSE_TAG;
use codex_protocol::protocol::COLLABORATION_MODE_OPEN_TAG;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const MULTI_AGENT_NAMESPACE: &str = "multi_agent_v1";
const SPAWN_CALL_ID: &str = "spawn-context-worker";
const CLOSE_CALL_ID: &str = "close-context-worker";
const SPAWN_PROMPT: &str = "spawn a context worker";
const CHILD_PROMPT: &str = "inspect context changes";
const CLOSE_PROMPT: &str = "close the context worker";

fn body_contains(request: &wiremock::Request, text: &str) -> bool {
    decoded_body(request)
        .and_then(|body| String::from_utf8(body).ok())
        .is_some_and(|body| body.contains(text))
}

fn decoded_body(request: &wiremock::Request) -> Option<Vec<u8>> {
    let is_zstd = request
        .headers
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|entry| entry.trim().eq_ignore_ascii_case("zstd"))
        });
    if is_zstd {
        zstd::stream::decode_all(std::io::Cursor::new(&request.body)).ok()
    } else {
        Some(request.body.clone())
    }
}

fn collaboration_mode(model: &str, instructions: Option<&str>) -> CollaborationMode {
    CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: model.to_string(),
            reasoning_effort: None,
            developer_instructions: instructions.map(str::to_string),
        },
    }
}

fn collaboration_xml(instructions: &str) -> String {
    format!("{COLLABORATION_MODE_OPEN_TAG}{instructions}{COLLABORATION_MODE_CLOSE_TAG}")
}

fn count_exact(texts: &[String], target: &str) -> usize {
    texts.iter().filter(|text| text.as_str() == target).count()
}

async fn submit_plain_turn(test: &TestCodex, prompt: &str) -> Result<()> {
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: prompt.to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subagent_changes_emit_environment_updates() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let spawn_arguments = serde_json::to_string(&json!({
        "message": CHILD_PROMPT,
        "task_name": "context_worker",
    }))?;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| body_contains(request, SPAWN_PROMPT),
        sse(vec![
            ev_response_created("spawn-response"),
            ev_function_call_with_namespace(
                SPAWN_CALL_ID,
                MULTI_AGENT_NAMESPACE,
                "spawn_agent",
                &spawn_arguments,
            ),
            ev_completed("spawn-response"),
        ]),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, CHILD_PROMPT) && !body_contains(request, SPAWN_CALL_ID)
        },
        sse(vec![
            ev_response_created("child-response"),
            ev_assistant_message("child-message", "child done"),
            ev_completed("child-response"),
        ]),
    )
    .await;
    let spawn_followup = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| body_contains(request, SPAWN_CALL_ID),
        sse(vec![
            ev_response_created("spawn-followup-response"),
            ev_assistant_message("spawn-followup-message", "spawned"),
            ev_completed("spawn-followup-response"),
        ]),
    )
    .await;

    let mut builder = test_codex().with_config(|config| {
        config
            .features
            .enable(Feature::Collab)
            .expect("collaboration feature should be configurable");
        config
            .features
            .enable(Feature::DeferredExecutor)
            .expect("deferred executor feature should be configurable");
    });
    let test = builder.build_with_auto_env(&server).await?;
    test.submit_turn(SPAWN_PROMPT).await?;

    let spawn_request = spawn_followup.single_request();
    let spawn_output = spawn_request
        .function_call_output_text(SPAWN_CALL_ID)
        .context("spawn output should be present")?;
    let spawn_output: Value = serde_json::from_str(&spawn_output)?;
    let agent_id = spawn_output["agent_id"]
        .as_str()
        .context("spawn output should include agent_id")?;
    let spawn_user_texts = spawn_request.message_input_texts("user");
    assert!(
        spawn_user_texts
            .iter()
            .any(|text| text.contains("<subagents>") && text.contains(agent_id)),
        "spawn follow-up should include the current subagent list: {spawn_user_texts:?}"
    );

    let close_arguments = serde_json::to_string(&json!({"target": agent_id}))?;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| body_contains(request, CLOSE_PROMPT),
        sse(vec![
            ev_response_created("close-response"),
            ev_function_call_with_namespace(
                CLOSE_CALL_ID,
                MULTI_AGENT_NAMESPACE,
                "close_agent",
                &close_arguments,
            ),
            ev_completed("close-response"),
        ]),
    )
    .await;
    let close_followup = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| body_contains(request, CLOSE_CALL_ID),
        sse(vec![
            ev_response_created("close-followup-response"),
            ev_assistant_message("close-followup-message", "closed"),
            ev_completed("close-followup-response"),
        ]),
    )
    .await;

    test.submit_turn(CLOSE_PROMPT).await?;

    assert!(close_followup
        .single_request()
        .message_input_texts("user")
        .iter()
        .any(|text| text.starts_with("<environment_context>") && text.contains("<subagents />")));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collaboration_instructions_clear_and_reenable_incrementally() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("active-response"),
                ev_completed("active-response"),
            ]),
            sse(vec![
                ev_response_created("cleared-response"),
                ev_completed("cleared-response"),
            ]),
            sse(vec![
                ev_response_created("reenabled-response"),
                ev_completed("reenabled-response"),
            ]),
        ],
    )
    .await;
    let mut builder = test_codex();
    let test = builder.build_with_auto_env(&server).await?;
    let model = &test.session_configured.model;
    let active = "active collaboration instructions";
    let reenabled = "reenabled collaboration instructions";

    core_test_support::submit_thread_settings(
        &test.codex,
        codex_protocol::protocol::ThreadSettingsOverrides {
            collaboration_mode: Some(collaboration_mode(model, Some(active))),
            ..Default::default()
        },
    )
    .await?;
    submit_plain_turn(&test, "active collaboration turn").await?;

    core_test_support::submit_thread_settings(
        &test.codex,
        codex_protocol::protocol::ThreadSettingsOverrides {
            collaboration_mode: Some(collaboration_mode(model, None)),
            ..Default::default()
        },
    )
    .await?;
    submit_plain_turn(&test, "cleared collaboration turn").await?;

    core_test_support::submit_thread_settings(
        &test.codex,
        codex_protocol::protocol::ThreadSettingsOverrides {
            collaboration_mode: Some(collaboration_mode(model, Some(reenabled))),
            ..Default::default()
        },
    )
    .await?;
    submit_plain_turn(&test, "reenabled collaboration turn").await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 3);
    let active = collaboration_xml(active);
    let cleared = collaboration_xml("");
    let reenabled = collaboration_xml(reenabled);
    let cleared_request = requests[1].message_input_texts("developer");
    assert_eq!(count_exact(&cleared_request, &active), 1);
    assert_eq!(count_exact(&cleared_request, &cleared), 1);
    let reenabled_request = requests[2].message_input_texts("developer");
    assert_eq!(count_exact(&reenabled_request, &active), 1);
    assert_eq!(count_exact(&reenabled_request, &cleared), 1);
    assert_eq!(count_exact(&reenabled_request, &reenabled), 1);

    Ok(())
}
