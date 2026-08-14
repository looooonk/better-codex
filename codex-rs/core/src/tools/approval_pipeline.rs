//! Session-scoped approval routing for command and patch actions.

use crate::guardian::guardian_rejection_message;
use crate::guardian::new_guardian_review_id;
use crate::guardian::review_approval_request;
use crate::hook_runtime::run_permission_request_hooks;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::tools::approvals::ApprovalAction;
use crate::tools::approvals::ApprovalReviewer;
use crate::tools::approvals::ApprovalResolutionSource;
use crate::tools::approvals::guardian_cwd;
use crate::tools::approvals::normalize_decision;
use crate::tools::flat_tool_name;
use crate::tools::sandboxing::ToolError;
use crate::tools::sandboxing::with_cached_approval;
use codex_hooks::PermissionRequestDecision;
use codex_otel::ToolDecisionSource;
use codex_protocol::protocol::ReviewDecision;
use codex_tools::ToolName;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct ApprovalContext {
    pub(crate) turn: Arc<TurnContext>,
    pub(crate) call_id: String,
    pub(crate) tool_name: ToolName,
    pub(crate) approval_reason: Option<String>,
    pub(crate) retry_reason: Option<String>,
    pub(crate) network_approval_context: Option<codex_protocol::approvals::NetworkApprovalContext>,
}

pub(crate) async fn request_approval(
    session: &Arc<Session>,
    action: ApprovalAction,
    ctx: ApprovalContext,
) -> Result<ReviewDecision, ToolError> {
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

    let (decision, source) = match hook_decision {
        Some(PermissionRequestDecision::Allow) => {
            (ReviewDecision::Approved, ApprovalResolutionSource::Hook)
        }
        Some(PermissionRequestDecision::Deny { message }) => (
            ReviewDecision::denied_with_reason(message),
            ApprovalResolutionSource::Hook,
        ),
        None => {
            // The policy is live and may change while a turn is running, including while hooks
            // are pending, so reviewer selection must happen at this decision point.
            let guardian = routes_to_guardian(session, &ctx.turn).await;
            if guardian {
                let decision = match action.into_guardian_request() {
                    Ok(action) => {
                        let review_id = new_guardian_review_id();
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
                        match decision {
                            ReviewDecision::Denied { .. } => ReviewDecision::denied_with_reason(
                                guardian_rejection_message(session.as_ref(), &review_id).await,
                            ),
                            ReviewDecision::Abort => {
                                let _ =
                                    guardian_rejection_message(session.as_ref(), &review_id).await;
                                ReviewDecision::Abort
                            }
                            ReviewDecision::Approved
                            | ReviewDecision::ApprovedExecpolicyAmendment { .. }
                            | ReviewDecision::ApprovedForSession
                            | ReviewDecision::ApprovedMcpPolicyAmendment
                            | ReviewDecision::NetworkPolicyAmendment { .. }
                            | ReviewDecision::TimedOut => decision,
                        }
                    }
                    Err(err) => {
                        tracing::error!(%err, "failed to build automatic approval action");
                        ReviewDecision::denied_with_reason(
                            "automatic approval review could not prepare the action",
                        )
                    }
                };
                (decision, ApprovalResolutionSource::Guardian)
            } else {
                (
                    request_user_approval(session, &action, &ctx).await,
                    ApprovalResolutionSource::User,
                )
            };
        }
    };

    let telemetry_source = match source {
        ApprovalResolutionSource::Hook => ToolDecisionSource::Config,
        ApprovalResolutionSource::Guardian => ToolDecisionSource::AutomatedReviewer,
        ApprovalResolutionSource::User => ToolDecisionSource::User,
    };
    let tool_name = flat_tool_name(&ctx.tool_name);
    ctx.turn.session_telemetry.tool_decision(
        tool_name.as_ref(),
        &ctx.call_id,
        &decision,
        telemetry_source,
    );
    normalize_decision(decision, source)
}

pub(crate) async fn routes_to_guardian(session: &Session, turn: &TurnContext) -> bool {
    session.strict_auto_review_enabled_for_turn().await
        || ApprovalReviewer::for_turn(turn) == ApprovalReviewer::Guardian
}

async fn request_user_approval(
    session: &Session,
    action: &ApprovalAction,
    ctx: &ApprovalContext,
) -> ReviewDecision {
    match action {
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
                    return ReviewDecision::denied_with_reason(format!(
                        "failed to resolve approval command cwd: {err}"
                    ));
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
            with_cached_approval(&session.services, tool_name, action.cache_keys(), || async {
                session
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
                    .await
            })
            .await
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
                return ReviewDecision::Approved;
            }
            if reason.is_some() {
                return session
                    .request_patch_approval(
                        &ctx.turn,
                        ctx.call_id.clone(),
                        changes.as_ref().clone(),
                        reason,
                        /*grant_root*/ None,
                    )
                    .await;
            }
            with_cached_approval(
                &session.services,
                "apply_patch",
                action.cache_keys(),
                || async {
                    session
                        .request_patch_approval(
                            &ctx.turn,
                            ctx.call_id.clone(),
                            changes.as_ref().clone(),
                            /*reason*/ None,
                            /*grant_root*/ None,
                        )
                        .await
                },
            )
            .await
        }
    }
}
