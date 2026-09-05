use super::*;
use pretty_assertions::assert_eq;

pub(super) async fn verify_message_delivery(
    test: &TestCodex,
    server: &MockServer,
    child_thread_id: ThreadId,
) -> Result<()> {
    let child = test.thread_manager.get_thread(child_thread_id).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(child.agent_status().await, AgentStatus::Completed(_)) {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    server.reset().await;

    let calls = [
        ("send_message", "plaintext-message-call", "queued message"),
        ("followup_task", "plaintext-followup-call", "next task"),
    ];
    let events = calls.iter().map(|(tool, call_id, message)| {
        let mut event = ev_function_call_with_namespace(
            call_id,
            MULTI_AGENT_V2_NAMESPACE,
            tool,
            &json!({"target": "worker", "message": message}).to_string(),
        );
        event["item"]["encrypted_function_args"] = json!([]);
        event
    });
    mount_sse_once_match(
        server,
        |req: &wiremock::Request| {
            body_contains(req, TURN_2_NO_WAIT_PROMPT)
                && !body_contains(req, "plaintext-followup-call")
        },
        sse(std::iter::once(ev_response_created("resp-messages"))
            .chain(events)
            .chain(std::iter::once(ev_completed("resp-messages")))
            .collect()),
    )
    .await;
    let child_requests = mount_sse_once_match(
        server,
        targets_child,
        sse(vec![
            ev_response_created("resp-child-followup"),
            ev_completed("resp-child-followup"),
        ]),
    )
    .await;
    mount_sse_once_match(
        server,
        |req: &wiremock::Request| {
            body_contains(req, "plaintext-followup-call") && !targets_child(req)
        },
        sse(vec![
            ev_response_created("resp-parent-done"),
            ev_assistant_message("msg-parent-done", "done"),
            ev_completed("resp-parent-done"),
        ]),
    )
    .await;
    test.submit_turn(TURN_2_NO_WAIT_PROMPT).await?;
    let request = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(request) = child_requests
                .requests()
                .into_iter()
                .find(|request| request.inputs_of_type("agent_message").len() == 3)
            {
                break request;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    let messages: Vec<_> = request
        .inputs_of_type("agent_message")
        .into_iter()
        .skip(1)
        .collect();
    let expected: Vec<_> = [("MESSAGE", "queued message"), ("NEW_TASK", "next task")].into_iter().map(|(kind, message)| json!({
        "type": "agent_message",
        "author": "/root",
        "recipient": "/root/worker",
        "content": [{"type": "input_text", "text": format!("Message Type: {kind}\nTask name: /root/worker\nSender: /root\nPayload:\n{message}")}],
    })).collect();
    assert_eq!(
        strip_response_item_ids_from_json(strip_metadata_from_json(json!(messages))),
        json!(expected)
    );
    Ok(())
}

fn targets_child(req: &wiremock::Request) -> bool {
    decoded_body(req)
        .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
        .is_some_and(|body| {
            body["input"].as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item["type"] == "agent_message" && item["recipient"] == "/root/worker"
                })
            })
        })
}
