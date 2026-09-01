//! Session-scoped approval routing for effectful actions.

use crate::guardian::guardian_rejection_message;
use crate::guardian::guardian_timeout_message;
use crate::guardian::new_guardian_review_id;
use crate::guardian::review_approval_request;
use crate::context::NodeReplReviewEvidence;
use crate::context::NodeReplReviewEvidenceItem;
use crate::hook_runtime::run_permission_request_hooks;
use crate::session::live_approval_policy::LiveApprovalPolicySnapshot;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::RequestPermissionsResponseConstraint;
use crate::tools::approvals::ApprovalAction;
use crate::tools::approvals::ApprovalCacheKey;
use crate::tools::approvals::guardian_cwd;
use crate::tools::approval_review_boundary::prepare_approval_review_input;
use crate::tools::approval_review_boundary::prepare_approval_review_action;
use crate::tools::approval_review_boundary::sanitize_approval_review_result;
use crate::tools::approval_review_lifecycle::ApprovalReviewLifecycle;
use crate::tools::flat_tool_name;
use crate::tools::context::ToolCallSource;
use crate::tools::sandboxing::ToolError;
use codex_config::types::ApprovalsReviewer;
use codex_extension_api::ApprovalReviewCancellation;
use codex_extension_api::ApprovalReviewEvidence;
use codex_extension_api::ApprovalReviewFailure;
use codex_extension_api::ApprovalReviewImage;
use codex_extension_api::ApprovalReviewInput;
use codex_extension_api::ApprovalReviewResult;
use codex_extension_api::ToolCallSource as ExtensionToolCallSource;
use codex_features::Feature;
use codex_hooks::PermissionRequestDecision;
use codex_otel::ToolDecisionSource;
use codex_protocol::error::CodexErr;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::NetworkPolicyRuleAction;
use codex_protocol::protocol::ReviewDecision;
use codex_tools::ToolName;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

const MAX_REVIEWER_REROUTES: usize = 8;
const MAX_ROUTE_SNAPSHOT_ATTEMPTS: usize = 8;
const GUARDIAN_V2_REVIEW_TIMEOUT: Duration = Duration::from_secs(30);
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
    pub(crate) attempt_id: String,
    pub(crate) source: ToolCallSource,
    pub(crate) cancellation_token: CancellationToken,
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

#[derive(Debug)]
pub(crate) struct ApprovalGrant {
    pub(crate) decision: ReviewDecision,
    route: ApprovalRouteSnapshot,
}

#[derive(Clone, Copy)]
enum ApprovalCacheMode {
    Use,
    Bypass,
}

