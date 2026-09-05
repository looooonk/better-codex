use super::*;
use pretty_assertions::assert_eq;

#[test]
fn plaintext_message_preserves_its_envelope_and_bounds_large_payloads() {
    let recipient = AgentPath::root().join("worker").unwrap();
    let small = InterAgentMessage::new(
        InterAgentMessageType::Message,
        recipient.clone(),
        AgentPath::root(),
        "hello",
    )
    .render();
    assert_eq!(
        small,
        "Message Type: MESSAGE\nTask name: /root/worker\nSender: /root\nPayload:\nhello"
    );
    let large = InterAgentMessage::new(
        InterAgentMessageType::NewTask,
        recipient,
        AgentPath::root(),
        format!("start {} finish", "payload ".repeat(5_000)),
    )
    .render();
    assert!(large.len() < 9_000);
    assert!(large.starts_with(
        "Message Type: NEW_TASK\nTask name: /root/worker\nSender: /root\nPayload:\nstart"
    ));
    assert!(large.ends_with("finish"));
}
