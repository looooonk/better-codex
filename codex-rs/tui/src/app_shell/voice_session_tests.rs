use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn stopping_cancels_capture_before_server_cleanup_finishes() {
    let (abort, registration) = AbortHandle::new_pair();
    let (native_abort, native_registration) = AbortHandle::new_pair();
    let (answers, _answer_rx) = mpsc::channel(/*buffer*/ 2);
    let (_events_tx, events) = mpsc::channel(/*buffer*/ 2);
    let (finished, finish) = tokio::sync::oneshot::channel();
    let (stopped, mut stop_confirmation) = mpsc::channel(/*buffer*/ 1);
    let mut session = VoiceSession {
        thread_id: "thread".to_string(),
        transport: None,
        answers,
        events,
        worker: tokio::spawn(async move {
            finish.await?;
            Ok(())
        }),
        abort,
        native_abort,
        stopped,
    };
    session.stop();
    assert_eq!(
        Abortable::new(std::future::pending::<()>(), registration).await,
        Err(futures::future::Aborted)
    );
    assert_eq!(
        Abortable::new(std::future::pending::<()>(), native_registration).await,
        Err(futures::future::Aborted)
    );
    assert!(!session.worker.is_finished());
    assert!(stop_confirmation.try_recv().is_err());
    session.confirm_stop();
    assert_eq!(stop_confirmation.recv().await, Some(()));
    finished.send(()).unwrap();
    (&mut session.worker).await.unwrap().unwrap();
}