pub(crate) async fn request_approval(
    session: &Arc<Session>,
    action: ApprovalAction,
    ctx: ApprovalContext,
) -> Result<ApprovalGrant, ToolError> {
    if ctx.cancellation_token.is_cancelled() {
        return Err(ToolError::Codex(CodexErr::TurnAborted));
    }
    let Some(initial) = route_snapshot(session, &ctx.turn).await else {
        return reject_settings_churn(&ctx);
    };
    if policy_blocks_request(initial) {
        return finish_resolution(
            &ctx,
            ApprovalResolution {
                decision: ReviewDecision::denied_with_reason(POLICY_CHANGED_REJECTION),
                source: ApprovalResolutionSource::Hook,
                pending_cache: Vec::new(),
            },
            None,
        );
    }

    let permission_request_run_id = match &action {
        #[cfg(unix)]
        ApprovalAction::Execve { approval_id, .. } => approval_id.clone(),
        ApprovalAction::RequestPermissions { .. } => ctx.attempt_id.clone(),
        ApprovalAction::Shell { .. }
        | ApprovalAction::ExecCommand { .. }
        | ApprovalAction::ApplyPatch { .. } => {
            if ctx.retry_reason.is_some() {
                format!("{}:retry", ctx.call_id)
            } else {
                ctx.call_id.clone()
            }
        }
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
                    Some(current),
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
            None,
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
) -> Result<ApprovalGrant, ToolError> {
    for _ in 0..MAX_REVIEWER_REROUTES {
        let Some(before) = route_snapshot(session, &ctx.turn).await else {
            return reject_settings_churn(ctx);
        };
        if before.policy.never_revision != never_revision || policy_blocks_request(before) {
            return finish_resolution(
                ctx,
                ApprovalResolution {
                    decision: ReviewDecision::denied_with_reason(POLICY_CHANGED_REJECTION),
                    source: ApprovalResolutionSource::Hook,
                    pending_cache: Vec::new(),
                },
                None,
            );
        }

        let resolution = if before.guardian {
            request_guardian_approval(session, action, ctx, before).await
        } else {
            request_user_approval(session, action, ctx, before, ApprovalCacheMode::Use).await
        };
        if !decision_grants_action(&resolution.decision) {
            return finish_resolution(ctx, resolution, None);
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
                None,
            );
        }
        if before != after {
            continue;
        }

        if !commit_pending_cache(session, &ctx.turn, &resolution.pending_cache, after).await {
            continue;
        }
        return finish_resolution(ctx, resolution, Some(after));
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

fn policy_blocks_request(route: ApprovalRouteSnapshot) -> bool {
    route.policy.value == AskForApproval::Never
}

async fn request_guardian_approval(
    session: &Arc<Session>,
    action: &ApprovalAction,
    ctx: &ApprovalContext,
    route: ApprovalRouteSnapshot,
) -> ApprovalResolution {
    if ctx.turn.config.features.enabled(Feature::GuardianV2) {
        return match request_guardian_v2_approval(session, action, ctx).await {
            ApprovalReviewResult::Allow(_) => ApprovalResolution {
                decision: ReviewDecision::Approved,
                source: ApprovalResolutionSource::Guardian,
                pending_cache: Vec::new(),
            },
            ApprovalReviewResult::Deny(outcome) => ApprovalResolution {
                decision: ReviewDecision::denied_with_reason(
                    if outcome.rationale.trim().is_empty() {
                        "automatic approval review denied the action".to_string()
                    } else {
                        outcome.rationale
                    },
                ),
                source: ApprovalResolutionSource::Guardian,
                pending_cache: Vec::new(),
            },
            ApprovalReviewResult::Cancelled => ApprovalResolution {
                decision: ReviewDecision::Abort,
                source: ApprovalResolutionSource::Guardian,
                pending_cache: Vec::new(),
            },
            ApprovalReviewResult::ManualReview(failure) => {
                tracing::warn!(?failure, "Guardian V2 requires manual review");
                if route.strict || ctx.required_by_strict {
                    ApprovalResolution {
                        decision: ReviewDecision::denied_with_reason(
                            "automatic approval review failed closed; manual review is required",
                        ),
                        source: ApprovalResolutionSource::Guardian,
                        pending_cache: Vec::new(),
                    }
                } else {
                    let resolution = request_user_approval(
                        session,
                        action,
                        ctx,
                        route,
                        ApprovalCacheMode::Bypass,
                    )
                    .await;
                    ApprovalResolution {
                        decision: match resolution.decision {
                            ReviewDecision::Approved
                            | ReviewDecision::ApprovedForSession
                            | ReviewDecision::ApprovedExecpolicyAmendment { .. } => {
                                ReviewDecision::Approved
                            }
                            decision => decision,
                        },
                        source: ApprovalResolutionSource::User,
                        pending_cache: Vec::new(),
                    }
                }
            }
        };
    }
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

struct CoreApprovalReviewCancellation(CancellationToken);

impl ApprovalReviewCancellation for CoreApprovalReviewCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }

    fn cancelled(&self) -> codex_extension_api::ExtensionFuture<'_, ()> {
        Box::pin(self.0.cancelled())
    }
}

async fn request_guardian_v2_approval(
    session: &Arc<Session>,
    action: &ApprovalAction,
    ctx: &ApprovalContext,
) -> ApprovalReviewResult {
    request_guardian_v2_approval_until(
        session,
        action,
        ctx,
        Instant::now() + GUARDIAN_V2_REVIEW_TIMEOUT,
    )
    .await
}

async fn request_guardian_v2_approval_until(
    session: &Arc<Session>,
    action: &ApprovalAction,
    ctx: &ApprovalContext,
    deadline: Instant,
) -> ApprovalReviewResult {
    let action = match action.approval_review_action() {
        Ok(action) => action,
        Err(error) => {
            tracing::warn!(%error, "failed to prepare Guardian V2 action");
            return ApprovalReviewResult::ManualReview(ApprovalReviewFailure::InvalidInput);
        }
    };
    let action = match prepare_approval_review_action(action) {
        Ok(action) => action,
        Err(failure) => return ApprovalReviewResult::ManualReview(failure),
    };
    let lifecycle = ApprovalReviewLifecycle::begin(
        session,
        &ctx.turn,
        ctx.call_id.clone(),
        action.assessment_action(),
    )
    .await;
    let result = request_guardian_v2_approval_loop(session, action, ctx, deadline).await;
    lifecycle.finish(session, &ctx.turn, &result).await;
    result
}

