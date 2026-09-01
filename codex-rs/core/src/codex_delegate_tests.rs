use super::*;
use crate::mcp_tool_call::MCP_TOOL_APPROVAL_DECLINE_SYNTHETIC;
use crate::mcp_tool_call::MCP_TOOL_APPROVAL_QUESTION_ID_PREFIX;
use async_channel::bounded;
use codex_mcp::CODEX_APPS_MCP_SERVER_NAME;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::NetworkPermissions;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ExecApprovalRequestEvent;
use codex_protocol::protocol::GuardianAssessmentAction;
use codex_protocol::protocol::GuardianAssessmentStatus;
use codex_protocol::protocol::GuardianCommandSource;
use codex_protocol::protocol::McpInvocation;
use codex_protocol::protocol::McpStartupCompleteEvent;
use codex_protocol::protocol::McpStartupStatus;
use codex_protocol::protocol::McpStartupUpdateEvent;
use codex_protocol::protocol::RawResponseItemEvent;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::request_permissions::RequestPermissionProfile;
use codex_protocol::request_permissions::RequestPermissionsEvent;
use codex_protocol::request_permissions::RequestPermissionsResponse;
use codex_protocol::request_user_input::RequestUserInputAnswer;
use codex_protocol::request_user_input::RequestUserInputEvent;
use codex_protocol::request_user_input::RequestUserInputQuestion;
use core_test_support::PathBufExt;
use core_test_support::test_path_buf;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tokio::sync::Mutex;
use tokio::sync::watch;
use tokio::time::timeout;

#[derive(Default)]
struct AllowingDelegatedAuthority {
    reviews: Arc<
        StdMutex<
            Vec<(
                codex_extension_api::ApprovalReviewBinding,
                codex_extension_api::ApprovalReviewAction,
            )>,
        >,
    >,
}

impl codex_extension_api::ApprovalReviewContributor for AllowingDelegatedAuthority {
    fn review<'a>(
        &'a self,
        _session_store: &'a codex_extension_api::ExtensionData,
        _thread_store: &'a codex_extension_api::ExtensionData,
        input: codex_extension_api::ApprovalReviewInput,
    ) -> codex_extension_api::ExtensionFuture<'a, codex_extension_api::ApprovalReviewResult> {
        self.reviews
            .lock()
            .expect("review lock")
            .push((input.binding, input.action));
        Box::pin(std::future::ready(
            codex_extension_api::ApprovalReviewResult::Allow(
                codex_extension_api::ApprovalReviewOutcome {
                    risk_level: codex_protocol::protocol::GuardianRiskLevel::Low,
                    user_authorization: codex_protocol::protocol::GuardianUserAuthorization::High,
                    rationale: "allowed".to_string(),
                },
            ),
        ))
    }
}

async fn delegated_v2_fixture() -> (
    Arc<Session>,
    Arc<TurnContext>,
    Arc<AllowingDelegatedAuthority>,
) {
    use crate::state::ActiveTurn;

    let (session, turn, _events) = crate::session::tests::make_session_and_context_with_rx().await;
    let Ok(mut session) = Arc::try_unwrap(session) else {
        panic!("single session reference")
    };
    let mut turn = Arc::try_unwrap(turn).expect("single turn context reference");
    let mut config = (*turn.config).clone();
    config
        .features
        .enable(Feature::GuardianV2)
        .expect("enable Guardian V2");
    config.approvals_reviewer = ApprovalsReviewer::AutoReview;
    turn.config = Arc::new(config);
    turn.approval_policy
        .set(AskForApproval::OnRequest)
        .expect("set on-request policy");
    turn.approvals_reviewer
        .replace(ApprovalsReviewer::AutoReview);
    let authority = Arc::new(AllowingDelegatedAuthority::default());
    let mut extensions = codex_extension_api::ExtensionRegistryBuilder::new();
    extensions.approval_review_contributor(authority.clone());
    session.services.extensions = Arc::new(extensions.build());
    *session.active_turn.lock().await = Some(ActiveTurn::default());
    (Arc::new(session), Arc::new(turn), authority)
}

