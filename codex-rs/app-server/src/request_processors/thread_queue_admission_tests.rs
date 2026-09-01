use std::future::pending;
use std::time::Duration;

use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::EventMsg;
use pretty_assertions::assert_eq;
use tokio::sync::oneshot;
use tokio::time::advance;

use super::*;

#[tokio::test]
async fn error_ends_admission_wait_before_timeout() {
    let (sender, receiver) = oneshot::channel();
    sender
        .send(
            admission_result_for_event(
                "client-1",
                &EventMsg::Error(ErrorEvent {
                    message: "rejected".to_string(),
                    codex_error_info: None,
                }),
            )
            .expect("error should reject admission"),
        )
        .expect("admission receiver should remain open");

    assert_eq!(
        wait_for_queue_admission(receiver, pending(), Duration::from_secs(10)).await,
        QueueAdmissionWaitResult::Admission(QueueAdmissionResult::RejectedByError)
    );
}

#[tokio::test(start_paused = true)]
async fn admission_wait_has_a_finite_timeout() {
    let (_sender, receiver) = oneshot::channel();
    let wait = tokio::spawn(wait_for_queue_admission(
        receiver,
        pending(),
        Duration::from_secs(10),
    ));
    tokio::task::yield_now().await;

    advance(Duration::from_secs(10)).await;

    assert_eq!(
        wait.await.expect("admission wait task should complete"),
        QueueAdmissionWaitResult::TimedOut
    );
}