async fn request_guardian_v2_approval_loop(
    session: &Arc<Session>,
    action: codex_extension_api::ApprovalReviewAction,
    ctx: &ApprovalContext,
    deadline: Instant,
) -> ApprovalReviewResult {
    let history: Vec<ResponseItem> = session
        .clone_history()
        .await
        .raw_items()
        .cloned()
        .collect();
    for _ in 0..MAX_REVIEWER_REROUTES {
        if ctx.cancellation_token.is_cancelled() {
            return ApprovalReviewResult::Cancelled;
        }
        let (evidence_revision, evidence, images) = approval_review_evidence(&ctx.turn, &ctx.source);
        let input = ApprovalReviewInput {
            binding: codex_extension_api::ApprovalReviewBinding {
                thread_id: session.thread_id.to_string(),
                turn_id: ctx.turn.sub_id.clone(),
                action_id: ctx.call_id.clone(),
                attempt_id: ctx.attempt_id.clone(),
                source: extension_tool_call_source(ctx.source.clone()),
                evidence_revision,
            },
            action: action.clone(),
            history: history.clone(),
            evidence,
            images,
            deadline,
            cancellation: Arc::new(CoreApprovalReviewCancellation(
                ctx.cancellation_token.clone(),
            )),
        };
        let input = match prepare_approval_review_input(input) {
            Ok(input) => input,
            Err(failure) => return ApprovalReviewResult::ManualReview(failure),
        };
        let review = session
            .services
            .extensions
            .approval_review(
                &session.services.session_extension_data,
                &session.services.thread_extension_data,
                input,
            );
        let result = tokio::select! {
            biased;
            _ = ctx.cancellation_token.cancelled() => ApprovalReviewResult::Cancelled,
            result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), review) => {
                result.unwrap_or(ApprovalReviewResult::ManualReview(
                    ApprovalReviewFailure::Deadline,
                ))
            }
        };
        let result = sanitize_approval_review_result(result);
        if !matches!(result, ApprovalReviewResult::Allow(_)) {
            return result;
        }
        if Instant::now() >= deadline {
            return ApprovalReviewResult::ManualReview(ApprovalReviewFailure::Deadline);
        }
        if ctx.cancellation_token.is_cancelled() {
            return ApprovalReviewResult::Cancelled;
        }
        if matches!(&ctx.source, ToolCallSource::CodeMode { .. })
            && session
                .turn_context_for_sub_id(&ctx.turn.sub_id)
                .await
                .is_none()
        {
            return ApprovalReviewResult::Cancelled;
        }
        if approval_review_evidence_revision(&ctx.turn, &ctx.source) == evidence_revision {
            return result;
        }
    }
    ApprovalReviewResult::ManualReview(ApprovalReviewFailure::InvalidInput)
}

fn approval_review_evidence(
    turn: &TurnContext,
    source: &ToolCallSource,
) -> (u64, Vec<ApprovalReviewEvidence>, Vec<ApprovalReviewImage>) {
    let ToolCallSource::CodeMode { cell_id, .. } = source else {
        return (0, Vec::new(), Vec::new());
    };
    let Some(snapshot) = turn
        .extension_data
        .get::<NodeReplReviewEvidence>()
        .map(|evidence| evidence.snapshot())
    else {
        return (0, Vec::new(), Vec::new());
    };
    let mut evidence = Vec::new();
    let mut images = Vec::new();
    for record in snapshot
        .records
        .into_iter()
        .filter(|record| record.has_cell_id(cell_id))
    {
        for item in record.items {
            match item {
                NodeReplReviewEvidenceItem::Text(text) => evidence.push(ApprovalReviewEvidence {
                    kind: "node_repl_output".to_string(),
                    provenance: Some(record.provenance.clone()),
                    text,
                }),
                NodeReplReviewEvidenceItem::Image { data_url } => {
                    images.push(ApprovalReviewImage { data_url });
                }
            }
        }
    }
    (snapshot.sequence, evidence, images)
}

fn approval_review_evidence_revision(turn: &TurnContext, source: &ToolCallSource) -> u64 {
    if !matches!(source, ToolCallSource::CodeMode { .. }) {
        return 0;
    }
    turn.extension_data
        .get::<NodeReplReviewEvidence>()
        .map_or(0, |evidence| evidence.snapshot().sequence)
}

fn extension_tool_call_source(source: ToolCallSource) -> ExtensionToolCallSource {
    match source {
        ToolCallSource::Direct => ExtensionToolCallSource::Direct,
        ToolCallSource::CodeMode {
            cell_id,
            runtime_tool_call_id,
        } => ExtensionToolCallSource::CodeMode {
            cell_id,
            runtime_tool_call_id,
        },
    }
}

