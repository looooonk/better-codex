use super::*;
use crate::runtime::test_support::unique_temp_dir;
use pretty_assertions::assert_eq;

async fn runtime() -> anyhow::Result<Arc<StateRuntime>> {
    StateRuntime::init(unique_temp_dir(), "test-provider".to_string()).await
}

#[tokio::test]
async fn queue_crud_pagination_and_ordering_are_durable() -> anyhow::Result<()> {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string()).await?;
    let thread_id = ThreadId::new();
    let first = runtime
        .enqueue_queued_submission(thread_id, r#"{"text":"first"}"#, "client-1")
        .await?;
    let second = runtime
        .enqueue_queued_submission(thread_id, r#"{"text":"second"}"#, "client-2")
        .await?;
    let third = runtime
        .enqueue_queued_submission(thread_id, r#"{"text":"third"}"#, "client-3")
        .await?;

    assert_eq!(
        runtime
            .list_queued_submissions(thread_id, /*offset*/ 1, /*limit*/ 1)
            .await?,
        vec![second.clone()]
    );
    let updated = runtime
        .update_queued_submission(thread_id, &second.id, r#"{"text":"updated"}"#)
        .await?
        .expect("pending item should update");
    assert_eq!(updated.client_user_message_id, "client-2");
    runtime
        .reorder_queued_submissions(
            thread_id,
            &[third.id.clone(), first.id.clone(), second.id.clone()],
        )
        .await?;
    assert!(
        runtime
            .delete_queued_submission(thread_id, &first.id)
            .await?
    );
    assert_eq!(
        runtime
            .list_queued_submissions(thread_id, /*offset*/ 0, /*limit*/ 10)
            .await?
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![third.id, second.id]
    );

    runtime.close().await;
    let reopened = StateRuntime::init(codex_home, "test-provider".to_string()).await?;
    assert_eq!(
        reopened
            .list_queued_submissions(thread_id, /*offset*/ 0, /*limit*/ 10)
            .await?
            .len(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn queue_enforces_item_and_total_input_limits() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let item_limit_thread = ThreadId::new();
    for index in 0..MAX_QUEUED_SUBMISSIONS {
        runtime
            .enqueue_queued_submission(
                item_limit_thread,
                &format!(r#"{{"index":{index}}}"#),
                &format!("client-{index}"),
            )
            .await?;
    }
    assert!(matches!(
        runtime
            .enqueue_queued_submission(item_limit_thread, "{}", "overflow")
            .await,
        Err(ThreadQueueError::QueueFull)
    ));

    let byte_limit_thread = ThreadId::new();
    runtime
        .enqueue_queued_submission(
            byte_limit_thread,
            &"x".repeat(MAX_QUEUED_INPUT_BYTES),
            "full",
        )
        .await?;
    assert!(matches!(
        runtime
            .enqueue_queued_submission(byte_limit_thread, "x", "overflow")
            .await,
        Err(ThreadQueueError::InputBytesExceeded)
    ));
    Ok(())
}
