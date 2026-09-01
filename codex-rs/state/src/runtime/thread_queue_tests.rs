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

#[tokio::test]
async fn queue_claims_are_idempotent_and_cas_guarded() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let thread_id = ThreadId::new();
    let first = runtime
        .enqueue_queued_submission(thread_id, "first", "client-1")
        .await?;
    let second = runtime
        .enqueue_queued_submission(thread_id, "second", "client-2")
        .await?;

    let claimed = runtime
        .claim_queued_submission(thread_id, Some(&first.id), "turn-1")
        .await?;
    assert!(matches!(claimed, QueueClaimResult::Claimed(ref item) if item.id == first.id));
    assert!(matches!(
        runtime
            .claim_queued_submission(thread_id, Some(&first.id), "turn-other")
            .await?,
        QueueClaimResult::Existing(ref item) if item.turn_id.as_deref() == Some("turn-1")
    ));
    assert!(matches!(
        runtime
            .claim_queued_submission(thread_id, Some(&second.id), "turn-2")
            .await?,
        QueueClaimResult::Busy(ref item) if item.id == first.id
    ));
    assert!(
        !runtime
            .release_queued_submission_claim(thread_id, &first.id, "wrong-turn")
            .await?
    );
    assert!(
        runtime
            .mark_queued_submission_inflight(thread_id, "turn-1")
            .await?
    );
    assert!(
        runtime
            .finish_queued_submission(
                thread_id,
                "turn-1",
                QueuedSubmissionTerminalStatus::Completed,
                QueueTerminalDisposition::Continue,
            )
            .await?
    );
    assert!(matches!(
        runtime
            .claim_queued_submission(thread_id, Some(&first.id), "turn-other")
            .await?,
        QueueClaimResult::Existing(ref item)
            if item.state == QueuedSubmissionState::Terminal
                && item.turn_id.as_deref() == Some("turn-1")
    ));
    Ok(())
}

#[tokio::test]
async fn deleting_thread_removes_its_queue() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let thread_id = ThreadId::new();
    let codex_home = runtime.codex_home().to_path_buf();
    runtime
        .upsert_thread(&crate::runtime::test_support::test_thread_metadata(
            codex_home.as_path(),
            thread_id,
            codex_home.clone(),
        ))
        .await?;
    runtime
        .enqueue_queued_submission(thread_id, "queued", "client")
        .await?;

    assert_eq!(runtime.delete_thread(thread_id).await?, 1);
    assert!(
        runtime
            .list_queued_submissions(thread_id, /*offset*/ 0, /*limit*/ 10)
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn invalid_reorder_keeps_existing_order() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let thread_id = ThreadId::new();
    let first = runtime
        .enqueue_queued_submission(thread_id, "first", "client-1")
        .await?;
    let second = runtime
        .enqueue_queued_submission(thread_id, "second", "client-2")
        .await?;
    assert!(matches!(
        runtime
            .reorder_queued_submissions(thread_id, std::slice::from_ref(&first.id))
            .await,
        Err(ThreadQueueError::InvalidReorder)
    ));
    assert_eq!(
        runtime
            .list_queued_submissions(thread_id, /*offset*/ 0, /*limit*/ 10)
            .await?
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![first.id, second.id]
    );
    Ok(())
}

#[tokio::test]
async fn reorder_fails_closed_when_temporary_order_overflows() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let thread_id = ThreadId::new();
    let first = runtime
        .enqueue_queued_submission(thread_id, "first", "client-1")
        .await?;
    let second = runtime
        .enqueue_queued_submission(thread_id, "second", "client-2")
        .await?;
    sqlx::query(
        r#"
UPDATE thread_queue_items
SET queue_order = CASE id WHEN ? THEN ? WHEN ? THEN ? END
WHERE thread_id = ? AND id IN (?, ?)
        "#,
    )
    .bind(&first.id)
    .bind(i64::MAX - 2)
    .bind(&second.id)
    .bind(i64::MAX - 1)
    .bind(thread_id.to_string())
    .bind(&first.id)
    .bind(&second.id)
    .execute(runtime.pool.as_ref())
    .await?;

    assert!(matches!(
        runtime
            .reorder_queued_submissions(thread_id, &[first.id.clone(), second.id.clone()])
            .await,
        Err(ThreadQueueError::Storage(_))
    ));
    assert_eq!(
        runtime
            .list_queued_submissions(thread_id, /*offset*/ 0, /*limit*/ 10)
            .await?
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![first.id, second.id]
    );
    Ok(())
}

#[tokio::test]
async fn reorder_uses_collision_free_slots_and_release_restores_position() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let thread_id = ThreadId::new();
    let first = runtime
        .enqueue_queued_submission(thread_id, "first", "client-1")
        .await?;
    let second = runtime
        .enqueue_queued_submission(thread_id, "second", "client-2")
        .await?;
    let third = runtime
        .enqueue_queued_submission(thread_id, "third", "client-3")
        .await?;
    assert!(runtime
        .delete_queued_submission(thread_id, &first.id)
        .await?);

    runtime
        .reorder_queued_submissions(thread_id, &[third.id.clone(), second.id.clone()])
        .await?;
    assert!(matches!(
        runtime
            .claim_queued_submission(thread_id, Some(&third.id), "turn-3")
            .await?,
        QueueClaimResult::Claimed(_)
    ));
    assert!(runtime
        .release_queued_submission_claim(thread_id, &third.id, "turn-3")
        .await?);
    assert_eq!(
        runtime
            .list_queued_submissions(thread_id, /*offset*/ 0, /*limit*/ 10)
            .await?
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![third.id, second.id]
    );
    Ok(())
}