async fn request_user_approval(
    session: &Session,
    action: &ApprovalAction,
    ctx: &ApprovalContext,
    route: ApprovalRouteSnapshot,
    cache_mode: ApprovalCacheMode,
) -> ApprovalResolution {
    let keys = match cache_mode {
        ApprovalCacheMode::Use => action.cache_keys(),
        ApprovalCacheMode::Bypass => Vec::new(),
    };
    if matches!(cache_mode, ApprovalCacheMode::Use)
        && cached_for_session(session, &keys, route).await
    {
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
                #[cfg(unix)]
                ApprovalAction::Execve { .. } => unreachable!("matched command approval"),
                ApprovalAction::RequestPermissions { .. } => {
                    unreachable!("matched command approval")
                }
            };
            let reason = ctx
                .retry_reason
                .clone()
                .or_else(|| ctx.approval_reason.clone())
                .or_else(|| justification.clone());
            register_delegated_approval_action(session, action, ctx, &ctx.call_id).await;
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
        #[cfg(unix)]
        ApprovalAction::Execve {
            approval_id,
            environment_id,
            source,
            program,
            argv,
            cwd,
            additional_permissions,
            ..
        } => {
            let command = std::iter::once(program.clone())
                .chain(argv.iter().cloned())
                .collect();
            register_delegated_approval_action(session, action, ctx, approval_id).await;
            let decision = session
                .request_command_approval(
                    &ctx.turn,
                    ctx.call_id.clone(),
                    Some(approval_id.clone()),
                    Some(environment_id.clone()),
                    command,
                    cwd.clone(),
                    ctx.approval_reason.clone(),
                    /*network_approval_context*/ None,
                    /*proposed_execpolicy_amendment*/ None,
                    additional_permissions.clone(),
                    Some(vec![ReviewDecision::Approved, ReviewDecision::Abort]),
                )
                .await;
            let tool_name = match source {
                codex_protocol::approvals::GuardianCommandSource::Shell => "shell",
                codex_protocol::approvals::GuardianCommandSource::UnifiedExec => "unified_exec",
            };
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
            register_delegated_approval_action(session, action, ctx, &ctx.call_id).await;
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
        ApprovalAction::RequestPermissions {
            environment,
            cwd,
            reason,
            permissions,
            ..
        } => {
            let response = session
                .request_permissions_user_review(
                    &ctx.turn,
                    ctx.call_id.clone(),
                    reason.clone(),
                    permissions.clone(),
                    environment.clone(),
                    cwd.clone(),
                    RequestPermissionsResponseConstraint::OneShotExact,
                    ctx.cancellation_token.clone(),
                )
                .await;
            let decision = match response {
                Some(response) if !response.permissions.is_empty() => {
                    ReviewDecision::Approved
                }
                Some(_) => ReviewDecision::denied(),
                None => ReviewDecision::Abort,
            };
            ("request_permissions", decision)
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

async fn register_delegated_approval_action(
    session: &Session,
    action: &ApprovalAction,
    ctx: &ApprovalContext,
    approval_id: &str,
) {
    if ctx.turn.session_source.is_non_root_agent() {
        session
            .register_pending_delegated_approval_action(
                approval_id.to_string(),
                action.clone(),
            )
            .await;
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

fn reject_settings_churn(ctx: &ApprovalContext) -> Result<ApprovalGrant, ToolError> {
    finish_resolution(
        ctx,
        ApprovalResolution {
            decision: ReviewDecision::denied_with_reason(APPROVAL_SETTINGS_CHURN_REJECTION),
            source: ApprovalResolutionSource::Hook,
            pending_cache: Vec::new(),
        },
        None,
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
    approved_route: Option<ApprovalRouteSnapshot>,
) -> Result<ApprovalGrant, ToolError> {
    if ctx.cancellation_token.is_cancelled() {
        return Err(ToolError::Codex(CodexErr::TurnAborted));
    }
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
    let decision = normalize_decision(resolution.decision, resolution.source)?;
    let Some(route) = approved_route else {
        return Err(ToolError::Rejected(
            APPROVAL_SETTINGS_CHURN_REJECTION.to_string(),
        ));
    };
    Ok(ApprovalGrant { decision, route })
}

pub(crate) async fn ensure_approval_grant_is_current(
    session: &Session,
    turn: &TurnContext,
    cancellation_token: &CancellationToken,
    grant: &ApprovalGrant,
) -> Result<(), ToolError> {
    if cancellation_token.is_cancelled() {
        return Err(ToolError::Codex(CodexErr::TurnAborted));
    }
    if route_snapshot(session, turn).await != Some(grant.route) {
        return Err(ToolError::Rejected(
            "approval settings changed before the action could run".to_string(),
        ));
    }
    Ok(())
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
