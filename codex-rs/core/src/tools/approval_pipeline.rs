//! Session-scoped approval routing for command and patch actions.

use crate::guardian::guardian_rejection_message;
use crate::guardian::guardian_timeout_message;
use crate::guardian::new_guardian_review_id;
use crate::guardian::review_approval_request;
use crate::hook_runtime::run_permission_request_hooks;
use crate::session::live_approval_policy::LiveApprovalPolicySnapshot;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::tools::approvals::ApprovalAction;
use crate::tools::approvals::ApprovalCacheKey;
use crate::tools::approvals::guardian_cwd;
use crate::tools::flat_tool_name;
use crate::tools::sandboxing::ToolError;
use codex_config::types::ApprovalsReviewer;
use codex_hooks::PermissionRequestDecision;
use codex_otel::ToolDecisionSource;
use codex_protocol::error::CodexErr;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::NetworkPolicyRuleAction;
use codex_protocol::protocol::ReviewDecision;
use codex_tools::ToolName;
use std::future::Future;
use std::sync::Arc;

const MAX_REVIEWER_REROUTES: usize = 8;
const MAX_ROUTE_SNAPSHOT_ATTEMPTS: usize = 8;
const APPROVAL_SETTINGS_CHURN_REJECTION: &str =
    "approval settings changed too often to authorize the action";
pub(crate) const POLICY_CHANGED_REJECTION: &str =
    "approval policy changed to never before the action could run";

