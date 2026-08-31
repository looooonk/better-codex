use super::McpToolApprovalMetadata;
use crate::connectors;
use crate::session::turn_context::TurnContext;
use codex_config::types::AppToolApproval;
use codex_config::types::ApprovalsReviewer;
use codex_mcp::McpConnectionManager;
use codex_protocol::protocol::AskForApproval;
use serde::Serialize;

const MAX_ROUTE_SNAPSHOT_ATTEMPTS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(super) struct McpApprovalRouteSnapshot {
    pub(super) catalog_revision: u64,
    pub(super) approval_policy: AskForApproval,
    pub(super) policy_revision: u64,
    pub(super) never_revision: u64,
    pub(super) approvals_reviewer: ApprovalsReviewer,
    pub(super) reviewer_revision: u64,
    pub(super) approval_mode: AppToolApproval,
}

impl McpApprovalRouteSnapshot {
    pub(super) async fn capture(
        turn_context: &TurnContext,
        manager: &McpConnectionManager,
        server: &str,
        metadata: Option<&McpToolApprovalMetadata>,
        approval_mode: AppToolApproval,
    ) -> Option<Self> {
        for _ in 0..MAX_ROUTE_SNAPSHOT_ATTEMPTS {
            let before = Self::sample(
                turn_context,
                manager,
                server,
                metadata,
                approval_mode,
            )
            .await;
            let after = Self::sample(
                turn_context,
                manager,
                server,
                metadata,
                approval_mode,
            )
            .await;
            if before == after {
                return Some(after);
            }
        }
        None
    }

    pub(super) async fn is_current(
        self,
        turn_context: &TurnContext,
        manager: &McpConnectionManager,
        server: &str,
        metadata: Option<&McpToolApprovalMetadata>,
    ) -> bool {
        Self::capture(
            turn_context,
            manager,
            server,
            metadata,
            self.approval_mode,
        )
        .await
        == Some(self)
    }

    async fn sample(
        turn_context: &TurnContext,
        manager: &McpConnectionManager,
        server: &str,
        metadata: Option<&McpToolApprovalMetadata>,
        approval_mode: AppToolApproval,
    ) -> Self {
        let policy = turn_context.approval_policy.decision_snapshot();
        let (default_reviewer, reviewer_revision) = turn_context.approvals_reviewer.snapshot();
        let approvals_reviewer = connectors::mcp_approvals_reviewer_with_default(
            turn_context.config.as_ref(),
            server,
            metadata.and_then(|metadata| metadata.connector_id.as_deref()),
            default_reviewer,
        );
        Self {
            catalog_revision: manager.catalog_revision().await,
            approval_policy: policy.value,
            policy_revision: policy.revision,
            never_revision: policy.never_revision,
            approvals_reviewer,
            reviewer_revision,
            approval_mode,
        }
    }
}