#[tokio::test]
async fn enqueue_retry_reuses_stable_submission_id() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let thread_id = ThreadId::new();
    let first = runtime
        .enqueue_queued_submission(thread_id, "first", "client-1")
        .await?;
    let retry = runtime
        .enqueue_queued_submission(thread_id, "first", "client-1")
        .await?;

    assert_eq!(retry, first);
    assert_eq!(
        runtime
            .list_queued_submissions(thread_id, /*offset*/ 0, /*limit*/ 10)
            .await?,
        vec![first]
    );
    assert!(matches!(
        runtime
            .enqueue_queued_submission(thread_id, "different", "client-1")
            .await,
        Err(ThreadQueueError::ClientMessageConflict)
    ));
    Ok(())
}

#[tokio::test]
async fn active_items_remain_within_queue_limits_and_terminal_payloads_are_scrubbed()
-> anyhow::Result<()> {
    let runtime = runtime().await?;
    let thread_id = ThreadId::new();
    let item = runtime
        .enqueue_queued_submission(
            thread_id,
            &"x".repeat(MAX_QUEUED_INPUT_BYTES),
            "client-1",
        )
        .await?;
    assert!(matches!(
        runtime
            .claim_queued_submission(thread_id, Some(&item.id), "turn-1")
            .await?,
        QueueClaimResult::Claimed(_)
    ));
    assert!(matches!(
        runtime
            .enqueue_queued_submission(thread_id, "x", "client-2")
            .await,
        Err(ThreadQueueError::InputBytesExceeded)
    ));
    assert!(runtime
        .finish_queued_submission(
            thread_id,
            "turn-1",
            QueuedSubmissionTerminalStatus::Completed,
            QueueTerminalDisposition::Continue,
        )
        .await?);
    assert_eq!(
        runtime
            .queued_submission(thread_id, &item.id)
            .await?
            .expect("terminal tombstone should remain")
            .payload,
        "[]"
    );
    assert!(runtime
        .enqueue_queued_submission(thread_id, "x", "client-2")
        .await
        .is_ok());
    Ok(())
}

#[tokio::test]
async fn hook_rejection_and_pause_state_survive_reopen() -> anyhow::Result<()> {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string()).await?;
    let thread_id = ThreadId::new();
    let item = runtime
        .enqueue_queued_submission(thread_id, "queued", "client-1")
        .await?;
    assert!(matches!(
        runtime
            .claim_queued_submission(thread_id, Some(&item.id), "turn-1")
            .await?,
        QueueClaimResult::Claimed(_)
    ));
    assert!(runtime
        .mark_queued_submission_admission_rejected(
            thread_id,
            "turn-1",
            QueuedSubmissionAdmissionRejection::Hook,
        )
        .await?);
    assert!(runtime
        .finish_queued_submission(
            thread_id,
            "turn-1",
            QueuedSubmissionTerminalStatus::Failed,
            QueueTerminalDisposition::Pause(ThreadQueuePauseReason::Interrupted),
        )
        .await?);

    runtime.close().await;
    let reopened = StateRuntime::init(codex_home.clone(), "test-provider".to_string()).await?;
    let stored = reopened
        .queued_submission(thread_id, &item.id)
        .await?
        .expect("terminal tombstone should remain");
    assert_eq!(
        stored.admission_rejection,
        Some(QueuedSubmissionAdmissionRejection::Hook)
    );
    assert_eq!(
        reopened.thread_queue_pause_reason(thread_id).await?,
        Some(ThreadQueuePauseReason::Interrupted)
    );
    reopened.close().await;

    let reopened_again = StateRuntime::init(codex_home, "test-provider".to_string()).await?;
    assert_eq!(
        reopened_again.thread_queue_pause_reason(thread_id).await?,
        Some(ThreadQueuePauseReason::Interrupted)
    );
    assert!(reopened_again.resume_thread_queue(thread_id).await?);
    assert_eq!(
        reopened_again.thread_queue_pause_reason(thread_id).await?,
        None
    );
    Ok(())
}

#[tokio::test]
async fn queue_rejects_unbounded_identifiers() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let thread_id = ThreadId::new();
    assert!(matches!(
        runtime
            .enqueue_queued_submission(
                thread_id,
                "[]",
                &"x".repeat(MAX_QUEUE_IDENTIFIER_BYTES + 1),
            )
            .await,
        Err(ThreadQueueError::InvalidIdentifier)
    ));
    assert!(matches!(
        runtime
            .queued_submission(thread_id, &"x".repeat(MAX_QUEUE_IDENTIFIER_BYTES + 1))
            .await,
        Err(ThreadQueueError::InvalidIdentifier)
    ));
    Ok(())
}