#[derive(Clone)]
pub(crate) struct ApprovalContext {
    pub(crate) turn: Arc<TurnContext>,
    pub(crate) call_id: String,
    pub(crate) tool_name: ToolName,
    pub(crate) approval_reason: Option<String>,
    pub(crate) retry_reason: Option<String>,
    pub(crate) network_approval_context: Option<codex_protocol::approvals::NetworkApprovalContext>,
    pub(crate) required_by_strict: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ApprovalRouteSnapshot {
    policy: LiveApprovalPolicySnapshot,
    reviewer: ApprovalsReviewer,
    reviewer_revision: u64,
    strict: bool,
    guardian: bool,
}

#[derive(serde::Serialize)]
struct RoutedApprovalCacheKey<'a> {
    action: &'a ApprovalCacheKey,
    policy: AskForApproval,
    policy_revision: u64,
    never_revision: u64,
    reviewer: ApprovalsReviewer,
    reviewer_revision: u64,
    strict: bool,
    guardian: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ApprovalResolutionSource {
    Hook,
    Guardian,
    User,
}

struct ApprovalResolution {
    decision: ReviewDecision,
    source: ApprovalResolutionSource,
    pending_cache: Vec<ApprovalCacheKey>,
}

pub(crate) async fn request_approval(
    session: &Arc<Session>,
    action: ApprovalAction,
    ctx: ApprovalContext,
) -> Result<ReviewDecision, ToolError> {
    let Some(initial) = route_snapshot(session, &ctx.turn).await else {
        return reject_settings_churn(&ctx);
    };
    if policy_blocks_request(&ctx, initial) {
        return finish_resolution(
            &ctx,
            ApprovalResolution {
                decision: ReviewDecision::denied_with_reason(POLICY_CHANGED_REJECTION),
                source: ApprovalResolutionSource::Hook,
                pending_cache: Vec::new(),
            },
        );
    }

    let permission_request_run_id = if ctx.retry_reason.is_some() {
        format!("{}:retry", ctx.call_id)
    } else {
        ctx.call_id.clone()
    };
    let hook_decision = run_permission_request_hooks(
        session,
        &ctx.turn,
        &permission_request_run_id,
        action.permission_request_payload(),
    )
    .await;

    match hook_decision {
        Some(PermissionRequestDecision::Allow) => {
            let Some(current) = route_snapshot(session, &ctx.turn).await else {
                return reject_settings_churn(&ctx);
            };
            if current == initial {
                finish_resolution(
                    &ctx,
                    ApprovalResolution {
                        decision: ReviewDecision::Approved,
                        source: ApprovalResolutionSource::Hook,
                        pending_cache: Vec::new(),
                    },
                )
            } else {
                resolve_with_live_reviewer(session, &action, &ctx, initial.policy.never_revision)
                    .await
            }
        }
        Some(PermissionRequestDecision::Deny { message }) => finish_resolution(
            &ctx,
            ApprovalResolution {
                decision: ReviewDecision::denied_with_reason(message),
                source: ApprovalResolutionSource::Hook,
                pending_cache: Vec::new(),
            },
        ),
        None => {
            resolve_with_live_reviewer(session, &action, &ctx, initial.policy.never_revision).await
        }
    }
}

pub(crate) async fn routes_to_guardian(session: &Session, turn: &TurnContext) -> bool {
    route_snapshot(session, turn)
        .await
        .is_none_or(|route| route.guardian)
}

async fn resolve_with_live_reviewer(
    session: &Arc<Session>,
    action: &ApprovalAction,
    ctx: &ApprovalContext,
    never_revision: u64,
) -> Result<ReviewDecision, ToolError> {
    for _ in 0..MAX_REVIEWER_REROUTES {
        let Some(before) = route_snapshot(session, &ctx.turn).await else {
            return reject_settings_churn(ctx);
        };
        if before.policy.never_revision != never_revision || policy_blocks_request(ctx, before) {
            return finish_resolution(
                ctx,
                ApprovalResolution {
                    decision: ReviewDecision::denied_with_reason(POLICY_CHANGED_REJECTION),
                    source: ApprovalResolutionSource::Hook,
                    pending_cache: Vec::new(),
                },
            );
        }

        let resolution = if before.guardian {
            request_guardian_approval(session, action, ctx).await
        } else {
            request_user_approval(session, action, ctx, before).await
        };
        if !decision_grants_action(&resolution.decision) {
            return finish_resolution(ctx, resolution);
        }

        let Some(after) = route_snapshot(session, &ctx.turn).await else {
            return reject_settings_churn(ctx);
        };
        if after.policy.never_revision != never_revision {
            return finish_resolution(
                ctx,
                ApprovalResolution {
                    decision: ReviewDecision::denied_with_reason(POLICY_CHANGED_REJECTION),
                    source: ApprovalResolutionSource::Hook,
                    pending_cache: Vec::new(),
                },
            );
        }
        if before != after {
            continue;
        }

        if !commit_pending_cache(session, &ctx.turn, &resolution.pending_cache, after).await {
            continue;
        }
        return finish_resolution(ctx, resolution);
    }

    reject_settings_churn(ctx)
}

async fn route_snapshot(session: &Session, turn: &TurnContext) -> Option<ApprovalRouteSnapshot> {
    bounded_stable_snapshot(|| async {
        let policy = turn.approval_policy.decision_snapshot();
        let (reviewer, reviewer_revision) = turn.approvals_reviewer.snapshot();
        let strict = session.strict_auto_review_enabled_for_turn().await;
        let guardian = strict
            || (matches!(
                policy.value,
                AskForApproval::OnRequest | AskForApproval::Granular(_)
            ) && reviewer == ApprovalsReviewer::AutoReview);
        ApprovalRouteSnapshot {
            policy,
            reviewer,
            reviewer_revision,
            strict,
            guardian,
        }
    })
    .await
}

async fn bounded_stable_snapshot<T, F, Fut>(mut sample: F) -> Option<T>
where
    T: Eq,
    F: FnMut() -> Fut,
    Fut: Future<Output = T>,
{
    for _ in 0..MAX_ROUTE_SNAPSHOT_ATTEMPTS {
        let before = sample().await;
        let after = sample().await;
        if before == after {
            return Some(after);
        }
    }
    None
}

fn policy_blocks_request(ctx: &ApprovalContext, route: ApprovalRouteSnapshot) -> bool {
    route.policy.value == AskForApproval::Never && !(ctx.required_by_strict && route.strict)
}

async fn request_guardian_approval(
    session: &Arc<Session>,
    action: &ApprovalAction,
    ctx: &ApprovalContext,
) -> ApprovalResolution {
    let review_id = new_guardian_review_id();
    let action = match action.clone().into_guardian_request() {
        Ok(action) => action,
        Err(err) => {
            tracing::error!(%err, "failed to build automatic approval action");
            return ApprovalResolution {
                decision: ReviewDecision::denied_with_reason(
                    "automatic approval review could not prepare the action",
                ),
                source: ApprovalResolutionSource::Guardian,
                pending_cache: Vec::new(),
            };
        }
    };
    let decision = review_approval_request(
        session,
        &ctx.turn,
        review_id.clone(),
        action,
        ctx.retry_reason
            .clone()
            .or_else(|| ctx.approval_reason.clone()),
    )
    .await;
    let decision = match decision {
        ReviewDecision::Denied { .. } => ReviewDecision::denied_with_reason(
            guardian_rejection_message(session.as_ref(), &review_id).await,
        ),
        ReviewDecision::Abort => {
            let _ = guardian_rejection_message(session.as_ref(), &review_id).await;
            ReviewDecision::Abort
        }
        decision => decision,
    };
    ApprovalResolution {
        decision,
        source: ApprovalResolutionSource::Guardian,
        pending_cache: Vec::new(),
    }
}

async fn request_user_approval(
    session: &Session,
    action: &ApprovalAction,
    ctx: &ApprovalContext,
    route: ApprovalRouteSnapshot,
) -> ApprovalResolution {
    let keys = action.cache_keys();
    if cached_for_session(session, &keys, route).await {
        return ApprovalResolution {
            decision: ReviewDecision::ApprovedForSession,
            source: ApprovalResolutionSource::User,
            pending_cache: Vec::new(),
        };
    }

    let (tool_name, decision) = match action {
        ApprovalAction::Shell {
            environment_id,
            command,
            cwd,
            additional_permissions,
            justification,
            proposed_execpolicy_amendment,
            ..
        }
        | ApprovalAction::ExecCommand {
            environment_id,
            command,
            cwd,
            additional_permissions,
            justification,
            proposed_execpolicy_amendment,
            ..
        } => {
            let cwd = match guardian_cwd(environment_id, cwd.clone()) {
                Ok(cwd) => cwd,
                Err(err) => {
                    tracing::error!(%err, "failed to resolve approval command cwd");
                    return ApprovalResolution {
                        decision: ReviewDecision::denied_with_reason(format!(
                            "failed to resolve approval command cwd: {err}"
                        )),
                        source: ApprovalResolutionSource::User,
                        pending_cache: Vec::new(),
                    };
                }
            };
            let tool_name = match action {
                ApprovalAction::Shell { .. } => "shell",
                ApprovalAction::ExecCommand { .. } => "unified_exec",
                ApprovalAction::ApplyPatch { .. } => unreachable!("matched command approval"),
            };
            let reason = ctx
                .retry_reason
                .clone()
                .or_else(|| ctx.approval_reason.clone())
                .or_else(|| justification.clone());
            let decision = session
                .request_command_approval(
                    &ctx.turn,
                    ctx.call_id.clone(),
                    /*approval_id*/ None,
                    Some(environment_id.clone()),
                    command.clone(),
                    cwd,
                    reason,
                    ctx.network_approval_context.clone(),
                    proposed_execpolicy_amendment.clone(),
                    additional_permissions.clone(),
                    /*available_decisions*/ None,
                )
                .await;
            (tool_name, decision)
        }
        ApprovalAction::ApplyPatch {
            changes,
            permissions_preapproved,
            ..
        } => {
            let reason = ctx
                .retry_reason
                .clone()
                .or_else(|| ctx.approval_reason.clone());
            if *permissions_preapproved && reason.is_none() {
                return ApprovalResolution {
                    decision: ReviewDecision::Approved,
                    source: ApprovalResolutionSource::User,
                    pending_cache: Vec::new(),
                };
            }
            let decision = session
                .request_patch_approval(
                    &ctx.turn,
                    ctx.call_id.clone(),
                    changes.as_ref().clone(),
                    reason,
                    /*grant_root*/ None,
                )
                .await;
            ("apply_patch", decision)
        }
    };
    record_approval_request(session, tool_name, &decision);
    let pending_cache = if matches!(decision, ReviewDecision::ApprovedForSession) {
        keys
    } else {
        Vec::new()
    };
    ApprovalResolution {
        decision,
        source: ApprovalResolutionSource::User,
        pending_cache,
    }
}

async fn cached_for_session(
    session: &Session,
    keys: &[ApprovalCacheKey],
    route: ApprovalRouteSnapshot,
) -> bool {
    if keys.is_empty() {
        return false;
    }
    let store = session.services.tool_approvals.lock().await;
    keys.iter().all(|key| {
        matches!(
            store.get(&routed_cache_key(key, route)),
            Some(ReviewDecision::ApprovedForSession)
        )
    })
}

async fn commit_pending_cache(
    session: &Session,
    turn: &TurnContext,
    keys: &[ApprovalCacheKey],
    expected_route: ApprovalRouteSnapshot,
) -> bool {
    if !keys.is_empty() {
        // Route-scoped keys keep a concurrent settings change from exposing a stale grant.
        let mut store = session.services.tool_approvals.lock().await;
        for key in keys {
            store.put(
                routed_cache_key(key, expected_route),
                ReviewDecision::ApprovedForSession,
            );
        }
    }
    route_snapshot(session, turn).await == Some(expected_route)
}

fn routed_cache_key(
    action: &ApprovalCacheKey,
    route: ApprovalRouteSnapshot,
) -> RoutedApprovalCacheKey<'_> {
    RoutedApprovalCacheKey {
        action,
        policy: route.policy.value,
        policy_revision: route.policy.revision,
        never_revision: route.policy.never_revision,
        reviewer: route.reviewer,
        reviewer_revision: route.reviewer_revision,
        strict: route.strict,
        guardian: route.guardian,
    }
}

