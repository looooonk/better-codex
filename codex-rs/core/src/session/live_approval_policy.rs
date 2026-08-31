use crate::config::Constrained;
#[cfg(test)]
use crate::config::ConstraintResult;
use codex_config::types::ApprovalsReviewer;
use codex_protocol::protocol::AskForApproval;
use std::sync::Arc;
use std::sync::PoisonError;
use std::sync::RwLock;

/// A turn's approval policy that can be replaced while the turn is running.
#[derive(Clone, Debug)]
pub(crate) struct LiveApprovalPolicy {
    inner: Arc<RwLock<LiveApprovalPolicyState>>,
}

#[derive(Clone, Debug)]
struct LiveApprovalPolicyState {
    policy: Constrained<AskForApproval>,
    revision: u64,
    never_revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LiveApprovalPolicySnapshot {
    pub(crate) value: AskForApproval,
    pub(crate) revision: u64,
    pub(crate) never_revision: u64,
}

impl LiveApprovalPolicy {
    pub(crate) fn new(approval_policy: Constrained<AskForApproval>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(LiveApprovalPolicyState {
                policy: approval_policy,
                revision: 0,
                never_revision: 0,
            })),
        }
    }

    pub(crate) fn value(&self) -> AskForApproval {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .policy
            .value()
    }

    pub(crate) fn snapshot(&self) -> Constrained<AskForApproval> {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .policy
            .clone()
    }

    pub(crate) fn decision_snapshot(&self) -> LiveApprovalPolicySnapshot {
        let state = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        LiveApprovalPolicySnapshot {
            value: state.policy.value(),
            revision: state.revision,
            never_revision: state.never_revision,
        }
    }

    #[cfg(test)]
    pub(crate) fn set(&self, approval_policy: AskForApproval) -> ConstraintResult<()> {
        let mut state = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        state.policy.set(approval_policy)?;
        state.revision = state.revision.wrapping_add(1);
        if approval_policy == AskForApproval::Never {
            state.never_revision = state.never_revision.wrapping_add(1);
        }
        Ok(())
    }

    pub(crate) fn replace(&self, approval_policy: Constrained<AskForApproval>) {
        let mut state = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        if state.policy == approval_policy {
            return;
        }
        state.revision = state.revision.wrapping_add(1);
        if approval_policy.value() == AskForApproval::Never {
            state.never_revision = state.never_revision.wrapping_add(1);
        }
        state.policy = approval_policy;
    }
}

/// A turn's reviewer selection that can be replaced while the turn is running.
#[derive(Clone, Debug)]
pub(crate) struct LiveApprovalsReviewer {
    inner: Arc<RwLock<(ApprovalsReviewer, u64)>>,
}

impl LiveApprovalsReviewer {
    pub(crate) fn new(reviewer: ApprovalsReviewer) -> Self {
        Self {
            inner: Arc::new(RwLock::new((reviewer, 0))),
        }
    }

    pub(crate) fn snapshot(&self) -> (ApprovalsReviewer, u64) {
        *self.inner.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn replace(&self, reviewer: ApprovalsReviewer) {
        let mut state = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        if state.0 != reviewer {
            *state = (reviewer, state.1.wrapping_add(1));
        }
    }
}

#[cfg(test)]
#[path = "live_approval_policy_tests.rs"]
mod tests;
