use anyhow::Result;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn astra_async_questions_return_before_the_user_answers() -> Result<()> {
    let server = responses::start_mock_server().await;
    let first = responses::mount_sse_once(&server, responses::sse(vec![
        responses::ev_function_call("question-1", "request_user_input_async", r#"{"questions":[{"title":"Which approach?","options":["Small change","Full rewrite"]}]}"#),
        responses::ev_function_call_with_namespace("time-1", "clock", "curr_time", "{}"),
        responses::ev_completed("resp-1"),
    ])).await;
    let continued = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_assistant_message("msg-2", "I can continue independent work."),
            responses::ev_completed("resp-2"),
        ]),
    )
    .await;
    let test = test_codex()
        .with_model("gpt-6-astra")
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "Ask me a question and continue.".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    let mut questions = Vec::new();
    loop {
        let event = test.codex.next_event().await?;
        match event.msg {
            EventMsg::ItemCompleted(event) if event.item.id() == "question-1" => {
                questions.push(event.item)
            }
            EventMsg::RequestUserInput(_) => {
                panic!("async questions must not block on a user-input response")
            }
            EventMsg::TurnComplete(_) => break,
            _ => {}
        }
    }
    assert_eq!(
        serde_json::to_value(questions)?,
        serde_json::json!([{
            "type": "AgentMessage", "id": "question-1", "content": [{"type":"Text","text":"Which approach?\n- Small change\n- Full rewrite"}], "phase":"commentary"
        }])
    );
    assert_eq!(
        continued
            .single_request()
            .function_call_output_text("question-1")
            .as_deref(),
        Some(r#"{"accepted":true}"#)
    );
    let request = first.single_request().body_json();
    let tools = request["input"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"] == "additional_tools")
        .unwrap()["tools"]
        .to_string();
    for name in ["request_user_input_async", "curr_time", "sleep"] {
        assert!(tools.contains(name), "Astra should expose {name}");
    }
    assert!(
        continued
            .single_request()
            .function_call_output_text("time-1")
            .unwrap()
            .contains("UTC")
    );
    let answered = responses::mount_sse_once(
        &server,
        responses::sse(vec![responses::ev_completed("resp-3")]),
    )
    .await;
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "Small change, please.".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    core_test_support::wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let request = answered.single_request();
    assert!(request.body_contains_text("Small change, please."));
    assert_eq!(
        request.function_call_output_text("question-1").as_deref(),
        Some(r#"{"accepted":true}"#)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_async_questions_return_errors_without_emitting_partial_messages() -> Result<()> {
    let server = responses::start_mock_server().await;
    let args = [
        serde_json::json!({"questions":[]}),
        serde_json::json!({"questions":[{"title":"Valid"},{"title":" "}]}),
        serde_json::json!({"questions":[{"title":"Choose","options":[]}]}),
        serde_json::json!({"questions":[{"title":"x".repeat(3100)}]}),
    ];
    let mut events = args
        .iter()
        .enumerate()
        .map(|(i, args)| {
            responses::ev_function_call(
                &format!("invalid-{i}"),
                "request_user_input_async",
                &args.to_string(),
            )
        })
        .collect::<Vec<_>>();
    events.push(responses::ev_completed("resp-1"));
    responses::mount_sse_once(&server, responses::sse(events)).await;
    let continued = responses::mount_sse_once(
        &server,
        responses::sse(vec![responses::ev_completed("resp-2")]),
    )
    .await;
    let test = test_codex()
        .with_model("gpt-6-astra")
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "Ask a question".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    loop {
        let event = test.codex.next_event().await?;
        match event.msg {
            EventMsg::ItemCompleted(event) => assert!(!event.item.id().starts_with("invalid-")),
            EventMsg::TurnComplete(_) => break,
            _ => {}
        }
    }
    let request = continued.single_request();
    for i in 0..args.len() {
        let output = request
            .function_call_output_text(&format!("invalid-{i}"))
            .unwrap();
        assert!(
            !output.contains(r#""accepted":true"#),
            "invalid input must return an error"
        );
    }
    Ok(())
}
