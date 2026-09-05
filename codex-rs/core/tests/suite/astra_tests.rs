use anyhow::Result;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn astra_effort_switching_preserves_model_and_fast_tier() -> Result<()> {
    let server = responses::start_mock_server().await;
    let efforts = [
        (ReasoningEffort::Low, "low"),
        (ReasoningEffort::Medium, "medium"),
        (ReasoningEffort::High, "high"),
        (ReasoningEffort::XHigh, "xhigh"),
        (ReasoningEffort::Max, "max"),
        (ReasoningEffort::Ultra, "xhigh"),
    ];
    let mocks = responses::mount_sse_sequence(
        &server,
        efforts
            .iter()
            .enumerate()
            .map(|(i, _)| responses::sse(vec![responses::ev_completed(&format!("resp-{i}"))]))
            .collect(),
    )
    .await;
    let test = test_codex()
        .with_model("gpt-6-astra")
        .with_config(|config| {
            config.service_tier = Some("priority".to_string());
        })
        .build_with_auto_env(&server)
        .await?;

    for (effort, _) in &efforts {
        test.codex
            .submit(Op::UserInput {
                items: vec![UserInput::Text {
                    text: "hello".to_string(),
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: None,
                responsesapi_client_metadata: None,
                additional_context: Default::default(),
                thread_settings: ThreadSettingsOverrides {
                    effort: Some(Some(effort.clone())),
                    ..Default::default()
                },
            })
            .await?;
        wait_for_event(&test.codex, |event| {
            matches!(event, EventMsg::TurnComplete(_))
        })
        .await;
    }

    assert_eq!(mocks.requests().len(), efforts.len());
    for (request, (effort, wire_effort)) in mocks.requests().into_iter().zip(efforts) {
        let request = request.body_json();
        assert_eq!(
            (
                &request["model"],
                &request["reasoning"]["effort"],
                &request["service_tier"]
            ),
            (
                &serde_json::json!("gpt-6-astra"),
                &serde_json::json!(wire_effort),
                &serde_json::json!("priority")
            )
        );
        let metadata: serde_json::Value = serde_json::from_str(
            request["client_metadata"]["x-codex-turn-metadata"]
                .as_str()
                .unwrap(),
        )?;
        assert_eq!(
            (
                metadata["node_repl_auto_review_required"].clone(),
                metadata["node_repl_disabled"].clone()
            ),
            (serde_json::json!(true), serde_json::json!(false))
        );
        let developer_messages = request["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["role"] == "developer")
            .map(ToString::to_string)
            .collect::<String>();
        assert_eq!(
            developer_messages.contains("Proactive multi-agent delegation is active."),
            effort == ReasoningEffort::Ultra
        );
    }
    Ok(())
}
