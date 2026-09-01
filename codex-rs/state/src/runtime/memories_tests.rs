use super::JOB_KIND_MEMORY_CONSOLIDATE_GLOBAL;
use super::JOB_KIND_MEMORY_STAGE1;
use super::MEMORY_CONSOLIDATION_JOB_KEY;
use super::PHASE2_SUCCESS_COOLDOWN_SECONDS;
use super::StateRuntime;
use super::test_support::test_thread_metadata;
use super::test_support::unique_temp_dir;
use crate::model::Phase2JobClaimOutcome;
use crate::model::Stage1JobClaim;
use crate::model::Stage1JobClaimOutcome;
use crate::model::Stage1Output;
use crate::model::Stage1StartupClaimParams;
use chrono::Duration;
use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::protocol::ThreadHistoryMode;
use pretty_assertions::assert_eq;
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

impl StateRuntime {
    async fn clear_memory_data(&self) -> anyhow::Result<()> {
        self.memories.clear_memory_data().await
    }

    async fn record_stage1_output_usage(&self, thread_ids: &[ThreadId]) -> anyhow::Result<usize> {
        self.memories.record_stage1_output_usage(thread_ids).await
    }

    async fn claim_stage1_jobs_for_startup(
        &self,
        current_thread_id: ThreadId,
        params: Stage1StartupClaimParams<'_>,
    ) -> anyhow::Result<Vec<Stage1JobClaim>> {
        self.memories
            .claim_stage1_jobs_for_startup(current_thread_id, params)
            .await
    }

    async fn list_stage1_outputs_for_global(&self, n: usize) -> anyhow::Result<Vec<Stage1Output>> {
        self.memories.list_stage1_outputs_for_global(n).await
    }

    async fn prune_stage1_outputs_for_retention(
        &self,
        max_unused_days: i64,
        limit: usize,
    ) -> anyhow::Result<usize> {
        self.memories
            .prune_stage1_outputs_for_retention(max_unused_days, limit)
            .await
    }

    async fn get_phase2_input_selection(
        &self,
        n: usize,
        max_unused_days: i64,
    ) -> anyhow::Result<Vec<Stage1Output>> {
        self.memories
            .get_phase2_input_selection(n, max_unused_days)
            .await
    }

    async fn mark_thread_memory_mode_polluted(&self, thread_id: ThreadId) -> anyhow::Result<bool> {
        self.memories
            .mark_thread_memory_mode_polluted(thread_id)
            .await
    }

    async fn try_claim_stage1_job(
        &self,
        thread_id: ThreadId,
        worker_id: ThreadId,
        source_updated_at: i64,
        lease_seconds: i64,
        max_running_jobs: usize,
    ) -> anyhow::Result<Stage1JobClaimOutcome> {
        self.memories
            .try_claim_stage1_job(
                thread_id,
                worker_id,
                source_updated_at,
                lease_seconds,
                max_running_jobs,
            )
            .await
    }

    async fn mark_stage1_job_succeeded(
        &self,
        thread_id: ThreadId,
        ownership_token: &str,
        source_updated_at: i64,
        raw_memory: &str,
        rollout_summary: &str,
        rollout_slug: Option<&str>,
    ) -> anyhow::Result<bool> {
        self.memories
            .mark_stage1_job_succeeded(
                thread_id,
                ownership_token,
                source_updated_at,
                raw_memory,
                rollout_summary,
                rollout_slug,
            )
            .await
    }

    async fn mark_stage1_job_succeeded_no_output(
        &self,
        thread_id: ThreadId,
        ownership_token: &str,
    ) -> anyhow::Result<bool> {
        self.memories
            .mark_stage1_job_succeeded_no_output(thread_id, ownership_token)
            .await
    }

    async fn mark_stage1_job_failed(
        &self,
        thread_id: ThreadId,
        ownership_token: &str,
        failure_reason: &str,
        retry_delay_seconds: i64,
    ) -> anyhow::Result<bool> {
        self.memories
            .mark_stage1_job_failed(
                thread_id,
                ownership_token,
                failure_reason,
                retry_delay_seconds,
            )
            .await
    }

    async fn enqueue_global_consolidation(&self, input_watermark: i64) -> anyhow::Result<()> {
        self.memories
            .enqueue_global_consolidation(input_watermark)
            .await
    }

    async fn try_claim_global_phase2_job(
        &self,
        worker_id: ThreadId,
        lease_seconds: i64,
    ) -> anyhow::Result<Phase2JobClaimOutcome> {
        self.memories
            .try_claim_global_phase2_job(worker_id, lease_seconds)
            .await
    }

    async fn mark_global_phase2_job_succeeded(
        &self,
        ownership_token: &str,
        completed_watermark: i64,
        selected_outputs: &[Stage1Output],
    ) -> anyhow::Result<bool> {
        self.memories
            .mark_global_phase2_job_succeeded(
                ownership_token,
                completed_watermark,
                selected_outputs,
            )
            .await
    }

    async fn mark_global_phase2_job_failed(
        &self,
        ownership_token: &str,
        failure_reason: &str,
        retry_delay_seconds: i64,
    ) -> anyhow::Result<bool> {
        self.memories
            .mark_global_phase2_job_failed(ownership_token, failure_reason, retry_delay_seconds)
            .await
    }

    async fn mark_global_phase2_job_failed_if_unowned(
        &self,
        ownership_token: &str,
        failure_reason: &str,
        retry_delay_seconds: i64,
    ) -> anyhow::Result<bool> {
        self.memories
            .mark_global_phase2_job_failed_if_unowned(
                ownership_token,
                failure_reason,
                retry_delay_seconds,
            )
            .await
    }
}

fn stable_thread_id(value: &str) -> ThreadId {
    ThreadId::from_string(value).expect("thread id")
}

fn memory_pool(runtime: &StateRuntime) -> &sqlx::SqlitePool {
    runtime.memories().pool.as_ref()
}

async fn age_phase2_success_beyond_cooldown(runtime: &StateRuntime) {
    sqlx::query("UPDATE jobs SET finished_at = ? WHERE kind = ? AND job_key = ?")
        .bind(Utc::now().timestamp() - PHASE2_SUCCESS_COOLDOWN_SECONDS - 1)
        .bind(JOB_KIND_MEMORY_CONSOLIDATE_GLOBAL)
        .bind(MEMORY_CONSOLIDATION_JOB_KEY)
        .execute(memory_pool(runtime))
        .await
        .expect("age phase2 success beyond cooldown");
}

