use super::*;
use codex_app_server_protocol::JSONRPCErrorError;
use pretty_assertions::assert_eq;
use std::cell::Cell;
use std::future::ready;

fn busy(thread_id: ThreadId) -> TypedRequestError {
    TypedRequestError::Server {
        method: "thread/resume".into(),
        source: JSONRPCErrorError {
            code: -32600,
            message: format!("thread {thread_id} already has an active writer"),
            data: None,
        },
    }
}

#[tokio::test]
async fn resume_waits_for_the_real_writer_to_release_the_thread() {
    let home = tempfile::tempdir().unwrap();
    let thread = ThreadId::new();
    let locks = Arc::new(WriterLockCoordinator::new(home.path()));
    let owner = locks.acquire(thread).unwrap();
    let handoff = WriterHandoff::default();
    let mut waiting = handoff.subscribe();
    let attempts = Cell::new(0);
    let resume = handoff.resume(home.path(), thread, || {
        attempts.set(attempts.get() + 1);
        ready(locks.acquire(thread).map_err(|_| busy(thread)))
    });
    tokio::pin!(resume);
    tokio::select! {
        _ = &mut resume => panic!("another process still owns the chat"),
        result = waiting.changed() => result.unwrap(),
    }
    tokio::select! {
        _ = &mut resume => panic!("resume must wait for release"),
        _ = tokio::time::sleep(POLL_INTERVAL + Duration::from_millis(50)) => {},
    }
    assert_eq!(attempts.get(), 1);
    drop(owner);
    let _next_owner = resume.await.unwrap();
    assert!(waiting.borrow().is_none());
    assert_eq!(attempts.get(), 2);
}

#[tokio::test(start_paused = true)]
async fn cancellation_and_timeout_leave_the_external_writer_alone() {
    let home = tempfile::tempdir().unwrap();
    let thread = ThreadId::new();
    let locks = Arc::new(WriterLockCoordinator::new(home.path()));
    let _owner = locks.acquire(thread).unwrap();
    let handoff = WriterHandoff::default();
    let mut waiting = handoff.subscribe();
    for cancel in [true, false] {
        let resume = handoff.resume(home.path(), thread, || ready(Err::<(), _>(busy(thread))));
        tokio::pin!(resume);
        tokio::select! {
            _ = &mut resume => panic!("resume failed before waiting"),
            result = waiting.changed() => result.unwrap(),
        }
        if cancel {
            waiting.borrow_and_update().as_ref().unwrap().cancel();
            assert!(resume.await.unwrap_err().is::<Canceled>());
        } else {
            tokio::time::advance(MAX_WAIT).await;
            assert!(resume.await.unwrap_err().to_string().contains("Timed out"));
        }
        assert!(waiting.borrow_and_update().is_none());
        assert_eq!(locks.active_writers(&[thread]).unwrap(), vec![thread]);
    }
    let wrong_thread = handoff.resume(home.path(), ThreadId::new(), || {
        ready(Err::<(), _>(busy(thread)))
    });
    assert!(wrong_thread.await.is_err());
    assert!(!waiting.has_changed().unwrap());
}