fn delegated_codex(session: Arc<Session>) -> (Arc<Codex>, Receiver<Submission>) {
    let (tx_sub, rx_sub) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_tx_events, rx_events) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_agent_status_tx, agent_status) = watch::channel(AgentStatus::PendingInit);
    (
        Arc::new(Codex {
            tx_sub,
            rx_event: rx_events,
            agent_status,
            session,
            session_loop_termination: completed_session_loop_termination(),
        }),
        rx_sub,
    )
}

#[tokio::test]
async fn forward_events_filters_private_events_before_blocked_send_is_cancelled() {
    let (tx_events, rx_events) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (tx_sub, rx_sub) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_agent_status_tx, agent_status) = watch::channel(AgentStatus::PendingInit);
    let (session, ctx, _rx_evt) = crate::session::tests::make_session_and_context_with_rx().await;
    let codex = Arc::new(Codex {
        tx_sub,
        rx_event: rx_events,
        agent_status,
        session: Arc::clone(&session),
        session_loop_termination: completed_session_loop_termination(),
    });

    let (tx_out, rx_out) = bounded(1);
    tx_out
        .send(Event {
            id: "full".to_string(),
            msg: EventMsg::TurnAborted(TurnAbortedEvent {
                turn_id: Some("turn-1".to_string()),
                started_at: None,
                reason: TurnAbortReason::Interrupted,
                completed_at: None,
                duration_ms: None,
            }),
        })
        .await
        .unwrap();

    let cancel = CancellationToken::new();
    let forward = tokio::spawn(forward_events(
        Arc::clone(&codex),
        tx_out.clone(),
        session,
        ctx,
        Arc::new(Mutex::new(HashMap::new())),
        cancel.clone(),
    ));

    for msg in [
        EventMsg::McpStartupUpdate(McpStartupUpdateEvent {
            server: "pending".to_string(),
            status: McpStartupStatus::Starting,
        }),
        EventMsg::McpStartupComplete(McpStartupCompleteEvent::default()),
    ] {
        tx_events
            .send(Event {
                id: "delegate-startup".to_string(),
                msg,
            })
            .await
            .unwrap();
    }
    let visible_msg = EventMsg::RawResponseItem(RawResponseItemEvent {
        item: ResponseItem::CustomToolCall {
            id: None,
            status: None,
            call_id: "call-1".to_string(),
            name: "tool".to_string(),
            namespace: None,
            input: "{}".to_string(),
            internal_chat_message_metadata_passthrough: None,
        },
    });
    for id in ["visible-1", "visible-2", "blocked"] {
        tx_events
            .send(Event {
                id: id.to_string(),
                msg: visible_msg.clone(),
            })
            .await
            .unwrap();
    }

    drop(tx_events);
    let received = rx_out.recv().await.expect("prefilled event missing");
    assert_eq!(received.id, "full");
    let received = rx_out.recv().await.expect("visible event missing");
    assert_eq!(received.id, "visible-1");
    cancel.cancel();
    timeout(std::time::Duration::from_millis(1000), forward)
        .await
        .expect("forward_events hung")
        .expect("forward_events join error");

    let mut ops = Vec::new();
    while let Ok(sub) = rx_sub.try_recv() {
        ops.push(sub.op);
    }
    assert!(
        ops.iter().any(|op| matches!(op, Op::Interrupt)),
        "expected Interrupt op after cancellation"
    );
    assert!(
        ops.iter().any(|op| matches!(op, Op::Shutdown)),
        "expected Shutdown op after cancellation"
    );
}

#[tokio::test]
async fn forward_ops_preserves_submission_trace_context() {
    let (tx_sub, rx_sub) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_tx_events, rx_events) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_agent_status_tx, agent_status) = watch::channel(AgentStatus::PendingInit);
    let (session, _ctx, _rx_evt) = crate::session::tests::make_session_and_context_with_rx().await;
    let codex = Arc::new(Codex {
        tx_sub,
        rx_event: rx_events,
        agent_status,
        session,
        session_loop_termination: completed_session_loop_termination(),
    });
    let (tx_ops, rx_ops) = bounded(1);
    let cancel = CancellationToken::new();
    let forward = tokio::spawn(forward_ops(Arc::clone(&codex), rx_ops, cancel));

    let submission = Submission {
        id: "sub-1".to_string(),
        op: Op::Interrupt,
        client_user_message_id: None,
        trace: Some(codex_protocol::protocol::W3cTraceContext {
            traceparent: Some(
                "00-1234567890abcdef1234567890abcdef-1234567890abcdef-01".to_string(),
            ),
            tracestate: Some("vendor=state".to_string()),
        }),
    };
    tx_ops.send(submission.clone()).await.unwrap();
    drop(tx_ops);

    let forwarded = timeout(Duration::from_secs(1), rx_sub.recv())
        .await
        .expect("forward_ops hung")
        .expect("forwarded submission missing");
    assert_eq!(submission.id, forwarded.id);
    assert_eq!(submission.op, forwarded.op);
    assert_eq!(submission.trace, forwarded.trace);

    timeout(Duration::from_secs(1), forward)
        .await
        .expect("forward_ops did not exit")
        .expect("forward_ops join error");
}

