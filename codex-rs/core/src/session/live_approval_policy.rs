use crate::config::Constrained;
#[cfg(test)]
use crate::config::ConstraintResult;
use codex_protocol::protocol::AskForApproval;
use std::sync::Arc;
use std::sync::PoisonError;
use std::sync::RwLock;

/// A turn's approval policy that can be replaced while the turn is running.
#[derive(Clone, Debug)]
pub(crate) struct LiveApprovalPolicy {
    inner: Arc<RwLock<Constrained<AskForApproval>>>,
}

impl LiveApprovalPolicy {
    pub(crate) fn new(approval_policy: Constrained<AskForApproval>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(approval_policy)),
        }
    }

    pub(crate) fn value(&self) -> AskForApproval {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .value()
    }

    pub(crate) fn snapshot(&self) -> Constrained<AskForApproval> {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn set(&self, approval_policy: AskForApproval) -> ConstraintResult<()> {
        self.inner
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .set(approval_policy)
    }

    pub(crate) fn replace(&self, approval_policy: Constrained<AskForApproval>) {
        *self.inner.write().unwrap_or_else(PoisonError::into_inner) = approval_policy;
    }
}
