use std::sync::Arc;

use codex_code_mode_protocol::grpc as proto;
use futures::StreamExt;
use prost::Message;
use pretty_assertions::assert_eq;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::EventSender;
use super::event_stream;

fn notification(text: &str) -> proto::session_event::Event {
    proto::session_event::Event::Notification(proto::Notification {
        notification_id: "notification".to_string(),
        execution_id: "execution".to_string(),
        cell_id: "cell".to_string(),
        call_id: "call".to_string(),
        text: text.to_string(),
    })
}

fn encoded_len(event: proto::session_event::Event) -> usize {
    proto::SessionEvent { event: Some(event) }.encoded_len()
}

#[tokio::test]
async fn byte_permits_are_held_until_the_client_polls_the_event() {
    let event = notification("bounded");
    let bytes = encoded_len(event.clone());
    let session_bytes = Arc::new(Semaphore::new(bytes));
    let host_bytes = Arc::new(Semaphore::new(bytes));
    let closed = CancellationToken::new();
    let (output, receiver) = mpsc::channel(/*buffer*/ 1);
    let sender = EventSender::new(
        output,
        closed,
        Arc::clone(&session_bytes),
        Arc::clone(&host_bytes),
    );
    sender.send_now(event, /*cell_permit*/ None).unwrap();
    while session_bytes.available_permits() != 0 {
        tokio::task::yield_now().await;
    }

    let mut stream = event_stream(receiver);
    assert!(stream.next().await.unwrap().is_ok());
    assert_eq!(session_bytes.available_permits(), bytes);
    assert_eq!(host_bytes.available_permits(), bytes);
    sender.shutdown().await;
}

#[tokio::test]
async fn shared_host_budget_rejects_another_buffered_session() {
    let event = notification("shared");
    let bytes = encoded_len(event.clone());
    let host_bytes = Arc::new(Semaphore::new(bytes));
    let first_closed = CancellationToken::new();
    let (first_output, _first_receiver) = mpsc::channel(/*buffer*/ 1);
    let first = EventSender::new(
        first_output,
        first_closed,
        Arc::new(Semaphore::new(bytes)),
        Arc::clone(&host_bytes),
    );
    first
        .send_now(event.clone(), /*cell_permit*/ None)
        .unwrap();
    while host_bytes.available_permits() != 0 {
        tokio::task::yield_now().await;
    }

    let second_closed = CancellationToken::new();
    let (second_output, _second_receiver) = mpsc::channel(/*buffer*/ 1);
    let second = EventSender::new(
        second_output,
        second_closed.clone(),
        Arc::new(Semaphore::new(bytes)),
        Arc::clone(&host_bytes),
    );
    assert!(second.send_now(event, /*cell_permit*/ None).is_err());
    assert!(second_closed.is_cancelled());

    first.shutdown().await;
    second.shutdown().await;
}
