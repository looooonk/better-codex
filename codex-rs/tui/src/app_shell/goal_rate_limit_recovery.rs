use super::ShellState;
use super::backend::AppShellBackend;
use codex_app_server_protocol::GetAccountRateLimitsResponse;
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_protocol::ThreadId;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const GOAL_RATE_LIMIT_RECOVERY_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 5);

#[derive(Default)]
pub(super) struct GoalRateLimitRecoveryState {
    generation: u64,
    resume_after_refresh: bool,
    task: Option<JoinHandle<GoalRateLimitRecoveryLookup>>,
}

struct GoalRateLimitRecoveryLookup {
    generation: u64,
    thread_id: ThreadId,
    result: Result<ThreadGoal, String>,
}

impl ShellState {
    pub(super) fn goal_status_changed_for_rate_limit_recovery(&mut self) {
        self.cancel_goal_rate_limit_recovery();
        if self
            .active_goal
            .as_ref()
            .is_some_and(|goal| goal.status == ThreadGoalStatus::UsageLimited)
        {
            self.request_rate_limits_refresh();
        }
    }

    pub(super) fn rate_limits_refreshed_for_goal_recovery(
        &mut self,
        response: &GetAccountRateLimitsResponse,
    ) {
        self.goal_rate_limit_recovery.resume_after_refresh = self
            .active_goal
            .as_ref()
            .is_some_and(|goal| goal.status == ThreadGoalStatus::UsageLimited)
            && rate_limits_allow_goal_resume(response, &self.model);
    }

    pub(super) fn maybe_start_goal_rate_limit_recovery<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        if !self.goal_rate_limit_recovery.resume_after_refresh
            || self.goal_rate_limit_recovery.task.is_some()
            || self.active_turn_id.is_some()
            || !self
                .active_goal
                .as_ref()
                .is_some_and(|goal| goal.status == ThreadGoalStatus::UsageLimited)
        {
            return;
        }

        self.goal_rate_limit_recovery.resume_after_refresh = false;
        let generation = self.goal_rate_limit_recovery.generation;
        let thread_id = self.thread_id;
        let resume = app_server.resume_usage_limited_goal_in_background(thread_id);
        self.goal_rate_limit_recovery.task = Some(tokio::spawn(async move {
            let result = match timeout(GOAL_RATE_LIMIT_RECOVERY_TIMEOUT, resume).await {
                Ok(Ok(response)) => Ok(response.goal),
                Ok(Err(err)) => Err(err.to_string()),
                Err(_) => Err("usage-limited goal recovery timed out".to_string()),
            };
            GoalRateLimitRecoveryLookup {
                generation,
                thread_id,
                result,
            }
        }));
    }

    pub(super) async fn poll_goal_rate_limit_recovery(&mut self) -> bool {
        if !self
            .goal_rate_limit_recovery
            .task
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
            return false;
        }
        let Some(task) = self.goal_rate_limit_recovery.task.take() else {
            return false;
        };
        let lookup = match task.await {
            Ok(lookup) => lookup,
            Err(err) => {
                if !err.is_cancelled() {
                    tracing::warn!(%err, "goal rate-limit recovery task failed");
                }
                return false;
            }
        };
        if lookup.generation != self.goal_rate_limit_recovery.generation
            || lookup.thread_id != self.thread_id
            || !self
                .active_goal
                .as_ref()
                .is_some_and(|goal| goal.status == ThreadGoalStatus::UsageLimited)
        {
            return false;
        }
        match lookup.result {
            Ok(goal) => {
                self.record_active_goal(Some(goal));
                true
            }
            Err(err) => {
                tracing::warn!(%err, "failed to resume usage-limited goal after rate-limit reset");
                false
            }
        }
    }

    pub(super) fn has_pending_goal_rate_limit_recovery(&self) -> bool {
        self.goal_rate_limit_recovery.task.is_some()
    }

    pub(super) fn cancel_goal_rate_limit_recovery(&mut self) {
        self.goal_rate_limit_recovery.generation =
            self.goal_rate_limit_recovery.generation.wrapping_add(1);
        self.goal_rate_limit_recovery.resume_after_refresh = false;
        if let Some(task) = self.goal_rate_limit_recovery.task.take() {
            task.abort();
        }
    }
}

fn rate_limits_allow_goal_resume(response: &GetAccountRateLimitsResponse, model: &str) -> bool {
    rate_limit_snapshot_allows_goal_resume(&response.rate_limits)
        && response
            .rate_limits_by_limit_id
            .as_ref()
            .and_then(|limits| limits.get(model))
            .is_none_or(rate_limit_snapshot_allows_goal_resume)
}

fn rate_limit_snapshot_allows_goal_resume(limits: &RateLimitSnapshot) -> bool {
    if limits.rate_limit_reached_type.is_some()
        || limits
            .credits
            .as_ref()
            .is_some_and(|credits| !credits.unlimited && !credits.has_credits)
        || limits
            .individual_limit
            .as_ref()
            .is_some_and(|limit| limit.remaining_percent <= 0)
    {
        return false;
    }

    let windows = [limits.primary.as_ref(), limits.secondary.as_ref()];
    windows.iter().flatten().next().is_some()
        && windows
            .into_iter()
            .flatten()
            .all(|window| window.used_percent < 100)
}

#[cfg(test)]
#[path = "goal_rate_limit_recovery_tests.rs"]
mod tests;