#[tokio::test]
async fn stage1_claim_skips_when_up_to_date() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let metadata = test_thread_metadata(&codex_home, thread_id, codex_home.join("a"));
    runtime
        .upsert_thread(&metadata)
        .await
        .expect("upsert thread");

    let owner_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let owner_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");

    let claim = runtime
        .try_claim_stage1_job(
            thread_id, owner_a, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 job");
    let ownership_token = match claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected claim outcome: {other:?}"),
    };

    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id,
                ownership_token.as_str(),
                /*source_updated_at*/ 100,
                "raw",
                "sum",
                /*rollout_slug*/ None,
            )
            .await
            .expect("mark stage1 succeeded"),
        "stage1 success should finalize for current token"
    );

    let up_to_date = runtime
        .try_claim_stage1_job(
            thread_id, owner_b, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 up-to-date");
    assert_eq!(up_to_date, Stage1JobClaimOutcome::SkippedUpToDate);

    let needs_rerun = runtime
        .try_claim_stage1_job(
            thread_id, owner_b, /*source_updated_at*/ 101, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 newer source");
    assert!(
        matches!(needs_rerun, Stage1JobClaimOutcome::Claimed { .. }),
        "newer source_updated_at should be claimable"
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn stage1_running_stale_can_be_stolen_but_fresh_running_is_skipped() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let owner_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let cwd = codex_home.join("workspace");
    runtime
        .upsert_thread(&test_thread_metadata(&codex_home, thread_id, cwd))
        .await
        .expect("upsert thread");

    let claim_a = runtime
        .try_claim_stage1_job(
            thread_id, owner_a, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim a");
    assert!(matches!(claim_a, Stage1JobClaimOutcome::Claimed { .. }));

    let claim_b_fresh = runtime
        .try_claim_stage1_job(
            thread_id, owner_b, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim b fresh");
    assert_eq!(claim_b_fresh, Stage1JobClaimOutcome::SkippedRunning);

    sqlx::query("UPDATE jobs SET lease_until = 0 WHERE kind = 'memory_stage1' AND job_key = ?")
        .bind(thread_id.to_string())
        .execute(memory_pool(&runtime))
        .await
        .expect("force stale lease");

    let claim_b_stale = runtime
        .try_claim_stage1_job(
            thread_id, owner_b, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim b stale");
    assert!(matches!(
        claim_b_stale,
        Stage1JobClaimOutcome::Claimed { .. }
    ));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn stage1_concurrent_claim_for_same_thread_is_conflict_safe() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.join("workspace"),
        ))
        .await
        .expect("upsert thread");

    let owner_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let owner_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let thread_id_a = thread_id;
    let thread_id_b = thread_id;
    let runtime_a = Arc::clone(&runtime);
    let runtime_b = Arc::clone(&runtime);
    let claim_with_retry = |runtime: Arc<StateRuntime>, thread_id: ThreadId, owner: ThreadId| async move {
        for attempt in 0..5 {
            match runtime
                .try_claim_stage1_job(
                    thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3_600,
                    /*max_running_jobs*/ 64,
                )
                .await
            {
                Ok(outcome) => return outcome,
                Err(err) if err.to_string().contains("database is locked") && attempt < 4 => {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(err) => panic!("claim stage1 should not fail: {err}"),
            }
        }
        panic!("claim stage1 should have returned within retry budget")
    };

    let (claim_a, claim_b) = tokio::join!(
        claim_with_retry(runtime_a, thread_id_a, owner_a),
        claim_with_retry(runtime_b, thread_id_b, owner_b),
    );

    let claim_outcomes = vec![claim_a, claim_b];
    let claimed_count = claim_outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Stage1JobClaimOutcome::Claimed { .. }))
        .count();
    assert_eq!(claimed_count, 1);
    assert!(
        claim_outcomes.iter().all(|outcome| {
            matches!(
                outcome,
                Stage1JobClaimOutcome::Claimed { .. } | Stage1JobClaimOutcome::SkippedRunning
            )
        }),
        "unexpected claim outcomes: {claim_outcomes:?}"
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn stage1_concurrent_claims_respect_running_cap() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let thread_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_a,
            codex_home.join("workspace-a"),
        ))
        .await
        .expect("upsert thread a");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_b,
            codex_home.join("workspace-b"),
        ))
        .await
        .expect("upsert thread b");

    let owner_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let owner_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let runtime_a = Arc::clone(&runtime);
    let runtime_b = Arc::clone(&runtime);

    let (claim_a, claim_b) = tokio::join!(
        async move {
            runtime_a
                .try_claim_stage1_job(
                    thread_a, owner_a, /*source_updated_at*/ 100,
                    /*lease_seconds*/ 3_600, /*max_running_jobs*/ 1,
                )
                .await
                .expect("claim stage1 thread a")
        },
        async move {
            runtime_b
                .try_claim_stage1_job(
                    thread_b, owner_b, /*source_updated_at*/ 101,
                    /*lease_seconds*/ 3_600, /*max_running_jobs*/ 1,
                )
                .await
                .expect("claim stage1 thread b")
        },
    );

    let claim_outcomes = vec![claim_a, claim_b];
    let claimed_count = claim_outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Stage1JobClaimOutcome::Claimed { .. }))
        .count();
    assert_eq!(claimed_count, 1);
    assert!(
        claim_outcomes
            .iter()
            .any(|outcome| { matches!(outcome, Stage1JobClaimOutcome::SkippedRunning) }),
        "one concurrent claim should be throttled by running cap: {claim_outcomes:?}"
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn claim_stage1_jobs_filters_by_age_idle_and_current_thread() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let now = Utc::now();
    let fresh_at = now - Duration::hours(1);
    let just_under_idle_at = now - Duration::hours(12) + Duration::minutes(1);
    let eligible_idle_at = now - Duration::hours(12) - Duration::minutes(1);
    let old_at = now - Duration::days(31);

    let current_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("current thread id");
    let fresh_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("fresh thread id");
    let just_under_idle_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("just under idle thread id");
    let eligible_idle_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("eligible idle thread id");
    let old_thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("old thread id");

    let mut current =
        test_thread_metadata(&codex_home, current_thread_id, codex_home.join("current"));
    current.created_at = now;
    current.updated_at = now;
    runtime
        .upsert_thread(&current)
        .await
        .expect("upsert current");

    let mut fresh = test_thread_metadata(&codex_home, fresh_thread_id, codex_home.join("fresh"));
    fresh.created_at = fresh_at;
    fresh.updated_at = fresh_at;
    runtime.upsert_thread(&fresh).await.expect("upsert fresh");

    let mut just_under_idle = test_thread_metadata(
        &codex_home,
        just_under_idle_thread_id,
        codex_home.join("just-under-idle"),
    );
    just_under_idle.created_at = just_under_idle_at;
    just_under_idle.updated_at = just_under_idle_at;
    runtime
        .upsert_thread(&just_under_idle)
        .await
        .expect("upsert just-under-idle");

    let mut eligible_idle = test_thread_metadata(
        &codex_home,
        eligible_idle_thread_id,
        codex_home.join("eligible-idle"),
    );
    eligible_idle.created_at = eligible_idle_at;
    eligible_idle.updated_at = eligible_idle_at;
    runtime
        .upsert_thread(&eligible_idle)
        .await
        .expect("upsert eligible-idle");

    let mut old = test_thread_metadata(&codex_home, old_thread_id, codex_home.join("old"));
    old.created_at = old_at;
    old.updated_at = old_at;
    runtime.upsert_thread(&old).await.expect("upsert old");

    let allowed_sources = vec!["cli".to_string()];
    let claims = runtime
        .claim_stage1_jobs_for_startup(
            current_thread_id,
            Stage1StartupClaimParams {
                scan_limit: 1,
                max_claimed: 5,
                max_age_days: 30,
                min_rollout_idle_hours: 12,
                allowed_sources: allowed_sources.as_slice(),
                lease_seconds: 3600,
            },
        )
        .await
        .expect("claim stage1 jobs");

    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].thread.id, eligible_idle_thread_id);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn claim_stage1_jobs_bounds_state_scan_before_memory_probes() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let now = Utc::now();
    let eligible_newer_at = now - Duration::hours(13);
    let eligible_older_at = now - Duration::hours(14);

    let current_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("current thread id");
    let up_to_date_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("up-to-date thread id");
    let stale_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("stale thread id");
    let worker_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("worker id");

    let mut current =
        test_thread_metadata(&codex_home, current_thread_id, codex_home.join("current"));
    current.created_at = now;
    current.updated_at = now;
    runtime
        .upsert_thread(&current)
        .await
        .expect("upsert current thread");

    let mut up_to_date = test_thread_metadata(
        &codex_home,
        up_to_date_thread_id,
        codex_home.join("up-to-date"),
    );
    up_to_date.created_at = eligible_newer_at;
    up_to_date.updated_at = eligible_newer_at;
    runtime
        .upsert_thread(&up_to_date)
        .await
        .expect("upsert up-to-date thread");

    let up_to_date_claim = runtime
        .try_claim_stage1_job(
            up_to_date_thread_id,
            worker_id,
            up_to_date.updated_at.timestamp(),
            /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim up-to-date thread for seed");
    let up_to_date_token = match up_to_date_claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected seed claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                up_to_date_thread_id,
                up_to_date_token.as_str(),
                up_to_date.updated_at.timestamp(),
                "raw",
                "summary",
                /*rollout_slug*/ None,
            )
            .await
            .expect("mark up-to-date thread succeeded"),
        "seed stage1 success should complete for up-to-date thread"
    );

    let mut stale = test_thread_metadata(&codex_home, stale_thread_id, codex_home.join("stale"));
    stale.created_at = eligible_older_at;
    stale.updated_at = eligible_older_at;
    runtime
        .upsert_thread(&stale)
        .await
        .expect("upsert stale thread");

    let allowed_sources = vec!["cli".to_string()];
    let claims_with_one_scanned_thread = runtime
        .claim_stage1_jobs_for_startup(
            current_thread_id,
            Stage1StartupClaimParams {
                scan_limit: 1,
                max_claimed: 1,
                max_age_days: 30,
                min_rollout_idle_hours: 12,
                allowed_sources: allowed_sources.as_slice(),
                lease_seconds: 3600,
            },
        )
        .await
        .expect("claim stage1 startup jobs");
    assert_eq!(claims_with_one_scanned_thread.len(), 0);

    let claims = runtime
        .claim_stage1_jobs_for_startup(
            current_thread_id,
            Stage1StartupClaimParams {
                scan_limit: 2,
                max_claimed: 1,
                max_age_days: 30,
                min_rollout_idle_hours: 12,
                allowed_sources: allowed_sources.as_slice(),
                lease_seconds: 3600,
            },
        )
        .await
        .expect("claim stage1 startup jobs with wider scan");
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].thread.id, stale_thread_id);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn claim_stage1_jobs_skips_threads_without_legacy_enabled_memory() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let now = Utc::now();
    let eligible_at = now - Duration::hours(13);

    let current_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("current thread id");
    let disabled_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("disabled thread id");
    let paginated_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("paginated thread id");
    let enabled_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("enabled thread id");

    let mut current =
        test_thread_metadata(&codex_home, current_thread_id, codex_home.join("current"));
    current.created_at = now;
    current.updated_at = now;
    runtime
        .upsert_thread(&current)
        .await
        .expect("upsert current thread");

    let mut disabled =
        test_thread_metadata(&codex_home, disabled_thread_id, codex_home.join("disabled"));
    disabled.created_at = eligible_at;
    disabled.updated_at = eligible_at;
    runtime
        .upsert_thread(&disabled)
        .await
        .expect("upsert disabled thread");
    sqlx::query("UPDATE threads SET memory_mode = 'disabled' WHERE id = ?")
        .bind(disabled_thread_id.to_string())
        .execute(runtime.pool.as_ref())
        .await
        .expect("disable thread memory mode");

    let mut paginated = test_thread_metadata(
        &codex_home,
        paginated_thread_id,
        codex_home.join("paginated"),
    );
    paginated.created_at = eligible_at;
    paginated.updated_at = eligible_at;
    paginated.history_mode = ThreadHistoryMode::Paginated;
    runtime
        .upsert_thread(&paginated)
        .await
        .expect("upsert paginated thread");

    let mut enabled =
        test_thread_metadata(&codex_home, enabled_thread_id, codex_home.join("enabled"));
    enabled.created_at = eligible_at;
    enabled.updated_at = eligible_at;
    runtime
        .upsert_thread(&enabled)
        .await
        .expect("upsert enabled thread");

    let allowed_sources = vec!["cli".to_string()];
    let claims = runtime
        .claim_stage1_jobs_for_startup(
            current_thread_id,
            Stage1StartupClaimParams {
                scan_limit: 10,
                max_claimed: 10,
                max_age_days: 30,
                min_rollout_idle_hours: 12,
                allowed_sources: allowed_sources.as_slice(),
                lease_seconds: 3600,
            },
        )
        .await
        .expect("claim stage1 startup jobs");

    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].thread.id, enabled_thread_id);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn clear_memory_data_clears_rows_and_preserves_thread_memory_modes() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let now = Utc::now() - Duration::hours(13);
    let worker_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("worker id");
    let enabled_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("enabled thread id");
    let disabled_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("disabled thread id");

    let mut enabled =
        test_thread_metadata(&codex_home, enabled_thread_id, codex_home.join("enabled"));
    enabled.created_at = now;
    enabled.updated_at = now;
    runtime
        .upsert_thread(&enabled)
        .await
        .expect("upsert enabled thread");

    let claim = runtime
        .try_claim_stage1_job(
            enabled_thread_id,
            worker_id,
            enabled.updated_at.timestamp(),
            /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim enabled thread");
    let ownership_token = match claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                enabled_thread_id,
                ownership_token.as_str(),
                enabled.updated_at.timestamp(),
                "raw",
                "summary",
                /*rollout_slug*/ None,
            )
            .await
            .expect("mark enabled thread succeeded"),
        "stage1 success should be recorded"
    );
    runtime
        .enqueue_global_consolidation(enabled.updated_at.timestamp())
        .await
        .expect("enqueue global consolidation");

    let mut disabled =
        test_thread_metadata(&codex_home, disabled_thread_id, codex_home.join("disabled"));
    disabled.created_at = now;
    disabled.updated_at = now;
    runtime
        .upsert_thread(&disabled)
        .await
        .expect("upsert disabled thread");
    sqlx::query("UPDATE threads SET memory_mode = 'disabled' WHERE id = ?")
        .bind(disabled_thread_id.to_string())
        .execute(runtime.pool.as_ref())
        .await
        .expect("disable existing thread");

    runtime
        .clear_memory_data()
        .await
        .expect("clear memory data");

    let stage1_outputs_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stage1_outputs")
        .fetch_one(memory_pool(&runtime))
        .await
        .expect("count stage1 outputs");
    assert_eq!(stage1_outputs_count, 0);

    let memory_jobs_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind = ? OR kind = ?")
            .bind(JOB_KIND_MEMORY_STAGE1)
            .bind(JOB_KIND_MEMORY_CONSOLIDATE_GLOBAL)
            .fetch_one(memory_pool(&runtime))
            .await
            .expect("count memory jobs");
    assert_eq!(memory_jobs_count, 0);

    let enabled_memory_mode: String =
        sqlx::query_scalar("SELECT memory_mode FROM threads WHERE id = ?")
            .bind(enabled_thread_id.to_string())
            .fetch_one(runtime.pool.as_ref())
            .await
            .expect("read enabled thread memory mode");
    assert_eq!(enabled_memory_mode, "enabled");

    let disabled_memory_mode: String =
        sqlx::query_scalar("SELECT memory_mode FROM threads WHERE id = ?")
            .bind(disabled_thread_id.to_string())
            .fetch_one(runtime.pool.as_ref())
            .await
            .expect("read disabled thread memory mode");
    assert_eq!(disabled_memory_mode, "disabled");

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn claim_stage1_jobs_enforces_global_running_cap() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let current_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("current thread id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            current_thread_id,
            codex_home.join("current"),
        ))
        .await
        .expect("upsert current");

    let now = Utc::now();
    let started_at = now.timestamp();
    let lease_until = started_at + 3600;
    let eligible_at = now - Duration::hours(13);
    let existing_running = 10usize;
    let total_candidates = 80usize;

    for idx in 0..total_candidates {
        let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
        let mut metadata = test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.join(format!("thread-{idx}")),
        );
        metadata.created_at = eligible_at - Duration::seconds(idx as i64);
        metadata.updated_at = eligible_at - Duration::seconds(idx as i64);
        runtime
            .upsert_thread(&metadata)
            .await
            .expect("upsert thread");

        if idx < existing_running {
            sqlx::query(
                r#"
INSERT INTO jobs (
kind,
job_key,
status,
worker_id,
ownership_token,
started_at,
finished_at,
lease_until,
retry_at,
retry_remaining,
last_error,
input_watermark,
last_success_watermark
) VALUES (?, ?, 'running', ?, ?, ?, NULL, ?, NULL, ?, NULL, ?, NULL)
                "#,
            )
            .bind("memory_stage1")
            .bind(thread_id.to_string())
            .bind(current_thread_id.to_string())
            .bind(Uuid::new_v4().to_string())
            .bind(started_at)
            .bind(lease_until)
            .bind(3)
            .bind(metadata.updated_at.timestamp())
            .execute(memory_pool(&runtime))
            .await
            .expect("seed running stage1 job");
        }
    }

    let allowed_sources = vec!["cli".to_string()];
    let claims = runtime
        .claim_stage1_jobs_for_startup(
            current_thread_id,
            Stage1StartupClaimParams {
                scan_limit: 200,
                max_claimed: 64,
                max_age_days: 30,
                min_rollout_idle_hours: 12,
                allowed_sources: allowed_sources.as_slice(),
                lease_seconds: 3600,
            },
        )
        .await
        .expect("claim stage1 jobs");
    assert_eq!(claims.len(), 54);

    let running_count = sqlx::query(
        r#"
SELECT COUNT(*) AS count
FROM jobs
WHERE kind = 'memory_stage1'
  AND status = 'running'
  AND lease_until IS NOT NULL
  AND lease_until > ?
        "#,
    )
    .bind(Utc::now().timestamp())
    .fetch_one(memory_pool(&runtime))
    .await
    .expect("count running stage1 jobs")
    .try_get::<i64, _>("count")
    .expect("running count value");
    assert_eq!(running_count, 64);

    let more_claims = runtime
        .claim_stage1_jobs_for_startup(
            current_thread_id,
            Stage1StartupClaimParams {
                scan_limit: 200,
                max_claimed: 64,
                max_age_days: 30,
                min_rollout_idle_hours: 12,
                allowed_sources: allowed_sources.as_slice(),
                lease_seconds: 3600,
            },
        )
        .await
        .expect("claim stage1 jobs with cap reached");
    assert_eq!(more_claims.len(), 0);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn claim_stage1_jobs_processes_two_full_batches_across_startup_passes() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let current_thread_id =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("current thread id");
    let mut current =
        test_thread_metadata(&codex_home, current_thread_id, codex_home.join("current"));
    current.created_at = Utc::now();
    current.updated_at = Utc::now();
    runtime
        .upsert_thread(&current)
        .await
        .expect("upsert current");

    let eligible_at = Utc::now() - Duration::hours(13);
    for idx in 0..200 {
        let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
        let mut metadata = test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.join(format!("thread-{idx}")),
        );
        metadata.created_at = eligible_at - Duration::seconds(idx as i64);
        metadata.updated_at = eligible_at - Duration::seconds(idx as i64);
        runtime
            .upsert_thread(&metadata)
            .await
            .expect("upsert eligible thread");
    }

    let allowed_sources = vec!["cli".to_string()];
    let first_claims = runtime
        .claim_stage1_jobs_for_startup(
            current_thread_id,
            Stage1StartupClaimParams {
                scan_limit: 5_000,
                max_claimed: 64,
                max_age_days: 30,
                min_rollout_idle_hours: 12,
                allowed_sources: allowed_sources.as_slice(),
                lease_seconds: 3_600,
            },
        )
        .await
        .expect("first stage1 startup claim");
    assert_eq!(first_claims.len(), 64);

    for claim in first_claims {
        assert!(
            runtime
                .mark_stage1_job_succeeded(
                    claim.thread.id,
                    claim.ownership_token.as_str(),
                    claim.thread.updated_at.timestamp(),
                    "raw",
                    "summary",
                    /*rollout_slug*/ None,
                )
                .await
                .expect("mark first-batch stage1 success"),
            "first batch stage1 completion should succeed"
        );
    }

    let second_claims = runtime
        .claim_stage1_jobs_for_startup(
            current_thread_id,
            Stage1StartupClaimParams {
                scan_limit: 5_000,
                max_claimed: 64,
                max_age_days: 30,
                min_rollout_idle_hours: 12,
                allowed_sources: allowed_sources.as_slice(),
                lease_seconds: 3_600,
            },
        )
        .await
        .expect("second stage1 startup claim");
    assert_eq!(second_claims.len(), 64);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn delete_thread_removes_stage1_output_and_enqueues_phase2_when_selected() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let cwd = codex_home.join("workspace");
    runtime
        .upsert_thread(&test_thread_metadata(&codex_home, thread_id, cwd))
        .await
        .expect("upsert thread");

    let claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1");
    let ownership_token = match claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id,
                ownership_token.as_str(),
                /*source_updated_at*/ 100,
                "raw",
                "sum",
                /*rollout_slug*/ None,
            )
            .await
            .expect("mark stage1 succeeded"),
        "mark stage1 succeeded should write stage1_outputs"
    );

    let count_before =
        sqlx::query("SELECT COUNT(*) AS count FROM stage1_outputs WHERE thread_id = ?")
            .bind(thread_id.to_string())
            .fetch_one(memory_pool(&runtime))
            .await
            .expect("count before delete")
            .try_get::<i64, _>("count")
            .expect("count value");
    assert_eq!(count_before, 1);

    let phase2_claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2");
    let (phase2_token, input_watermark) = match phase2_claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected phase2 claim outcome: {other:?}"),
    };
    let selected_outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 10)
        .await
        .expect("list stage1 outputs");
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                phase2_token.as_str(),
                input_watermark,
                &selected_outputs,
            )
            .await
            .expect("mark phase2 succeeded"),
        "phase2 success should mark selected stage1 output"
    );

    let before_delete = Utc::now().timestamp();
    assert_eq!(
        runtime
            .delete_thread(thread_id)
            .await
            .expect("delete thread"),
        1
    );

    let count_after =
        sqlx::query("SELECT COUNT(*) AS count FROM stage1_outputs WHERE thread_id = ?")
            .bind(thread_id.to_string())
            .fetch_one(memory_pool(&runtime))
            .await
            .expect("count after delete")
            .try_get::<i64, _>("count")
            .expect("count value");
    assert_eq!(count_after, 0);

    let phase2_job = sqlx::query(
        r#"
SELECT status, input_watermark
FROM jobs
WHERE kind = ? AND job_key = ?
        "#,
    )
    .bind(JOB_KIND_MEMORY_CONSOLIDATE_GLOBAL)
    .bind(MEMORY_CONSOLIDATION_JOB_KEY)
    .fetch_one(memory_pool(&runtime))
    .await
    .expect("load phase2 job after delete");
    let status: String = phase2_job.try_get("status").expect("status");
    let input_watermark: i64 = phase2_job
        .try_get("input_watermark")
        .expect("input watermark");
    assert_eq!(status, "pending");
    assert!(input_watermark >= before_delete);

    let visible_outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 10)
        .await
        .expect("list stage1 outputs after thread delete");
    assert_eq!(visible_outputs.len(), 0);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn mark_stage1_job_succeeded_no_output_skips_phase2_when_output_was_already_absent() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let owner_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.join("workspace"),
        ))
        .await
        .expect("upsert thread");

    let claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1");
    let ownership_token = match claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded_no_output(thread_id, ownership_token.as_str())
            .await
            .expect("mark stage1 succeeded without output"),
        "stage1 no-output success should complete the job"
    );

    let output_row_count =
        sqlx::query("SELECT COUNT(*) AS count FROM stage1_outputs WHERE thread_id = ?")
            .bind(thread_id.to_string())
            .fetch_one(memory_pool(&runtime))
            .await
            .expect("load stage1 output count")
            .try_get::<i64, _>("count")
            .expect("stage1 output count");
    assert_eq!(
        output_row_count, 0,
        "stage1 no-output success should not persist empty stage1 outputs"
    );

    let up_to_date = runtime
        .try_claim_stage1_job(
            thread_id, owner_b, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 up-to-date");
    assert_eq!(up_to_date, Stage1JobClaimOutcome::SkippedUpToDate);

    let global_job_row_count = sqlx::query("SELECT COUNT(*) AS count FROM jobs WHERE kind = ?")
        .bind("memory_consolidate_global")
        .fetch_one(memory_pool(&runtime))
        .await
        .expect("load phase2 job row count")
        .try_get::<i64, _>("count")
        .expect("phase2 job row count");
    assert_eq!(
        global_job_row_count, 0,
        "no-output without an existing stage1 output should not enqueue phase2"
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn mark_stage1_job_succeeded_no_output_enqueues_phase2_when_deleting_output() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let owner_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.join("workspace"),
        ))
        .await
        .expect("upsert thread");

    let first_claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim initial stage1");
    let first_token = match first_claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected initial stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id,
                first_token.as_str(),
                /*source_updated_at*/ 100,
                "raw",
                "sum",
                /*rollout_slug*/ None
            )
            .await
            .expect("mark initial stage1 succeeded"),
        "initial stage1 success should create stage1 output"
    );

    let phase2_claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2 after initial output");
    let (phase2_token, phase2_input_watermark) = match phase2_claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected phase2 claim after initial output: {other:?}"),
    };
    assert_eq!(phase2_input_watermark, 100);
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(phase2_token.as_str(), phase2_input_watermark, &[],)
            .await
            .expect("mark initial phase2 succeeded"),
        "initial phase2 success should finalize the global job"
    );

    let no_output_claim = runtime
        .try_claim_stage1_job(
            thread_id, owner_b, /*source_updated_at*/ 101, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 for no-output delete");
    let no_output_token = match no_output_claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected no-output stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded_no_output(thread_id, no_output_token.as_str())
            .await
            .expect("mark stage1 no-output after existing output"),
        "no-output should succeed when deleting an existing stage1 output"
    );

    let output_row_count =
        sqlx::query("SELECT COUNT(*) AS count FROM stage1_outputs WHERE thread_id = ?")
            .bind(thread_id.to_string())
            .fetch_one(memory_pool(&runtime))
            .await
            .expect("load stage1 output count after delete")
            .try_get::<i64, _>("count")
            .expect("stage1 output count");
    assert_eq!(output_row_count, 0);

    age_phase2_success_beyond_cooldown(&runtime).await;
    let claim_phase2 = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2 after no-output deletion");
    let (phase2_token, phase2_input_watermark) = match claim_phase2 {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected phase2 claim after no-output deletion: {other:?}"),
    };
    assert_eq!(phase2_input_watermark, 101);
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(phase2_token.as_str(), phase2_input_watermark, &[],)
            .await
            .expect("mark phase2 succeeded after no-output delete")
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn stage1_retry_exhaustion_does_not_block_newer_watermark() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.join("workspace"),
        ))
        .await
        .expect("upsert thread");

    for attempt in 0..3 {
        let claim = runtime
            .try_claim_stage1_job(
                thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3_600,
                /*max_running_jobs*/ 64,
            )
            .await
            .expect("claim stage1 for retry exhaustion");
        let ownership_token = match claim {
            Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
            other => panic!(
                "attempt {} should claim stage1 before retries are exhausted: {other:?}",
                attempt + 1
            ),
        };
        assert!(
            runtime
                .mark_stage1_job_failed(
                    thread_id,
                    ownership_token.as_str(),
                    "boom",
                    /*retry_delay_seconds*/ 0
                )
                .await
                .expect("mark stage1 failed"),
            "attempt {} should decrement retry budget",
            attempt + 1
        );
    }

    let exhausted_claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3_600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 after retry exhaustion");
    assert_eq!(
        exhausted_claim,
        Stage1JobClaimOutcome::SkippedRetryExhausted
    );

    let newer_source_claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 101, /*lease_seconds*/ 3_600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 with newer source watermark");
    assert!(
        matches!(newer_source_claim, Stage1JobClaimOutcome::Claimed { .. }),
        "newer source watermark should reset retry budget and be claimable"
    );

    let job_row = sqlx::query(
        "SELECT retry_remaining, input_watermark FROM jobs WHERE kind = ? AND job_key = ?",
    )
    .bind("memory_stage1")
    .bind(thread_id.to_string())
    .fetch_one(memory_pool(&runtime))
    .await
    .expect("load stage1 job row after newer-source claim");
    assert_eq!(
        job_row
            .try_get::<i64, _>("retry_remaining")
            .expect("retry_remaining"),
        3
    );
    assert_eq!(
        job_row
            .try_get::<i64, _>("input_watermark")
            .expect("input_watermark"),
        101
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn phase2_global_lock_respects_success_cooldown() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");

    runtime
        .enqueue_global_consolidation(/*input_watermark*/ 100)
        .await
        .expect("enqueue global consolidation");

    let claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2");
    let (ownership_token, input_watermark) = match claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected phase2 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(ownership_token.as_str(), input_watermark, &[],)
            .await
            .expect("mark phase2 succeeded"),
        "phase2 success should finalize for current token"
    );

    let claim_after_success = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2 after success");
    assert_eq!(claim_after_success, Phase2JobClaimOutcome::SkippedCooldown);

    runtime
        .enqueue_global_consolidation(/*input_watermark*/ 101)
        .await
        .expect("enqueue global consolidation after success");
    let claim_after_enqueue = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2 after enqueue");
    assert_eq!(claim_after_enqueue, Phase2JobClaimOutcome::SkippedCooldown);

    age_phase2_success_beyond_cooldown(&runtime).await;
    let claim_after_cooldown = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2 after cooldown");
    assert!(matches!(
        claim_after_cooldown,
        Phase2JobClaimOutcome::Claimed { .. }
    ));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn phase2_global_lock_can_be_claimed_after_retry_budget_is_exhausted() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .enqueue_global_consolidation(/*input_watermark*/ 100)
        .await
        .expect("enqueue global consolidation");

    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    for attempt in 0..3 {
        let claim = runtime
            .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3_600)
            .await
            .expect("claim phase2 before retry exhaustion");
        let ownership_token = match claim {
            Phase2JobClaimOutcome::Claimed {
                ownership_token, ..
            } => ownership_token,
            other => panic!(
                "attempt {} should claim phase2 before retries are exhausted: {other:?}",
                attempt + 1
            ),
        };
        assert!(
            runtime
                .mark_global_phase2_job_failed(
                    ownership_token.as_str(),
                    "boom",
                    /*retry_delay_seconds*/ 0,
                )
                .await
                .expect("mark phase2 failed"),
            "attempt {} should decrement retry budget",
            attempt + 1
        );
    }

    let job_row = sqlx::query("SELECT retry_remaining FROM jobs WHERE kind = ? AND job_key = ?")
        .bind("memory_consolidate_global")
        .bind("global")
        .fetch_one(memory_pool(&runtime))
        .await
        .expect("load phase2 job row after retry exhaustion");
    assert_eq!(
        job_row
            .try_get::<i64, _>("retry_remaining")
            .expect("retry_remaining"),
        0
    );

    let claim_after_exhaustion = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3_600)
        .await
        .expect("claim phase2 after retry exhaustion");
    assert!(
        matches!(
            claim_after_exhaustion,
            Phase2JobClaimOutcome::Claimed { .. }
        ),
        "phase2 claim should only lock; workspace diffing decides whether there is work"
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn list_stage1_outputs_for_global_returns_latest_outputs() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let thread_id_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id_a,
            codex_home.join("workspace-a"),
        ))
        .await
        .expect("upsert thread a");
    let mut metadata_b =
        test_thread_metadata(&codex_home, thread_id_b, codex_home.join("workspace-b"));
    metadata_b.git_branch = Some("feature/stage1-b".to_string());
    runtime
        .upsert_thread(&metadata_b)
        .await
        .expect("upsert thread b");

    let claim = runtime
        .try_claim_stage1_job(
            thread_id_a,
            owner,
            /*source_updated_at*/ 100,
            /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 a");
    let ownership_token = match claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id_a,
                ownership_token.as_str(),
                /*source_updated_at*/ 100,
                "raw memory a",
                "summary a",
                /*rollout_slug*/ None,
            )
            .await
            .expect("mark stage1 succeeded a"),
        "stage1 success should persist output a"
    );

    let claim = runtime
        .try_claim_stage1_job(
            thread_id_b,
            owner,
            /*source_updated_at*/ 101,
            /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 b");
    let ownership_token = match claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id_b,
                ownership_token.as_str(),
                /*source_updated_at*/ 101,
                "raw memory b",
                "summary b",
                Some("rollout-b"),
            )
            .await
            .expect("mark stage1 succeeded b"),
        "stage1 success should persist output b"
    );

    let outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 10)
        .await
        .expect("list stage1 outputs for global");
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].thread_id, thread_id_b);
    assert_eq!(outputs[0].rollout_summary, "summary b");
    assert_eq!(outputs[0].rollout_slug.as_deref(), Some("rollout-b"));
    assert_eq!(outputs[0].cwd, codex_home.join("workspace-b"));
    assert_eq!(outputs[0].git_branch.as_deref(), Some("feature/stage1-b"));
    assert_eq!(outputs[1].thread_id, thread_id_a);
    assert_eq!(outputs[1].rollout_summary, "summary a");
    assert_eq!(outputs[1].rollout_slug, None);
    assert_eq!(outputs[1].cwd, codex_home.join("workspace-a"));
    assert_eq!(outputs[1].git_branch, None);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn list_stage1_outputs_for_global_skips_empty_payloads() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id_non_empty =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let thread_id_empty = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id_non_empty,
            codex_home.join("workspace-non-empty"),
        ))
        .await
        .expect("upsert non-empty thread");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id_empty,
            codex_home.join("workspace-empty"),
        ))
        .await
        .expect("upsert empty thread");

    sqlx::query(
        r#"
INSERT INTO stage1_outputs (thread_id, source_updated_at, raw_memory, rollout_summary, generated_at)
VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(thread_id_non_empty.to_string())
    .bind(100_i64)
    .bind("raw memory")
    .bind("summary")
    .bind(100_i64)
    .execute(memory_pool(&runtime))
    .await
    .expect("insert non-empty stage1 output");
    sqlx::query(
        r#"
INSERT INTO stage1_outputs (thread_id, source_updated_at, raw_memory, rollout_summary, generated_at)
VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(thread_id_empty.to_string())
    .bind(101_i64)
    .bind("")
    .bind("")
    .bind(101_i64)
    .execute(memory_pool(&runtime))
    .await
    .expect("insert empty stage1 output");

    let outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 1)
        .await
        .expect("list stage1 outputs for global");
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].thread_id, thread_id_non_empty);
    assert_eq!(outputs[0].rollout_summary, "summary");
    assert_eq!(outputs[0].cwd, codex_home.join("workspace-non-empty"));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn list_stage1_outputs_for_global_skips_polluted_threads() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id_enabled = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let thread_id_polluted = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");

    for (thread_id, workspace) in [
        (thread_id_enabled, "workspace-enabled"),
        (thread_id_polluted, "workspace-polluted"),
    ] {
        runtime
            .upsert_thread(&test_thread_metadata(
                &codex_home,
                thread_id,
                codex_home.join(workspace),
            ))
            .await
            .expect("upsert thread");

        let claim = runtime
            .try_claim_stage1_job(
                thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
                /*max_running_jobs*/ 64,
            )
            .await
            .expect("claim stage1");
        let ownership_token = match claim {
            Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
            other => panic!("unexpected stage1 claim outcome: {other:?}"),
        };
        assert!(
            runtime
                .mark_stage1_job_succeeded(
                    thread_id,
                    ownership_token.as_str(),
                    /*source_updated_at*/ 100,
                    "raw memory",
                    "summary",
                    /*rollout_slug*/ None,
                )
                .await
                .expect("mark stage1 succeeded"),
            "stage1 success should persist output"
        );
    }

    runtime
        .set_thread_memory_mode(thread_id_polluted, "polluted")
        .await
        .expect("mark thread polluted");

    let outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 10)
        .await
        .expect("list stage1 outputs for global");
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].thread_id, thread_id_enabled);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn get_phase2_input_selection_returns_current_selected_rows() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id_a = stable_thread_id("00000000-0000-4000-8000-000000000001");
    let thread_id_b = stable_thread_id("00000000-0000-4000-8000-000000000002");
    let thread_id_c = stable_thread_id("00000000-0000-4000-8000-000000000003");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");

    for (thread_id, workspace) in [
        (thread_id_a, "workspace-a"),
        (thread_id_b, "workspace-b"),
        (thread_id_c, "workspace-c"),
    ] {
        runtime
            .upsert_thread(&test_thread_metadata(
                &codex_home,
                thread_id,
                codex_home.join(workspace),
            ))
            .await
            .expect("upsert thread");
    }

    for (thread_id, updated_at, slug) in [
        (thread_id_a, 100, Some("rollout-a")),
        (thread_id_b, 101, Some("rollout-b")),
        (thread_id_c, 102, Some("rollout-c")),
    ] {
        let claim = runtime
            .try_claim_stage1_job(
                thread_id, owner, updated_at, /*lease_seconds*/ 3600,
                /*max_running_jobs*/ 64,
            )
            .await
            .expect("claim stage1");
        let ownership_token = match claim {
            Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
            other => panic!("unexpected stage1 claim outcome: {other:?}"),
        };
        assert!(
            runtime
                .mark_stage1_job_succeeded(
                    thread_id,
                    ownership_token.as_str(),
                    updated_at,
                    &format!("raw-{updated_at}"),
                    &format!("summary-{updated_at}"),
                    slug,
                )
                .await
                .expect("mark stage1 succeeded"),
            "stage1 success should persist output"
        );
    }

    let claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2");
    let (ownership_token, input_watermark) = match claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected phase2 claim outcome: {other:?}"),
    };
    assert_eq!(input_watermark, 102);
    let selected_outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 10)
        .await
        .expect("list stage1 outputs for global")
        .into_iter()
        .filter(|output| output.thread_id == thread_id_c || output.thread_id == thread_id_a)
        .collect::<Vec<_>>();
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                ownership_token.as_str(),
                input_watermark,
                &selected_outputs,
            )
            .await
            .expect("mark phase2 success with selection"),
        "phase2 success should persist selected rows"
    );

    let selection = runtime
        .get_phase2_input_selection(/*n*/ 2, /*max_unused_days*/ 36_500)
        .await
        .expect("load phase2 input selection");

    assert_eq!(selection.len(), 2);
    assert_eq!(
        selection
            .iter()
            .map(|output| output.thread_id)
            .collect::<Vec<_>>(),
        vec![thread_id_b, thread_id_c]
    );
    let selected_c = selection
        .iter()
        .find(|output| output.thread_id == thread_id_c)
        .expect("thread c should be selected");
    assert_eq!(
        selected_c.rollout_path,
        codex_home.join(format!("rollout-{thread_id_c}.jsonl"))
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn get_phase2_input_selection_excludes_polluted_previous_selection() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id_enabled = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let thread_id_polluted = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");

    for (thread_id, updated_at) in [(thread_id_enabled, 100), (thread_id_polluted, 101)] {
        runtime
            .upsert_thread(&test_thread_metadata(
                &codex_home,
                thread_id,
                codex_home.join(thread_id.to_string()),
            ))
            .await
            .expect("upsert thread");

        let claim = runtime
            .try_claim_stage1_job(
                thread_id, owner, updated_at, /*lease_seconds*/ 3600,
                /*max_running_jobs*/ 64,
            )
            .await
            .expect("claim stage1");
        let ownership_token = match claim {
            Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
            other => panic!("unexpected stage1 claim outcome: {other:?}"),
        };
        assert!(
            runtime
                .mark_stage1_job_succeeded(
                    thread_id,
                    ownership_token.as_str(),
                    updated_at,
                    &format!("raw-{updated_at}"),
                    &format!("summary-{updated_at}"),
                    /*rollout_slug*/ None,
                )
                .await
                .expect("mark stage1 succeeded"),
            "stage1 success should persist output"
        );
    }

    let claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2");
    let (ownership_token, input_watermark) = match claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected phase2 claim outcome: {other:?}"),
    };
    let selected_outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 10)
        .await
        .expect("list stage1 outputs for global");
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                ownership_token.as_str(),
                input_watermark,
                &selected_outputs,
            )
            .await
            .expect("mark phase2 success"),
        "phase2 success should persist selected rows"
    );

    runtime
        .set_thread_memory_mode(thread_id_polluted, "polluted")
        .await
        .expect("mark thread polluted");

    let selection = runtime
        .get_phase2_input_selection(/*n*/ 2, /*max_unused_days*/ 36_500)
        .await
        .expect("load phase2 input selection");

    assert_eq!(selection.len(), 1);
    assert_eq!(selection[0].thread_id, thread_id_enabled);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn mark_thread_memory_mode_polluted_enqueues_phase2_for_selected_threads() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.join("workspace"),
        ))
        .await
        .expect("upsert thread");

    let claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1");
    let ownership_token = match claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id,
                ownership_token.as_str(),
                /*source_updated_at*/ 100,
                "raw",
                "summary",
                /*rollout_slug*/ None,
            )
            .await
            .expect("mark stage1 succeeded"),
        "stage1 success should persist output"
    );

    let phase2_claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2");
    let (phase2_token, input_watermark) = match phase2_claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected phase2 claim outcome: {other:?}"),
    };
    let selected_outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 10)
        .await
        .expect("list stage1 outputs");
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                phase2_token.as_str(),
                input_watermark,
                &selected_outputs,
            )
            .await
            .expect("mark phase2 success"),
        "phase2 success should persist selected rows"
    );

    assert!(
        runtime
            .mark_thread_memory_mode_polluted(thread_id)
            .await
            .expect("mark thread polluted"),
        "thread should transition to polluted"
    );

    age_phase2_success_beyond_cooldown(&runtime).await;
    let next_claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2 after pollution");
    assert!(matches!(next_claim, Phase2JobClaimOutcome::Claimed { .. }));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn mark_thread_memory_mode_polluted_enqueues_phase2_when_already_polluted() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.join("workspace"),
        ))
        .await
        .expect("upsert thread");

    let claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1");
    let ownership_token = match claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id,
                ownership_token.as_str(),
                /*source_updated_at*/ 100,
                "raw",
                "summary",
                /*rollout_slug*/ None,
            )
            .await
            .expect("mark stage1 succeeded"),
        "stage1 success should persist output"
    );

    let phase2_claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2");
    let (phase2_token, input_watermark) = match phase2_claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected phase2 claim outcome: {other:?}"),
    };
    let selected_outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 10)
        .await
        .expect("list stage1 outputs");
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                phase2_token.as_str(),
                input_watermark,
                &selected_outputs,
            )
            .await
            .expect("mark phase2 success"),
        "phase2 success should persist selected rows"
    );

    sqlx::query("UPDATE threads SET memory_mode = 'polluted' WHERE id = ?")
        .bind(thread_id.to_string())
        .execute(runtime.pool.as_ref())
        .await
        .expect("mark thread polluted before memory enqueue");

    assert!(
        !runtime
            .mark_thread_memory_mode_polluted(thread_id)
            .await
            .expect("mark already polluted thread"),
        "already polluted thread should not report a state transition"
    );

    age_phase2_success_beyond_cooldown(&runtime).await;
    let next_claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2 after already-polluted enqueue");
    assert!(matches!(next_claim, Phase2JobClaimOutcome::Claimed { .. }));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn get_phase2_input_selection_returns_regenerated_selected_rows() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.join("workspace"),
        ))
        .await
        .expect("upsert thread");

    let first_claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim initial stage1");
    let first_token = match first_claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id,
                first_token.as_str(),
                /*source_updated_at*/ 100,
                "raw-100",
                "summary-100",
                Some("rollout-100"),
            )
            .await
            .expect("mark initial stage1 success"),
        "initial stage1 success should persist output"
    );

    let phase2_claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2");
    let (phase2_token, input_watermark) = match phase2_claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected phase2 claim outcome: {other:?}"),
    };
    let selected_outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 1)
        .await
        .expect("list selected outputs");
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                phase2_token.as_str(),
                input_watermark,
                &selected_outputs,
            )
            .await
            .expect("mark phase2 success"),
        "phase2 success should persist selected rows"
    );

    let refreshed_claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 101, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim refreshed stage1");
    let refreshed_token = match refreshed_claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id,
                refreshed_token.as_str(),
                /*source_updated_at*/ 101,
                "raw-101",
                "summary-101",
                Some("rollout-101"),
            )
            .await
            .expect("mark refreshed stage1 success"),
        "refreshed stage1 success should persist output"
    );

    let selection = runtime
        .get_phase2_input_selection(/*n*/ 1, /*max_unused_days*/ 36_500)
        .await
        .expect("load phase2 input selection");
    assert_eq!(selection.len(), 1);
    assert_eq!(selection[0].thread_id, thread_id);
    assert_eq!(selection[0].source_updated_at.timestamp(), 101);

    let (selected_for_phase2, selected_for_phase2_source_updated_at) =
        sqlx::query_as::<_, (i64, Option<i64>)>(
            "SELECT selected_for_phase2, selected_for_phase2_source_updated_at FROM stage1_outputs WHERE thread_id = ?",
        )
    .bind(thread_id.to_string())
    .fetch_one(memory_pool(&runtime))
    .await
    .expect("load selected_for_phase2");
    assert_eq!(selected_for_phase2, 1);
    assert_eq!(selected_for_phase2_source_updated_at, Some(100));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn get_phase2_input_selection_uses_current_ranking_after_refreshes() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id_a = stable_thread_id("00000000-0000-4000-8000-000000000001");
    let thread_id_b = stable_thread_id("00000000-0000-4000-8000-000000000002");
    let thread_id_c = stable_thread_id("00000000-0000-4000-8000-000000000003");
    let thread_id_d = stable_thread_id("00000000-0000-4000-8000-000000000004");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");

    for (thread_id, workspace) in [
        (thread_id_a, "workspace-a"),
        (thread_id_b, "workspace-b"),
        (thread_id_c, "workspace-c"),
        (thread_id_d, "workspace-d"),
    ] {
        runtime
            .upsert_thread(&test_thread_metadata(
                &codex_home,
                thread_id,
                codex_home.join(workspace),
            ))
            .await
            .expect("upsert thread");
    }

    for (thread_id, updated_at, slug) in [
        (thread_id_a, 100, Some("rollout-a-100")),
        (thread_id_b, 101, Some("rollout-b-101")),
        (thread_id_c, 99, Some("rollout-c-99")),
        (thread_id_d, 98, Some("rollout-d-98")),
    ] {
        let claim = runtime
            .try_claim_stage1_job(
                thread_id, owner, updated_at, /*lease_seconds*/ 3600,
                /*max_running_jobs*/ 64,
            )
            .await
            .expect("claim initial stage1");
        let ownership_token = match claim {
            Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
            other => panic!("unexpected stage1 claim outcome: {other:?}"),
        };
        assert!(
            runtime
                .mark_stage1_job_succeeded(
                    thread_id,
                    ownership_token.as_str(),
                    updated_at,
                    &format!("raw-{updated_at}"),
                    &format!("summary-{updated_at}"),
                    slug,
                )
                .await
                .expect("mark stage1 succeeded"),
            "stage1 success should persist output"
        );
    }

    let phase2_claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2");
    let (phase2_token, input_watermark) = match phase2_claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected phase2 claim outcome: {other:?}"),
    };
    let selected_outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 2)
        .await
        .expect("list selected outputs");
    assert_eq!(
        selected_outputs
            .iter()
            .map(|output| output.thread_id)
            .collect::<Vec<_>>(),
        vec![thread_id_b, thread_id_a]
    );
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                phase2_token.as_str(),
                input_watermark,
                &selected_outputs,
            )
            .await
            .expect("mark phase2 success"),
        "phase2 success should persist selected rows"
    );

    for (thread_id, updated_at, slug) in [
        (thread_id_a, 102, Some("rollout-a-102")),
        (thread_id_c, 103, Some("rollout-c-103")),
        (thread_id_d, 104, Some("rollout-d-104")),
    ] {
        let claim = runtime
            .try_claim_stage1_job(
                thread_id, owner, updated_at, /*lease_seconds*/ 3600,
                /*max_running_jobs*/ 64,
            )
            .await
            .expect("claim refreshed stage1");
        let ownership_token = match claim {
            Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
            other => panic!("unexpected stage1 claim outcome: {other:?}"),
        };
        assert!(
            runtime
                .mark_stage1_job_succeeded(
                    thread_id,
                    ownership_token.as_str(),
                    updated_at,
                    &format!("raw-{updated_at}"),
                    &format!("summary-{updated_at}"),
                    slug,
                )
                .await
                .expect("mark refreshed stage1 success"),
            "refreshed stage1 success should persist output"
        );
    }

    let selection = runtime
        .get_phase2_input_selection(/*n*/ 2, /*max_unused_days*/ 36_500)
        .await
        .expect("load phase2 input selection");
    assert_eq!(
        selection
            .iter()
            .map(|output| output.thread_id)
            .collect::<Vec<_>>(),
        vec![thread_id_c, thread_id_d]
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn mark_global_phase2_job_succeeded_updates_selected_snapshot_timestamp() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.join("workspace"),
        ))
        .await
        .expect("upsert thread");

    let initial_claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim initial stage1");
    let initial_token = match initial_claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id,
                initial_token.as_str(),
                /*source_updated_at*/ 100,
                "raw-100",
                "summary-100",
                Some("rollout-100"),
            )
            .await
            .expect("mark initial stage1 success"),
        "initial stage1 success should persist output"
    );

    let first_phase2_claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim first phase2");
    let (first_phase2_token, first_input_watermark) = match first_phase2_claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected first phase2 claim outcome: {other:?}"),
    };
    let first_selected_outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 1)
        .await
        .expect("list first selected outputs");
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                first_phase2_token.as_str(),
                first_input_watermark,
                &first_selected_outputs,
            )
            .await
            .expect("mark first phase2 success"),
        "first phase2 success should persist selected rows"
    );

    let refreshed_claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 101, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim refreshed stage1");
    let refreshed_token = match refreshed_claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected refreshed stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id,
                refreshed_token.as_str(),
                /*source_updated_at*/ 101,
                "raw-101",
                "summary-101",
                Some("rollout-101"),
            )
            .await
            .expect("mark refreshed stage1 success"),
        "refreshed stage1 success should persist output"
    );

    age_phase2_success_beyond_cooldown(&runtime).await;
    let second_phase2_claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim second phase2");
    let (second_phase2_token, second_input_watermark) = match second_phase2_claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected second phase2 claim outcome: {other:?}"),
    };
    let second_selected_outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 1)
        .await
        .expect("list second selected outputs");
    assert_eq!(
        second_selected_outputs[0].source_updated_at.timestamp(),
        101
    );
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                second_phase2_token.as_str(),
                second_input_watermark,
                &second_selected_outputs,
            )
            .await
            .expect("mark second phase2 success"),
        "second phase2 success should persist selected rows"
    );

    let selection = runtime
        .get_phase2_input_selection(/*n*/ 1, /*max_unused_days*/ 36_500)
        .await
        .expect("load phase2 input selection after refresh");
    assert_eq!(selection.len(), 1);
    assert_eq!(selection[0].thread_id, thread_id);

    let (selected_for_phase2, selected_for_phase2_source_updated_at) =
        sqlx::query_as::<_, (i64, Option<i64>)>(
            "SELECT selected_for_phase2, selected_for_phase2_source_updated_at FROM stage1_outputs WHERE thread_id = ?",
        )
        .bind(thread_id.to_string())
        .fetch_one(memory_pool(&runtime))
        .await
        .expect("load selected snapshot after phase2");
    assert_eq!(selected_for_phase2, 1);
    assert_eq!(selected_for_phase2_source_updated_at, Some(101));

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn mark_global_phase2_job_succeeded_only_marks_exact_selected_snapshots() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_id = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_id,
            codex_home.join("workspace"),
        ))
        .await
        .expect("upsert thread");

    let initial_claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim initial stage1");
    let initial_token = match initial_claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id,
                initial_token.as_str(),
                /*source_updated_at*/ 100,
                "raw-100",
                "summary-100",
                Some("rollout-100"),
            )
            .await
            .expect("mark initial stage1 success"),
        "initial stage1 success should persist output"
    );

    let phase2_claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim phase2");
    let (phase2_token, input_watermark) = match phase2_claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected phase2 claim outcome: {other:?}"),
    };
    let selected_outputs = runtime
        .list_stage1_outputs_for_global(/*n*/ 1)
        .await
        .expect("list selected outputs");
    assert_eq!(selected_outputs[0].source_updated_at.timestamp(), 100);

    let refreshed_claim = runtime
        .try_claim_stage1_job(
            thread_id, owner, /*source_updated_at*/ 101, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim refreshed stage1");
    let refreshed_token = match refreshed_claim {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_id,
                refreshed_token.as_str(),
                /*source_updated_at*/ 101,
                "raw-101",
                "summary-101",
                Some("rollout-101"),
            )
            .await
            .expect("mark refreshed stage1 success"),
        "refreshed stage1 success should persist output"
    );

    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                phase2_token.as_str(),
                input_watermark,
                &selected_outputs,
            )
            .await
            .expect("mark phase2 success"),
        "phase2 success should still complete"
    );

    let (selected_for_phase2, selected_for_phase2_source_updated_at) =
        sqlx::query_as::<_, (i64, Option<i64>)>(
            "SELECT selected_for_phase2, selected_for_phase2_source_updated_at FROM stage1_outputs WHERE thread_id = ?",
        )
        .bind(thread_id.to_string())
        .fetch_one(memory_pool(&runtime))
        .await
        .expect("load selected_for_phase2");
    assert_eq!(selected_for_phase2, 0);
    assert_eq!(selected_for_phase2_source_updated_at, None);

    let selection = runtime
        .get_phase2_input_selection(/*n*/ 1, /*max_unused_days*/ 36_500)
        .await
        .expect("load phase2 input selection");
    assert_eq!(selection.len(), 1);
    assert_eq!(selection[0].source_updated_at.timestamp(), 101);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn record_stage1_output_usage_updates_usage_metadata() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id a");
    let thread_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id b");
    let missing = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("missing id");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");

    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_a,
            codex_home.join("workspace-a"),
        ))
        .await
        .expect("upsert thread a");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_b,
            codex_home.join("workspace-b"),
        ))
        .await
        .expect("upsert thread b");

    let claim_a = runtime
        .try_claim_stage1_job(
            thread_a, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 a");
    let token_a = match claim_a {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome for a: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_a,
                token_a.as_str(),
                /*source_updated_at*/ 100,
                "raw a",
                "sum a",
                /*rollout_slug*/ None
            )
            .await
            .expect("mark stage1 succeeded a")
    );

    let claim_b = runtime
        .try_claim_stage1_job(
            thread_b, owner, /*source_updated_at*/ 101, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 b");
    let token_b = match claim_b {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome for b: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_b,
                token_b.as_str(),
                /*source_updated_at*/ 101,
                "raw b",
                "sum b",
                /*rollout_slug*/ None
            )
            .await
            .expect("mark stage1 succeeded b")
    );

    let updated_rows = runtime
        .record_stage1_output_usage(&[thread_a, thread_a, thread_b, missing])
        .await
        .expect("record stage1 output usage");
    assert_eq!(updated_rows, 3);

    let row_a =
        sqlx::query("SELECT usage_count, last_usage FROM stage1_outputs WHERE thread_id = ?")
            .bind(thread_a.to_string())
            .fetch_one(memory_pool(&runtime))
            .await
            .expect("load stage1 usage row a");
    let row_b =
        sqlx::query("SELECT usage_count, last_usage FROM stage1_outputs WHERE thread_id = ?")
            .bind(thread_b.to_string())
            .fetch_one(memory_pool(&runtime))
            .await
            .expect("load stage1 usage row b");

    assert_eq!(
        row_a
            .try_get::<i64, _>("usage_count")
            .expect("usage_count a"),
        2
    );
    assert_eq!(
        row_b
            .try_get::<i64, _>("usage_count")
            .expect("usage_count b"),
        1
    );

    let last_usage_a = row_a.try_get::<i64, _>("last_usage").expect("last_usage a");
    let last_usage_b = row_b.try_get::<i64, _>("last_usage").expect("last_usage b");
    assert_eq!(last_usage_a, last_usage_b);
    assert!(last_usage_a > 0);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn get_phase2_input_selection_prioritizes_usage_count_then_recent_usage() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let now = Utc::now();
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let thread_a = stable_thread_id("00000000-0000-4000-8000-000000000001");
    let thread_b = stable_thread_id("00000000-0000-4000-8000-000000000002");
    let thread_c = stable_thread_id("00000000-0000-4000-8000-000000000003");

    for (thread_id, workspace) in [
        (thread_a, "workspace-a"),
        (thread_b, "workspace-b"),
        (thread_c, "workspace-c"),
    ] {
        runtime
            .upsert_thread(&test_thread_metadata(
                &codex_home,
                thread_id,
                codex_home.join(workspace),
            ))
            .await
            .expect("upsert thread");
    }

    for (thread_id, generated_at, summary) in [
        (thread_a, now - Duration::days(3), "summary-a"),
        (thread_b, now - Duration::days(2), "summary-b"),
        (thread_c, now - Duration::days(1), "summary-c"),
    ] {
        let source_updated_at = generated_at.timestamp();
        let claim = runtime
            .try_claim_stage1_job(
                thread_id,
                owner,
                source_updated_at,
                /*lease_seconds*/ 3600,
                /*max_running_jobs*/ 64,
            )
            .await
            .expect("claim stage1");
        let ownership_token = match claim {
            Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
            other => panic!("unexpected stage1 claim outcome: {other:?}"),
        };
        assert!(
            runtime
                .mark_stage1_job_succeeded(
                    thread_id,
                    ownership_token.as_str(),
                    source_updated_at,
                    &format!("raw-{summary}"),
                    summary,
                    /*rollout_slug*/ None,
                )
                .await
                .expect("mark stage1 success"),
            "stage1 success should persist output"
        );
    }

    for (thread_id, usage_count, last_usage) in [
        (thread_a, 5_i64, now - Duration::days(10)),
        (thread_b, 5_i64, now - Duration::days(1)),
        (thread_c, 1_i64, now - Duration::hours(1)),
    ] {
        sqlx::query(
            "UPDATE stage1_outputs SET usage_count = ?, last_usage = ? WHERE thread_id = ?",
        )
        .bind(usage_count)
        .bind(last_usage.timestamp())
        .bind(thread_id.to_string())
        .execute(memory_pool(&runtime))
        .await
        .expect("update usage metadata");
    }

    let selection = runtime
        .get_phase2_input_selection(/*n*/ 1, /*max_unused_days*/ 30)
        .await
        .expect("load phase2 input selection");

    assert_eq!(
        selection
            .iter()
            .map(|output| output.thread_id)
            .collect::<Vec<_>>(),
        vec![thread_b]
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn get_phase2_input_selection_excludes_stale_used_memories_but_keeps_fresh_never_used() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let now = Utc::now();
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let thread_a = stable_thread_id("00000000-0000-4000-8000-000000000001");
    let thread_b = stable_thread_id("00000000-0000-4000-8000-000000000002");
    let thread_c = stable_thread_id("00000000-0000-4000-8000-000000000003");

    for (thread_id, workspace) in [
        (thread_a, "workspace-a"),
        (thread_b, "workspace-b"),
        (thread_c, "workspace-c"),
    ] {
        runtime
            .upsert_thread(&test_thread_metadata(
                &codex_home,
                thread_id,
                codex_home.join(workspace),
            ))
            .await
            .expect("upsert thread");
    }

    for (thread_id, generated_at, summary) in [
        (thread_a, now - Duration::days(40), "summary-a"),
        (thread_b, now - Duration::days(2), "summary-b"),
        (thread_c, now - Duration::days(50), "summary-c"),
    ] {
        let source_updated_at = generated_at.timestamp();
        let claim = runtime
            .try_claim_stage1_job(
                thread_id,
                owner,
                source_updated_at,
                /*lease_seconds*/ 3600,
                /*max_running_jobs*/ 64,
            )
            .await
            .expect("claim stage1");
        let ownership_token = match claim {
            Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
            other => panic!("unexpected stage1 claim outcome: {other:?}"),
        };
        assert!(
            runtime
                .mark_stage1_job_succeeded(
                    thread_id,
                    ownership_token.as_str(),
                    source_updated_at,
                    &format!("raw-{summary}"),
                    summary,
                    /*rollout_slug*/ None,
                )
                .await
                .expect("mark stage1 success"),
            "stage1 success should persist output"
        );
    }

    for (thread_id, usage_count, last_usage) in [
        (thread_a, Some(9_i64), Some(now - Duration::days(31))),
        (thread_b, None, None),
        (thread_c, Some(1_i64), Some(now - Duration::days(1))),
    ] {
        sqlx::query(
            "UPDATE stage1_outputs SET usage_count = ?, last_usage = ? WHERE thread_id = ?",
        )
        .bind(usage_count)
        .bind(last_usage.map(|value| value.timestamp()))
        .bind(thread_id.to_string())
        .execute(memory_pool(&runtime))
        .await
        .expect("update usage metadata");
    }

    let selection = runtime
        .get_phase2_input_selection(/*n*/ 3, /*max_unused_days*/ 30)
        .await
        .expect("load phase2 input selection");

    assert_eq!(
        selection
            .iter()
            .map(|output| output.thread_id)
            .collect::<Vec<_>>(),
        vec![thread_b, thread_c]
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn get_phase2_input_selection_prefers_recent_thread_updates_over_recent_generation() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let older_thread = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("older thread id");
    let newer_thread = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("newer thread id");

    for (thread_id, workspace) in [
        (older_thread, "workspace-older"),
        (newer_thread, "workspace-newer"),
    ] {
        runtime
            .upsert_thread(&test_thread_metadata(
                &codex_home,
                thread_id,
                codex_home.join(workspace),
            ))
            .await
            .expect("upsert thread");
    }

    for (thread_id, source_updated_at, summary) in [
        (older_thread, 100_i64, "summary-older"),
        (newer_thread, 200_i64, "summary-newer"),
    ] {
        let claim = runtime
            .try_claim_stage1_job(
                thread_id,
                owner,
                source_updated_at,
                /*lease_seconds*/ 3600,
                /*max_running_jobs*/ 64,
            )
            .await
            .expect("claim stage1");
        let ownership_token = match claim {
            Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
            other => panic!("unexpected stage1 claim outcome: {other:?}"),
        };
        assert!(
            runtime
                .mark_stage1_job_succeeded(
                    thread_id,
                    ownership_token.as_str(),
                    source_updated_at,
                    &format!("raw-{summary}"),
                    summary,
                    /*rollout_slug*/ None,
                )
                .await
                .expect("mark stage1 success"),
            "stage1 success should persist output"
        );
    }

    sqlx::query("UPDATE stage1_outputs SET generated_at = ? WHERE thread_id = ?")
        .bind(300_i64)
        .bind(older_thread.to_string())
        .execute(memory_pool(&runtime))
        .await
        .expect("update older generated_at");
    sqlx::query("UPDATE stage1_outputs SET generated_at = ? WHERE thread_id = ?")
        .bind(150_i64)
        .bind(newer_thread.to_string())
        .execute(memory_pool(&runtime))
        .await
        .expect("update newer generated_at");

    let selection = runtime
        .get_phase2_input_selection(/*n*/ 1, /*max_unused_days*/ 36_500)
        .await
        .expect("load phase2 input selection");

    assert_eq!(selection.len(), 1);
    assert_eq!(selection[0].thread_id, newer_thread);
    assert_eq!(selection[0].source_updated_at.timestamp(), 200);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn prune_stage1_outputs_for_retention_prunes_stale_unselected_rows_only() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let stale_unused = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("stale unused");
    let stale_used = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("stale used");
    let stale_selected =
        ThreadId::from_string(&Uuid::new_v4().to_string()).expect("stale selected");
    let fresh_used = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("fresh used");

    for (thread_id, workspace) in [
        (stale_unused, "workspace-stale-unused"),
        (stale_used, "workspace-stale-used"),
        (stale_selected, "workspace-stale-selected"),
        (fresh_used, "workspace-fresh-used"),
    ] {
        runtime
            .upsert_thread(&test_thread_metadata(
                &codex_home,
                thread_id,
                codex_home.join(workspace),
            ))
            .await
            .expect("upsert thread");
    }

    let now = Utc::now().timestamp();
    for (thread_id, source_updated_at, summary) in [
        (
            stale_unused,
            now - Duration::days(60).num_seconds(),
            "stale-unused",
        ),
        (
            stale_used,
            now - Duration::days(50).num_seconds(),
            "stale-used",
        ),
        (
            stale_selected,
            now - Duration::days(45).num_seconds(),
            "stale-selected",
        ),
        (
            fresh_used,
            now - Duration::days(10).num_seconds(),
            "fresh-used",
        ),
    ] {
        let claim = runtime
            .try_claim_stage1_job(
                thread_id,
                owner,
                source_updated_at,
                /*lease_seconds*/ 3600,
                /*max_running_jobs*/ 64,
            )
            .await
            .expect("claim stage1");
        let ownership_token = match claim {
            Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
            other => panic!("unexpected stage1 claim outcome: {other:?}"),
        };
        assert!(
            runtime
                .mark_stage1_job_succeeded(
                    thread_id,
                    ownership_token.as_str(),
                    source_updated_at,
                    &format!("raw-{summary}"),
                    summary,
                    /*rollout_slug*/ None,
                )
                .await
                .expect("mark stage1 success"),
            "stage1 success should persist output"
        );
    }

    sqlx::query("UPDATE stage1_outputs SET usage_count = ?, last_usage = ? WHERE thread_id = ?")
        .bind(3_i64)
        .bind(now - Duration::days(40).num_seconds())
        .bind(stale_used.to_string())
        .execute(memory_pool(&runtime))
        .await
        .expect("set stale used metadata");
    sqlx::query(
        "UPDATE stage1_outputs SET selected_for_phase2 = 1, selected_for_phase2_source_updated_at = source_updated_at WHERE thread_id = ?",
    )
    .bind(stale_selected.to_string())
    .execute(memory_pool(&runtime))
    .await
    .expect("mark selected for phase2");
    sqlx::query("UPDATE stage1_outputs SET usage_count = ?, last_usage = ? WHERE thread_id = ?")
        .bind(8_i64)
        .bind(now - Duration::days(2).num_seconds())
        .bind(fresh_used.to_string())
        .execute(memory_pool(&runtime))
        .await
        .expect("set fresh used metadata");

    let before_jobs_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM jobs WHERE kind = 'memory_stage1'")
            .fetch_one(memory_pool(&runtime))
            .await
            .expect("count stage1 jobs before prune");

    let pruned = runtime
        .prune_stage1_outputs_for_retention(/*max_unused_days*/ 30, /*limit*/ 100)
        .await
        .expect("prune stage1 outputs");
    assert_eq!(pruned, 2);

    let remaining =
        sqlx::query_scalar::<_, String>("SELECT thread_id FROM stage1_outputs ORDER BY thread_id")
            .fetch_all(memory_pool(&runtime))
            .await
            .expect("load remaining stage1 outputs");
    let mut expected_remaining = vec![fresh_used.to_string(), stale_selected.to_string()];
    expected_remaining.sort();
    assert_eq!(remaining, expected_remaining);

    let after_jobs_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM jobs WHERE kind = 'memory_stage1'")
            .fetch_one(memory_pool(&runtime))
            .await
            .expect("count stage1 jobs after prune");
    assert_eq!(after_jobs_count, before_jobs_count);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn prune_stage1_outputs_for_retention_respects_batch_limit() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");
    let thread_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread a");
    let thread_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread b");
    let thread_c = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread c");

    for (thread_id, workspace) in [
        (thread_a, "workspace-a"),
        (thread_b, "workspace-b"),
        (thread_c, "workspace-c"),
    ] {
        runtime
            .upsert_thread(&test_thread_metadata(
                &codex_home,
                thread_id,
                codex_home.join(workspace),
            ))
            .await
            .expect("upsert thread");
    }

    let now = Utc::now().timestamp();
    for (thread_id, source_updated_at, summary) in [
        (thread_a, now - Duration::days(60).num_seconds(), "stale-a"),
        (thread_b, now - Duration::days(50).num_seconds(), "stale-b"),
        (thread_c, now - Duration::days(40).num_seconds(), "stale-c"),
    ] {
        let claim = runtime
            .try_claim_stage1_job(
                thread_id,
                owner,
                source_updated_at,
                /*lease_seconds*/ 3600,
                /*max_running_jobs*/ 64,
            )
            .await
            .expect("claim stage1");
        let ownership_token = match claim {
            Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
            other => panic!("unexpected stage1 claim outcome: {other:?}"),
        };
        assert!(
            runtime
                .mark_stage1_job_succeeded(
                    thread_id,
                    ownership_token.as_str(),
                    source_updated_at,
                    &format!("raw-{summary}"),
                    summary,
                    /*rollout_slug*/ None,
                )
                .await
                .expect("mark stage1 success"),
            "stage1 success should persist output"
        );
    }

    let pruned = runtime
        .prune_stage1_outputs_for_retention(/*max_unused_days*/ 30, /*limit*/ 2)
        .await
        .expect("prune stage1 outputs with limit");
    assert_eq!(pruned, 2);

    let remaining_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stage1_outputs")
        .fetch_one(memory_pool(&runtime))
        .await
        .expect("count remaining stage1 outputs");
    assert_eq!(remaining_count, 1);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn mark_stage1_job_succeeded_enqueues_global_consolidation() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let thread_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id a");
    let thread_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("thread id b");
    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner id");

    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_a,
            codex_home.join("workspace-a"),
        ))
        .await
        .expect("upsert thread a");
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            thread_b,
            codex_home.join("workspace-b"),
        ))
        .await
        .expect("upsert thread b");

    let claim_a = runtime
        .try_claim_stage1_job(
            thread_a, owner, /*source_updated_at*/ 100, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 a");
    let token_a = match claim_a {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome for thread a: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_a,
                token_a.as_str(),
                /*source_updated_at*/ 100,
                "raw-a",
                "summary-a",
                /*rollout_slug*/ None,
            )
            .await
            .expect("mark stage1 succeeded a"),
        "stage1 success should persist output for thread a"
    );

    let claim_b = runtime
        .try_claim_stage1_job(
            thread_b, owner, /*source_updated_at*/ 101, /*lease_seconds*/ 3600,
            /*max_running_jobs*/ 64,
        )
        .await
        .expect("claim stage1 b");
    let token_b = match claim_b {
        Stage1JobClaimOutcome::Claimed { ownership_token } => ownership_token,
        other => panic!("unexpected stage1 claim outcome for thread b: {other:?}"),
    };
    assert!(
        runtime
            .mark_stage1_job_succeeded(
                thread_b,
                token_b.as_str(),
                /*source_updated_at*/ 101,
                "raw-b",
                "summary-b",
                /*rollout_slug*/ None,
            )
            .await
            .expect("mark stage1 succeeded b"),
        "stage1 success should persist output for thread b"
    );

    let claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3600)
        .await
        .expect("claim global consolidation");
    let input_watermark = match claim {
        Phase2JobClaimOutcome::Claimed {
            input_watermark, ..
        } => input_watermark,
        other => panic!("unexpected global consolidation claim outcome: {other:?}"),
    };
    assert_eq!(input_watermark, 101);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn phase2_global_lock_allows_only_one_fresh_runner() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .enqueue_global_consolidation(/*input_watermark*/ 200)
        .await
        .expect("enqueue global consolidation");

    let owner_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner a");
    let owner_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner b");

    let running_claim = runtime
        .try_claim_global_phase2_job(owner_a, /*lease_seconds*/ 3600)
        .await
        .expect("claim global lock");
    assert!(
        matches!(running_claim, Phase2JobClaimOutcome::Claimed { .. }),
        "first owner should claim global lock"
    );

    let second_claim = runtime
        .try_claim_global_phase2_job(owner_b, /*lease_seconds*/ 3600)
        .await
        .expect("claim global lock from second owner");
    assert_eq!(second_claim, Phase2JobClaimOutcome::SkippedRunning);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn phase2_global_lock_creates_missing_job_row() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    let owner_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner a");
    let owner_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner b");

    let claim = runtime
        .try_claim_global_phase2_job(owner_a, /*lease_seconds*/ 3_600)
        .await
        .expect("claim global phase2 lock");
    let ownership_token = match claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => {
            assert_eq!(input_watermark, 0);
            ownership_token
        }
        other => panic!("unexpected phase2 lock claim outcome: {other:?}"),
    };

    let second_claim = runtime
        .try_claim_global_phase2_job(owner_b, /*lease_seconds*/ 3_600)
        .await
        .expect("claim global phase2 lock from second owner");
    assert_eq!(second_claim, Phase2JobClaimOutcome::SkippedRunning);

    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                ownership_token.as_str(),
                /*completed_watermark*/ 0,
                &[]
            )
            .await
            .expect("mark phase2 lock success")
    );
    let claim_after_success = runtime
        .try_claim_global_phase2_job(owner_b, /*lease_seconds*/ 3_600)
        .await
        .expect("claim global phase2 lock after success");
    assert_eq!(claim_after_success, Phase2JobClaimOutcome::SkippedCooldown);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn phase2_global_lock_stale_lease_allows_takeover() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .enqueue_global_consolidation(/*input_watermark*/ 300)
        .await
        .expect("enqueue global consolidation");

    let owner_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner a");
    let owner_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner b");

    let initial_claim = runtime
        .try_claim_global_phase2_job(owner_a, /*lease_seconds*/ 3600)
        .await
        .expect("claim initial global lock");
    let token_a = match initial_claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token, ..
        } => ownership_token,
        other => panic!("unexpected initial claim outcome: {other:?}"),
    };

    sqlx::query("UPDATE jobs SET lease_until = ? WHERE kind = ? AND job_key = ?")
        .bind(Utc::now().timestamp() - 1)
        .bind("memory_consolidate_global")
        .bind("global")
        .execute(memory_pool(&runtime))
        .await
        .expect("expire global consolidation lease");

    let takeover_claim = runtime
        .try_claim_global_phase2_job(owner_b, /*lease_seconds*/ 3600)
        .await
        .expect("claim stale global lock");
    let (token_b, input_watermark) = match takeover_claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => (ownership_token, input_watermark),
        other => panic!("unexpected takeover claim outcome: {other:?}"),
    };
    assert_ne!(token_a, token_b);
    assert_eq!(input_watermark, 300);

    assert_eq!(
        runtime
            .mark_global_phase2_job_succeeded(
                token_a.as_str(),
                /*completed_watermark*/ 300,
                &[]
            )
            .await
            .expect("mark stale owner success result"),
        false,
        "stale owner should lose finalization ownership after takeover"
    );
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                token_b.as_str(),
                /*completed_watermark*/ 300,
                &[]
            )
            .await
            .expect("mark takeover owner success"),
        "takeover owner should finalize consolidation"
    );

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn enqueue_global_consolidation_keeps_phase2_input_watermark_monotonic() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .enqueue_global_consolidation(/*input_watermark*/ 500)
        .await
        .expect("enqueue initial consolidation");
    let owner_a = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner a");
    let claim_a = runtime
        .try_claim_global_phase2_job(owner_a, /*lease_seconds*/ 3_600)
        .await
        .expect("claim initial consolidation");
    let token_a = match claim_a {
        Phase2JobClaimOutcome::Claimed {
            ownership_token,
            input_watermark,
        } => {
            assert_eq!(input_watermark, 500);
            ownership_token
        }
        other => panic!("unexpected initial phase2 claim outcome: {other:?}"),
    };
    assert!(
        runtime
            .mark_global_phase2_job_succeeded(
                token_a.as_str(),
                /*completed_watermark*/ 500,
                &[]
            )
            .await
            .expect("mark initial phase2 success"),
        "initial phase2 success should finalize"
    );

    runtime
        .enqueue_global_consolidation(/*input_watermark*/ 400)
        .await
        .expect("enqueue lower-watermark consolidation");

    age_phase2_success_beyond_cooldown(&runtime).await;
    let owner_b = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner b");
    let claim_b = runtime
        .try_claim_global_phase2_job(owner_b, /*lease_seconds*/ 3_600)
        .await
        .expect("claim lower-watermark consolidation");
    match claim_b {
        Phase2JobClaimOutcome::Claimed {
            input_watermark, ..
        } => {
            assert!(
                input_watermark > 500,
                "lower-watermark enqueue should still advance the bookkeeping watermark"
            );
        }
        other => panic!("unexpected lower-watermark phase2 claim outcome: {other:?}"),
    }

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}

