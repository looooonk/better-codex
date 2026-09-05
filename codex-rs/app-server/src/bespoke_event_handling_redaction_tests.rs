use super::*;
use crate::CHANNEL_CAPACITY;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::OutgoingEnvelope;
use crate::outgoing_message::OutgoingMessage;
use crate::outgoing_message::OutgoingMessageSender;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use tokio::sync::mpsc;

#[tokio::test]
async fn raw_response_item_diagnostic_redacts_arguments_and_preserves_call_id() -> Result<()> {
    let (tx, mut rx) = mpsc::channel(CHANNEL_CAPACITY);
    let outgoing = Arc::new(OutgoingMessageSender::new(
        tx,
        codex_analytics::AnalyticsEventsClient::disabled(),
    ));
    let outgoing =
        ThreadScopedOutgoingMessageSender::new(outgoing, vec![ConnectionId(1)], ThreadId::new());
    let secret = "example_synthetic_bearer_token_123456";

    maybe_emit_raw_response_item_completed(
        ThreadId::new(),
        "turn-1",
        codex_protocol::models::ResponseItem::FunctionCall {
            id: None,
            name: "exec_command".to_string(),
            namespace: None,
            arguments: format!(r#"{{"authorization":"Bearer {secret}"}}"#),
            call_id: "call-1".to_string(),
            encrypted_function_args: None,
            internal_chat_message_metadata_passthrough: None,
        },
        &outgoing,
    )
    .await;

    let envelope = rx
        .recv()
        .await
        .ok_or_else(|| anyhow!("should send one message"))?;
    let message = match envelope {
        OutgoingEnvelope::Broadcast { message }
        | OutgoingEnvelope::ToConnection { message, .. } => message,
    };
    let OutgoingMessage::AppServerNotification(ServerNotification::RawResponseItemCompleted(
        notification,
    )) = message
    else {
        bail!("unexpected message: {message:?}");
    };
    let codex_protocol::models::ResponseItem::FunctionCall {
        arguments, call_id, ..
    } = notification.item
    else {
        bail!("expected function call diagnostic");
    };
    assert_eq!(call_id, "call-1");
    assert_eq!(arguments, r#"{"authorization":"Bearer [REDACTED_SECRET]"}"#);
    Ok(())
}
