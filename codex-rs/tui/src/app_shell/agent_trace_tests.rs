use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::io::Write;

fn lane(id: &str, name: &str, records: Records) -> Lane {
    Lane {
        target: AgentLogTarget {
            thread_id: id.into(),
            display_name: name.into(),
            path: if name == "Main" {
                "/root".into()
            } else {
                format!("/root/{name}")
            },
            task_summary: None,
            status: AgentLifecycleStatus::Completed,
        },
        spans: records.spans.clone(),
        records: Arc::new(records),
        file: None,
        created: 0,
    }
}

fn record(second: u8, kind: &str, payload: Value) -> Value {
    json!({"timestamp":format!("2026-01-10T12:00:{second:02}Z"),"type":kind,"payload":payload})
}

fn receipt(at: u8, id: &str, author: &str, recipient: &str, content: Value) -> Value {
    record(
        at,
        "response_item",
        json!({"type":"agent_message","id":id,"author":author,"recipient":recipient,"content":content}),
    )
}

async fn parse(id: &str, path: &str, records: Vec<Value>) -> Records {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for record in records {
        writeln!(file, "{record}").unwrap();
    }
    Records::read(file.path(), path, id).await.unwrap()
}

#[tokio::test]
async fn delayed_encrypted_delivery_keeps_send_order_and_restart_gaps() {
    let root = "/root";
    let scout = "/root/scout";
    let review = "/root/review";
    let cipher = "gAAAAA-secret";
    let outgoing = json!({"type":"function_call","namespace":"collaboration","name":"send_message","call_id":"send1","arguments":json!({"target":review,"message":cipher}).to_string()});
    let proof = json!({"type":"item_completed","thread_id":"s","item":{"id":"send1","type":"SubAgentActivity","kind":"interacted","agent_thread_id":"r"}});
    let encrypted = json!([{"type":"input_text","text":"Message Type: MESSAGE\nTask name: /root/review\nSender: /root/scout\nPayload:\n"},{"type":"encrypted_content","encrypted_content":cipher}]);
    let ack = receipt(
        5,
        "reply",
        review,
        root,
        json!([{"type":"input_text","text":"Message Type: MESSAGE\nTask name: /root\nSender: /root/review\nPayload:\nReady."}]),
    );
    let task = receipt(
        7,
        "new-task",
        root,
        scout,
        json!([{"type":"input_text","text":"Message Type: NEW_TASK\nTask name: /root/scout\nSender: /root\nPayload:\nCheck the second case."}]),
    );
    let main = parse(
        "m",
        root,
        vec![
            record(0, "event_msg", json!({"type":"task_started"})),
            ack.clone(),
            ack,
        ],
    )
    .await;
    let worker = parse(
        "s",
        scout,
        vec![
            record(0, "event_msg", json!({"type":"task_started"})),
            record(1, "response_item", outgoing),
            record(2, "event_msg", proof),
            record(3, "event_msg", json!({"type":"task_complete"})),
            record(6, "event_msg", json!({"type":"task_started"})),
            task,
            record(8, "event_msg", json!({"type":"turn_aborted"})),
        ],
    )
    .await;
    let reviewer = parse(
        "r",
        review,
        vec![
            record(1, "event_msg", json!({"type":"task_started"})),
            receipt(9, "delivery", scout, review, encrypted),
        ],
    )
    .await;

    let graph = Trace::assemble(
        vec![
            lane("m", "Main", main),
            lane("s", "scout", worker),
            lane("r", "review", reviewer),
        ],
        "m",
        false,
    );
    let routes = graph
        .messages
        .iter()
        .map(|m| {
            (
                graph.lanes[m.from].target.display_name.as_str(),
                graph.lanes[m.to].target.display_name.as_str(),
                m.body.text(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        routes,
        vec![
            ("scout", "review", "Encrypted message"),
            ("review", "Main", "Ready."),
            ("Main", "scout", "Check the second case.")
        ]
    );
    assert_eq!(graph.messages[0].call_id.as_deref(), Some("send1"));
    assert!(graph.messages[0].received_at.unwrap() > graph.messages[2].at);
    let scout = graph
        .lanes
        .iter()
        .find(|lane| lane.target.path == "/root/scout")
        .unwrap();
    assert_eq!(
        graph
            .messages
            .iter()
            .map(|m| scout.span_at(m.at))
            .collect::<Vec<_>>(),
        vec![Some(0), None, Some(1)]
    );
}