#[tokio::test]
async fn phase2_failure_fallback_updates_unowned_running_job() {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string())
        .await
        .expect("initialize runtime");

    runtime
        .enqueue_global_consolidation(/*input_watermark*/ 400)
        .await
        .expect("enqueue global consolidation");

    let owner = ThreadId::from_string(&Uuid::new_v4().to_string()).expect("owner");
    let claim = runtime
        .try_claim_global_phase2_job(owner, /*lease_seconds*/ 3_600)
        .await
        .expect("claim global consolidation");
    let ownership_token = match claim {
        Phase2JobClaimOutcome::Claimed {
            ownership_token, ..
        } => ownership_token,
        other => panic!("unexpected claim outcome: {other:?}"),
    };

    sqlx::query("UPDATE jobs SET ownership_token = NULL WHERE kind = ? AND job_key = ?")
        .bind("memory_consolidate_global")
        .bind("global")
        .execute(memory_pool(&runtime))
        .await
        .expect("clear ownership token");

    assert_eq!(
        runtime
            .mark_global_phase2_job_failed(
                ownership_token.as_str(),
                "lost",
                /*retry_delay_seconds*/ 3_600
            )
            .await
            .expect("mark phase2 failed with strict ownership"),
        false,
        "strict failure update should not match unowned running job"
    );
    assert!(
        runtime
            .mark_global_phase2_job_failed_if_unowned(
                ownership_token.as_str(),
                "lost",
                /*retry_delay_seconds*/ 3_600
            )
            .await
            .expect("fallback failure update should match unowned running job"),
        "fallback failure update should transition the unowned running job"
    );

    let claim = runtime
        .try_claim_global_phase2_job(ThreadId::new(), /*lease_seconds*/ 3_600)
        .await
        .expect("claim after fallback failure");
    assert_eq!(claim, Phase2JobClaimOutcome::SkippedRetryUnavailable);

    let _ = tokio::fs::remove_dir_all(codex_home).await;
}
