use std::time::Duration;

use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::test_codex;
use tokio::sync::oneshot;
use tokio::time::timeout;

fn user_input(text: &str) -> Op {
    Op::UserInput {
        items: vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }],
        final_output_json_schema: None,
        responsesapi_client_metadata: None,
        additional_context: Default::default(),
        thread_settings: Default::default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turn_start_persists_developer_and_user_input_before_model_request() {
    let (release_response, response_gate) = oneshot::channel();
    let (server, _completions) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
        gate: Some(response_gate),
        body: responses::sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    }]])
    .await;
    let test = test_codex()
        .with_model("gpt-5.4")
        .with_history_mode(ThreadHistoryMode::Paginated)
        .build_with_streaming_server(&server)
        .await
        .expect("build default thread-store session");
    test.codex
        .inject_response_items(vec![ResponseItem::Message {
            id: None,
            role: "developer".to_string(),
            content: vec![ContentItem::InputText {
                text: "turn-start developer instructions".to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }])
        .await
        .expect("inject developer instructions");
    test.codex
        .submit(user_input("turn-start user input"))
        .await
        .expect("submit user input");

    timeout(
        Duration::from_secs(5),
        server.wait_for_request_count(/*count*/ 1),
    )
    .await
    .expect("turn should reach the model request");
    let rollout_path = test.codex.rollout_path().expect("local rollout path");
    let rollout = tokio::fs::read_to_string(rollout_path)
        .await
        .expect("read rollout while the model response is blocked");
    assert!(rollout.contains("turn-start developer instructions"));
    assert!(rollout.contains("turn-start user input"));

    release_response.send(()).expect("release model response");
    loop {
        let event = timeout(Duration::from_secs(5), test.codex.next_event())
            .await
            .expect("turn should finish")
            .expect("event stream should remain available");
        if matches!(event.msg, EventMsg::TurnComplete(_)) {
            break;
        }
    }
    server.shutdown().await;
}