fn reject_settings_churn(ctx: &ApprovalContext) -> Result<ReviewDecision, ToolError> {
    finish_resolution(
        ctx,
        ApprovalResolution {
            decision: ReviewDecision::denied_with_reason(APPROVAL_SETTINGS_CHURN_REJECTION),
            source: ApprovalResolutionSource::Hook,
            pending_cache: Vec::new(),
        },
    )
}

fn record_approval_request(session: &Session, tool_name: &str, decision: &ReviewDecision) {
    session.services.session_telemetry.counter(
        "codex.approval.requested",
        /*inc*/ 1,
        &[
            ("tool", tool_name),
            ("approved", decision.to_opaque_string()),
        ],
    );
}

fn decision_grants_action(decision: &ReviewDecision) -> bool {
    match decision {
        ReviewDecision::Approved
        | ReviewDecision::ApprovedExecpolicyAmendment { .. }
        | ReviewDecision::ApprovedForSession => true,
        ReviewDecision::NetworkPolicyAmendment {
            network_policy_amendment,
        } => network_policy_amendment.action == NetworkPolicyRuleAction::Allow,
        ReviewDecision::ApprovedMcpPolicyAmendment
        | ReviewDecision::Denied { .. }
        | ReviewDecision::TimedOut
        | ReviewDecision::Abort => false,
    }
}

