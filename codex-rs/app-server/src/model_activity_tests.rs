use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn tool_writing_is_counted_once_and_the_wait_starts_after_generation() {
    let thread_id = ThreadId::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let listeners = ThreadStateManager::default();
    listeners.register_listener_command_tx(thread_id, tx);
    let observer = ModelActivityObserver(listeners);
    let request = observer
        .request(ModelRequestInput {
            kind: ModelRequestKind::Generation,
            thread_id: &thread_id.to_string(),
            turn_id: Some("turn"),
            client_metadata: &mut None,
            model: "model",
        })
        .unwrap();
    let arguments = r#"{"cmd":"sleep 20"}"#;
    let call = serde_json::from_value(json!({
        "type": "function_call",
        "name": "exec_command",
        "arguments": arguments,
        "call_id": "call",
    }))
    .unwrap();
    let events = vec![
        ResponseEvent::ToolCallInputDelta {
            item_id: "item".into(),
            call_id: None,
            delta: arguments[..6].into(),
        },
        ResponseEvent::ToolCallInputDelta {
            item_id: "item".into(),
            call_id: None,
            delta: arguments[6..].into(),
        },
        ResponseEvent::OutputItemDone(call),
        ResponseEvent::Completed {
            response_id: "response".into(),
            token_usage: None,
            usage_metadata: None,
            end_turn: Some(false),
        },
    ];
    let upstream = futures::stream::iter(events.into_iter().map(Ok)).boxed();
    let mut downstream = request.intercept(upstream);
    let mut activities = Vec::new();
    while downstream.next().await.is_some() {}
    while let Ok(ThreadListenerCommand::ModelActivity(notification)) = rx.try_recv() {
        assert_eq!(notification.turn_id, "turn");
        activities.push(notification.activity);
    }
    assert_eq!(activities.first(), Some(&TurnModelActivity::Generating));
    assert_eq!(activities.last(), Some(&TurnModelActivity::WaitingForTool));
    let total: f64 = activities[1..activities.len() - 1]
        .iter()
        .map(|activity| match activity {
            TurnModelActivity::ToolInput { estimated_tokens } => *estimated_tokens,
            unexpected => panic!("unexpected activity {unexpected:?}"),
        })
        .sum();
    assert!((total - estimate_tokens(arguments, &mut '\0')).abs() < 1e-8);
}