#[tokio::test]
async fn run_codex_thread_interactive_respects_pre_cancelled_spawn() {
    let (parent_session, parent_ctx, _rx_events) =
        crate::session::tests::make_session_and_context_with_rx().await;
    let cancel_token = CancellationToken::new();
    cancel_token.cancel();

    let result = timeout(
        Duration::from_secs(/*secs*/ 1),
        run_codex_thread_interactive(
            parent_ctx.config.as_ref().clone(),
            Arc::clone(&parent_session.services.auth_manager),
            Arc::clone(&parent_session.services.models_manager),
            parent_session,
            parent_ctx,
            cancel_token,
            SubAgentSource::Review,
            /*initial_history*/ None,
        ),
    )
    .await
    .expect("cancelled delegate spawn should not hang");

    assert!(matches!(result, Err(CodexErr::TurnAborted)));
}

#[tokio::test]
async fn handle_request_permissions_uses_tool_call_id_for_round_trip() {
    let (parent_session, mut parent_ctx, rx_events) =
        crate::session::tests::make_session_and_context_with_rx().await;
    *parent_session.active_turn.lock().await = Some(crate::state::ActiveTurn::default());
    let parent_ctx_mut = Arc::get_mut(&mut parent_ctx).expect("single turn context ref");
    parent_ctx_mut.environments.turn_environments[0].environment_id = "remote".to_string();

    let (tx_sub, rx_sub) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_tx_events, rx_events_child) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_agent_status_tx, agent_status) = watch::channel(AgentStatus::PendingInit);
    let codex = Arc::new(Codex {
        tx_sub,
        rx_event: rx_events_child,
        agent_status,
        session: Arc::clone(&parent_session),
        session_loop_termination: completed_session_loop_termination(),
    });

    let call_id = "tool-call-1".to_string();
    let expected_response = RequestPermissionsResponse {
        permissions: RequestPermissionProfile {
            network: Some(NetworkPermissions {
                enabled: Some(true),
            }),
            ..RequestPermissionProfile::default()
        },
        scope: PermissionGrantScope::Turn,
        strict_auto_review: false,
    };
    #[allow(deprecated)]
    let delegated_cwd = parent_ctx.cwd.join("delegated-cwd");
    let cancel_token = CancellationToken::new();
    let request_call_id = call_id.clone();
    let request_cwd = delegated_cwd.clone();

    let handle = tokio::spawn({
        let codex = Arc::clone(&codex);
        let parent_session = Arc::clone(&parent_session);
        let parent_ctx = Arc::clone(&parent_ctx);
        let cancel_token = cancel_token.clone();
        async move {
            handle_request_permissions(
                codex.as_ref(),
                &parent_session,
                &parent_ctx,
                RequestPermissionsEvent {
                    call_id: request_call_id,
                    turn_id: "child-turn-1".to_string(),
                    environment_id: Some("remote".to_string()),
                    started_at_ms: 0,
                    reason: Some("need access".to_string()),
                    permissions: RequestPermissionProfile {
                        network: Some(NetworkPermissions {
                            enabled: Some(true),
                        }),
                        ..RequestPermissionProfile::default()
                    },
                    cwd: Some(request_cwd),
                },
                &cancel_token,
            )
            .await;
        }
    });

    let request_event = timeout(Duration::from_secs(1), rx_events.recv())
        .await
        .expect("request_permissions event timed out")
        .expect("request_permissions event missing");
    let EventMsg::RequestPermissions(request) = request_event.msg else {
        panic!("expected RequestPermissions event");
    };
    assert_eq!(request.call_id, call_id.clone());
    assert_eq!(request.environment_id.as_deref(), Some("remote"));
    assert_eq!(request.cwd, Some(delegated_cwd));

    parent_session
        .notify_request_permissions_response(&call_id, expected_response.clone())
        .await;

    timeout(Duration::from_secs(1), handle)
        .await
        .expect("handle_request_permissions hung")
        .expect("handle_request_permissions join error");

    let submission = timeout(Duration::from_secs(1), rx_sub.recv())
        .await
        .expect("request_permissions response timed out")
        .expect("request_permissions response missing");
    assert_eq!(
        submission.op,
        Op::RequestPermissionsResponse {
            id: call_id,
            response: expected_response,
        }
    );
}