fn finish_resolution(
    ctx: &ApprovalContext,
    resolution: ApprovalResolution,
) -> Result<ReviewDecision, ToolError> {
    let telemetry_source = match resolution.source {
        ApprovalResolutionSource::Hook => ToolDecisionSource::Config,
        ApprovalResolutionSource::Guardian => ToolDecisionSource::AutomatedReviewer,
        ApprovalResolutionSource::User => ToolDecisionSource::User,
    };
    let tool_name = flat_tool_name(&ctx.tool_name);
    ctx.turn.session_telemetry.tool_decision(
        tool_name.as_ref(),
        &ctx.call_id,
        &resolution.decision,
        telemetry_source,
    );
    normalize_decision(resolution.decision, resolution.source)
}

fn normalize_decision(
    decision: ReviewDecision,
    source: ApprovalResolutionSource,
) -> Result<ReviewDecision, ToolError> {
    let rejection = decision
        .rejection_reason()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            match source {
                ApprovalResolutionSource::Hook => "rejected by configuration",
                ApprovalResolutionSource::Guardian => "automatic approval review denied the action",
                ApprovalResolutionSource::User => "rejected by user",
            }
            .to_string()
        });
    match decision {
        ReviewDecision::NetworkPolicyAmendment {
            network_policy_amendment,
        } if network_policy_amendment.action == NetworkPolicyRuleAction::Deny => {
            Err(ToolError::Rejected(rejection))
        }
        ReviewDecision::Denied { .. } => Err(ToolError::Rejected(rejection)),
        ReviewDecision::TimedOut => Err(ToolError::Rejected(match source {
            ApprovalResolutionSource::Guardian => guardian_timeout_message(),
            ApprovalResolutionSource::Hook | ApprovalResolutionSource::User => {
                "approval request timed out".to_string()
            }
        })),
        ReviewDecision::Abort => Err(ToolError::Codex(CodexErr::TurnAborted)),
        ReviewDecision::ApprovedMcpPolicyAmendment => Err(ToolError::Rejected(
            "unsupported approval decision for this action".to_string(),
        )),
        decision => Ok(decision),
    }
}

#[cfg(test)]
#[path = "approval_pipeline_tests.rs"]
mod tests;