#[tokio::test]
async fn delegated_shell_and_patch_use_the_exact_v2_actions() {
    use codex_protocol::models::SandboxPermissions;
    use codex_protocol::protocol::ApplyPatchApprovalRequestEvent;
    use codex_protocol::protocol::FileChange;
    use codex_utils_path_uri::PathUri;

    let (parent_session, parent_ctx, authority) = delegated_v2_fixture().await;
    let (codex, submissions) = delegated_codex(Arc::clone(&parent_session));
    let cwd = test_path_buf("/tmp").abs();
    let command = vec!["printf".to_string(), "%s".to_string(), "safe".to_string()];
    parent_session
        .register_pending_delegated_approval_action(
            "child-shell-call".to_string(),
            ApprovalAction::Shell {
                id: "child-shell-call".to_string(),
                environment_id: codex_exec_server::LOCAL_ENVIRONMENT_ID.to_string(),
                command: command.clone(),
                hook_command: "printf %s safe".to_string(),
                cwd: PathUri::from_abs_path(&cwd),
                sandbox_permissions: SandboxPermissions::UseDefault,
                additional_permissions: None,
                justification: Some("child justification".to_string()),
                proposed_execpolicy_amendment: None,
                cache_keys: Vec::new(),
            },
        )
        .await;
    handle_exec_approval(
        &codex,
        "child-turn".to_string(),
        &parent_session,
        &parent_ctx,
        ExecApprovalRequestEvent {
            call_id: "child-shell-call".to_string(),
            approval_id: None,
            turn_id: "child-turn".to_string(),
            environment_id: None,
            started_at_ms: 0,
            command: vec!["lossy-event".to_string()],
            cwd: cwd.clone(),
            reason: None,
            network_approval_context: None,
            proposed_execpolicy_amendment: None,
            proposed_network_policy_amendments: None,
            additional_permissions: None,
            available_decisions: None,
            parsed_cmd: Vec::new(),
        },
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(
        submissions
            .recv()
            .await
            .expect("shell approval submission")
            .op,
        Op::ExecApproval {
            id: "child-shell-call".to_string(),
            turn_id: Some("child-turn".to_string()),
            decision: ReviewDecision::Approved,
        }
    );

    let relative_path = std::path::PathBuf::from("nested.txt");
    let absolute_path = cwd.join(&relative_path);
    let changes = HashMap::from([(
        relative_path,
        FileChange::Add {
            content: "exact patch content".to_string(),
        },
    )]);
    parent_session
        .register_pending_delegated_approval_action(
            "child-patch-call".to_string(),
            ApprovalAction::ApplyPatch {
                id: "child-patch-call".to_string(),
                environment_id: codex_exec_server::LOCAL_ENVIRONMENT_ID.to_string(),
                cwd: PathUri::from_abs_path(&cwd),
                files: vec![PathUri::from_abs_path(&absolute_path)],
                patch: "*** Add File: nested.txt\nexact patch content".to_string(),
                changes: Arc::new(changes.clone()),
                permissions_preapproved: false,
                cache_keys: Vec::new(),
            },
        )
        .await;
    handle_patch_approval(
        &codex,
        "child-turn".to_string(),
        &parent_session,
        &parent_ctx,
        ApplyPatchApprovalRequestEvent {
            call_id: "child-patch-call".to_string(),
            turn_id: "child-turn".to_string(),
            started_at_ms: 0,
            changes,
            reason: None,
            grant_root: None,
        },
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(
        submissions
            .recv()
            .await
            .expect("patch approval submission")
            .op,
        Op::PatchApproval {
            id: "child-patch-call".to_string(),
            decision: ReviewDecision::Approved,
        }
    );

    assert_eq!(
        authority.reviews.lock().expect("review lock").as_slice(),
        [
            (
                codex_extension_api::ApprovalReviewBinding {
                    thread_id: parent_session.thread_id.to_string(),
                    turn_id: parent_ctx.sub_id.clone(),
                    action_id: "child-shell-call".to_string(),
                    attempt_id: "child-shell-call".to_string(),
                    source: codex_extension_api::ToolCallSource::Direct,
                    evidence_revision: 0,
                },
                codex_extension_api::ApprovalReviewAction::Command {
                    source: GuardianCommandSource::Shell,
                    command: "printf %s safe".to_string(),
                    argv: command,
                    cwd: cwd.clone(),
                    sandbox_permissions: SandboxPermissions::UseDefault,
                    additional_permissions: None,
                    justification: Some("child justification".to_string()),
                    tty: None,
                },
            ),
            (
                codex_extension_api::ApprovalReviewBinding {
                    thread_id: parent_session.thread_id.to_string(),
                    turn_id: parent_ctx.sub_id.clone(),
                    action_id: "child-patch-call".to_string(),
                    attempt_id: "child-patch-call".to_string(),
                    source: codex_extension_api::ToolCallSource::Direct,
                    evidence_revision: 0,
                },
                codex_extension_api::ApprovalReviewAction::ApplyPatch {
                    cwd,
                    files: vec![absolute_path],
                    patch: "*** Add File: nested.txt\nexact patch content".to_string(),
                },
            ),
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn delegated_execve_attempts_keep_unique_approval_ids() {
    let (parent_session, parent_ctx, authority) = delegated_v2_fixture().await;
    let (codex, submissions) = delegated_codex(Arc::clone(&parent_session));
    let cwd = test_path_buf("/tmp").abs();

    for (approval_id, program) in [("execve-1", "/bin/echo"), ("execve-2", "/bin/printf")] {
        parent_session
            .register_pending_delegated_approval_action(
                approval_id.to_string(),
                ApprovalAction::Execve {
                    id: "shared-parent-call".to_string(),
                    approval_id: approval_id.to_string(),
                    environment_id: codex_exec_server::LOCAL_ENVIRONMENT_ID.to_string(),
                    source: GuardianCommandSource::Shell,
                    program: program.to_string(),
                    argv: vec!["custom-zero".to_string(), "arg".to_string()],
                    cwd: cwd.clone(),
                    additional_permissions: None,
                },
            )
            .await;
        handle_exec_approval(
            &codex,
            "child-turn".to_string(),
            &parent_session,
            &parent_ctx,
            ExecApprovalRequestEvent {
                call_id: "shared-parent-call".to_string(),
                approval_id: Some(approval_id.to_string()),
                turn_id: "child-turn".to_string(),
                environment_id: None,
                started_at_ms: 0,
                command: vec!["lossy-event".to_string()],
                cwd: cwd.clone(),
                reason: None,
                network_approval_context: None,
                proposed_execpolicy_amendment: None,
                proposed_network_policy_amendments: None,
                additional_permissions: None,
                available_decisions: None,
                parsed_cmd: Vec::new(),
            },
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(
            submissions
                .recv()
                .await
                .expect("execve approval submission")
                .op,
            Op::ExecApproval {
                id: approval_id.to_string(),
                turn_id: Some("child-turn".to_string()),
                decision: ReviewDecision::Approved,
            }
        );
    }

    let reviews = authority.reviews.lock().expect("review lock");
    assert_eq!(
        reviews.as_slice(),
        [
            (
                codex_extension_api::ApprovalReviewBinding {
                    thread_id: parent_session.thread_id.to_string(),
                    turn_id: parent_ctx.sub_id.clone(),
                    action_id: "shared-parent-call".to_string(),
                    attempt_id: "execve-1".to_string(),
                    source: codex_extension_api::ToolCallSource::Direct,
                    evidence_revision: 0,
                },
                codex_extension_api::ApprovalReviewAction::Execve {
                    source: GuardianCommandSource::Shell,
                    program: "/bin/echo".to_string(),
                    argv: vec!["custom-zero".to_string(), "arg".to_string()],
                    cwd: cwd.clone(),
                    additional_permissions: None,
                },
            ),
            (
                codex_extension_api::ApprovalReviewBinding {
                    thread_id: parent_session.thread_id.to_string(),
                    turn_id: parent_ctx.sub_id.clone(),
                    action_id: "shared-parent-call".to_string(),
                    attempt_id: "execve-2".to_string(),
                    source: codex_extension_api::ToolCallSource::Direct,
                    evidence_revision: 0,
                },
                codex_extension_api::ApprovalReviewAction::Execve {
                    source: GuardianCommandSource::Shell,
                    program: "/bin/printf".to_string(),
                    argv: vec!["custom-zero".to_string(), "arg".to_string()],
                    cwd,
                    additional_permissions: None,
                },
            ),
        ]
    );
}

#[tokio::test]
async fn handle_exec_approval_uses_call_id_for_guardian_review_and_approval_id_for_reply() {
    let (parent_session, parent_ctx, rx_events) =
        crate::session::tests::make_session_and_context_with_rx().await;
    let mut parent_ctx = Arc::try_unwrap(parent_ctx).expect("single turn context ref");
    let mut config = (*parent_ctx.config).clone();
    config.approvals_reviewer = ApprovalsReviewer::AutoReview;
    parent_ctx.config = Arc::new(config);
    parent_ctx
        .approval_policy
        .set(AskForApproval::OnRequest)
        .expect("set on-request policy");
    let parent_ctx = Arc::new(parent_ctx);

    let (tx_sub, rx_sub) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_tx_events, rx_events_child) = bounded(SUBMISSION_CHANNEL_CAPACITY);
    let (_agent_status_tx, agent_status) = watch::channel(AgentStatus::PendingInit);
    let codex = Arc::new(Codex {
        tx_sub,
        rx_event: rx_events_child,
        agent_status,
        session: Arc::clone(&parent_session),
        session_loop_termination: completed_session_loop_termination(),
    });

    let cancel_token = CancellationToken::new();
    let handle = tokio::spawn({
        let codex = Arc::clone(&codex);
        let parent_session = Arc::clone(&parent_session);
        let parent_ctx = Arc::clone(&parent_ctx);
        let cancel_token = cancel_token.clone();
        async move {
            handle_exec_approval(
                codex.as_ref(),
                "child-turn-1".to_string(),
                &parent_session,
                &parent_ctx,
                ExecApprovalRequestEvent {
                    call_id: "command-item-1".to_string(),
                    approval_id: Some("callback-approval-1".to_string()),
                    turn_id: "child-turn-1".to_string(),
                    environment_id: Some("remote".to_string()),
                    started_at_ms: 0,
                    command: vec!["rm".to_string(), "-rf".to_string(), "tmp".to_string()],
                    cwd: test_path_buf("/tmp").abs(),
                    reason: Some("unsafe subcommand".to_string()),
                    network_approval_context: None,
                    proposed_execpolicy_amendment: None,
                    proposed_network_policy_amendments: None,
                    additional_permissions: None,
                    available_decisions: Some(vec![
                        ReviewDecision::Approved,
                        ReviewDecision::Abort,
                    ]),
                    parsed_cmd: Vec::new(),
                },
                &cancel_token,
            )
            .await;
        }
    });

    let assessment_event = timeout(Duration::from_secs(2), async {
        loop {
            let event = rx_events.recv().await.expect("guardian assessment event");
            if let EventMsg::GuardianAssessment(assessment) = event.msg {
                return assessment;
            }
        }
    })
    .await
    .expect("timed out waiting for guardian assessment");
    let expected_action = GuardianAssessmentAction::Command {
        source: GuardianCommandSource::Shell,
        command: "rm -rf tmp".to_string(),
        cwd: test_path_buf("/tmp").abs(),
    };
    assert!(!assessment_event.id.is_empty());
    assert_eq!(
        assessment_event.target_item_id.as_deref(),
        Some("command-item-1")
    );
    assert_eq!(assessment_event.turn_id, parent_ctx.sub_id);
    assert_eq!(
        assessment_event.status,
        GuardianAssessmentStatus::InProgress
    );
    assert_eq!(assessment_event.risk_level, None);
    assert_eq!(assessment_event.user_authorization, None);
    assert_eq!(assessment_event.rationale, None);
    assert_eq!(assessment_event.decision_source, None);
    assert_eq!(assessment_event.action, expected_action);

    cancel_token.cancel();

    timeout(Duration::from_secs(2), handle)
        .await
        .expect("handle_exec_approval hung")
        .expect("handle_exec_approval join error");

    let submission = timeout(Duration::from_secs(2), rx_sub.recv())
        .await
        .expect("exec approval response timed out")
        .expect("exec approval response missing");
    assert_eq!(
        submission.op,
        Op::ExecApproval {
            id: "callback-approval-1".to_string(),
            turn_id: Some("child-turn-1".to_string()),
            decision: ReviewDecision::Abort,
        }
    );
}

#[tokio::test]
async fn delegated_mcp_guardian_abort_returns_synthetic_decline_answer() {
    let (parent_session, parent_ctx, _rx_events) =
        crate::session::tests::make_session_and_context_with_rx().await;
    let mut parent_ctx = Arc::try_unwrap(parent_ctx).expect("single turn context ref");
    let mut config = (*parent_ctx.config).clone();
    config.approvals_reviewer = ApprovalsReviewer::AutoReview;
    parent_ctx.config = Arc::new(config);
    parent_ctx
        .approval_policy
        .set(AskForApproval::OnRequest)
        .expect("set on-request policy");
    parent_ctx
        .approvals_reviewer
        .replace(ApprovalsReviewer::AutoReview);
    let parent_ctx = Arc::new(parent_ctx);

    let pending_mcp_invocations = Arc::new(Mutex::new(HashMap::from([(
        "call-1".to_string(),
        PendingMcpInvocation {
            invocation: McpInvocation {
                server: "custom_server".to_string(),
                tool: "dangerous_tool".to_string(),
                arguments: None,
            },
            metadata: None,
        },
    )])));
    let cancel_token = CancellationToken::new();
    cancel_token.cancel();

    let response = maybe_auto_review_mcp_request_user_input(
        &parent_session,
        &parent_ctx,
        &pending_mcp_invocations,
        &RequestUserInputEvent {
            call_id: "call-1".to_string(),
            turn_id: "child-turn-1".to_string(),
            questions: vec![RequestUserInputQuestion {
                id: format!("{MCP_TOOL_APPROVAL_QUESTION_ID_PREFIX}_call-1"),
                header: "Approve app tool call?".to_string(),
                question: "Allow this app tool?".to_string(),
                is_other: false,
                is_secret: false,
                options: None,
            }],
            auto_resolution_ms: None,
        },
        &cancel_token,
    )
    .await;

    assert_eq!(
        response,
        Some(RequestUserInputResponse {
            answers: HashMap::from([(
                format!("{MCP_TOOL_APPROVAL_QUESTION_ID_PREFIX}_call-1"),
                RequestUserInputAnswer {
                    answers: vec![MCP_TOOL_APPROVAL_DECLINE_SYNTHETIC.to_string()],
                },
            )]),
        })
    );
}

#[tokio::test]
async fn delegated_mcp_user_reviewer_returns_none_without_metadata() {
    let (parent_session, parent_ctx, _rx_events) =
        crate::session::tests::make_session_and_context_with_rx().await;
    let pending_mcp_invocations = Arc::new(Mutex::new(HashMap::from([(
        "call-1".to_string(),
        PendingMcpInvocation {
            invocation: McpInvocation {
                server: CODEX_APPS_MCP_SERVER_NAME.to_string(),
                tool: "dangerous_tool".to_string(),
                arguments: None,
            },
            metadata: None,
        },
    )])));
    let cancel_token = CancellationToken::new();

    let event = RequestUserInputEvent {
        call_id: "call-1".to_string(),
        turn_id: "child-turn-1".to_string(),
        questions: vec![RequestUserInputQuestion {
            id: format!("{MCP_TOOL_APPROVAL_QUESTION_ID_PREFIX}_call-1"),
            header: "Approve app tool call?".to_string(),
            question: "Allow this app tool?".to_string(),
            is_other: false,
            is_secret: false,
            options: None,
        }],
        auto_resolution_ms: None,
    };
    let response = maybe_auto_review_mcp_request_user_input(
        &parent_session,
        &parent_ctx,
        &pending_mcp_invocations,
        &event,
        &cancel_token,
    )
    .await;
    assert_eq!(response, None);
}
