use super::*;
use pretty_assertions::assert_eq;
use pretty_assertions::assert_ne;
use std::cell::Cell;
use std::future::ready;
use std::sync::Mutex as StdMutex;

async fn complete_permission_review(
    session: &Arc<crate::session::session::Session>,
    turn: &Arc<crate::session::turn_context::TurnContext>,
    events: &async_channel::Receiver<codex_protocol::protocol::Event>,
    call_id: &str,
    requested: codex_protocol::request_permissions::RequestPermissionProfile,
    response: codex_protocol::request_permissions::RequestPermissionsResponse,
) -> Option<codex_protocol::request_permissions::RequestPermissionsResponse> {
    let review = tokio::spawn({
        let session = Arc::clone(session);
        let turn = Arc::clone(turn);
        let call_id = call_id.to_string();
        async move {
            let environment = turn
                .environments
                .primary()
                .expect("primary environment")
                .selection();
            session
                .request_permissions_for_environment(
                    &turn,
                    call_id,
                    codex_protocol::request_permissions::RequestPermissionsArgs {
                        environment_id: None,
                        reason: None,
                        permissions: requested,
                    },
                    environment,
                    ToolCallSource::Direct,
                    CancellationToken::new(),
                )
                .await
        }
    });
    let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await
        .expect("manual fallback event timed out")
        .expect("manual fallback event");
    let codex_protocol::protocol::EventMsg::RequestPermissions(request) = event.msg else {
        panic!("expected request_permissions event")
    };
    assert_eq!(request.call_id, call_id);
    session
        .notify_request_permissions_response(call_id, response)
        .await;
    review.await.expect("permission review task")
}

#[tokio::test]
async fn stable_snapshot_fails_closed_after_bounded_churn() {
    let reads = Cell::new(0_usize);
    let snapshot = bounded_stable_snapshot(|| {
        let value = reads.get();
        reads.set(value + 1);
        ready(value)
    })
    .await;

    assert_eq!(
        (snapshot, reads.get()),
        (None, MAX_ROUTE_SNAPSHOT_ATTEMPTS * 2)
    );
}

#[test]
fn routed_cache_keys_include_the_live_route() {
    use crate::tools::runtimes::unified_exec::UnifiedExecApprovalKey;
    use codex_protocol::models::SandboxPermissions;
    use codex_utils_path_uri::PathUri;

    let action = ApprovalCacheKey::ExecCommand(UnifiedExecApprovalKey {
        environment_id: "local".to_string(),
        command: vec!["echo".to_string()],
        cwd: PathUri::parse("file:///workspace").expect("valid path URI"),
        tty: false,
        sandbox_permissions: SandboxPermissions::UseDefault,
        additional_permissions: None,
    });
    let route = ApprovalRouteSnapshot {
        policy: LiveApprovalPolicySnapshot {
            value: AskForApproval::OnRequest,
            revision: 0,
            never_revision: 0,
        },
        reviewer: ApprovalsReviewer::User,
        reviewer_revision: 0,
        strict: false,
        guardian: false,
    };
    let mut changed_route = route;
    changed_route.reviewer = ApprovalsReviewer::AutoReview;
    changed_route.reviewer_revision = 1;
    changed_route.guardian = true;

    assert_ne!(
        serde_json::to_string(&routed_cache_key(&action, route)).expect("serialize route key"),
        serde_json::to_string(&routed_cache_key(&action, changed_route))
            .expect("serialize changed route key")
    );
}

#[test]
fn timeout_rejection_is_specific_to_the_reviewer() {
    let rejection = |source| {
        let ToolError::Rejected(message) = normalize_decision(ReviewDecision::TimedOut, source)
            .expect_err("timeout should reject")
        else {
            panic!("timeout should produce a rejection")
        };
        message
    };
    assert_eq!(
        (
            rejection(ApprovalResolutionSource::Guardian),
            rejection(ApprovalResolutionSource::User),
        ),
        (
            guardian_timeout_message(),
            "approval request timed out".to_string(),
        )
    );
}

struct RecordingApprovalAuthority {
    bindings: Arc<StdMutex<Vec<codex_extension_api::ApprovalReviewBinding>>>,
}

impl codex_extension_api::ApprovalReviewContributor for RecordingApprovalAuthority {
    fn review<'a>(
        &'a self,
        _session_store: &'a codex_extension_api::ExtensionData,
        _thread_store: &'a codex_extension_api::ExtensionData,
        input: codex_extension_api::ApprovalReviewInput,
    ) -> codex_extension_api::ExtensionFuture<'a, codex_extension_api::ApprovalReviewResult> {
        self.bindings
            .lock()
            .expect("binding lock")
            .push(input.binding);
        Box::pin(std::future::ready(
            codex_extension_api::ApprovalReviewResult::Allow(
                codex_extension_api::ApprovalReviewOutcome {
                    risk_level: codex_protocol::protocol::GuardianRiskLevel::Low,
                    user_authorization:
                        codex_protocol::protocol::GuardianUserAuthorization::High,
                    rationale: "allowed".to_string(),
                },
            ),
        ))
    }
}

struct NonReturningApprovalAuthority {
    cancel_on_review: Option<CancellationToken>,
}

#[derive(Debug)]
struct HostileApprovalObservation {
    history_items: usize,
    history_bytes: usize,
    evidence_bytes: usize,
    action_bytes: usize,
    contains_secret: bool,
}

struct HostileApprovalAuthority {
    observation: Arc<StdMutex<Option<HostileApprovalObservation>>>,
    secret: String,
}

impl codex_extension_api::ApprovalReviewContributor for HostileApprovalAuthority {
    fn review<'a>(
        &'a self,
        _session_store: &'a codex_extension_api::ExtensionData,
        _thread_store: &'a codex_extension_api::ExtensionData,
        input: codex_extension_api::ApprovalReviewInput,
    ) -> codex_extension_api::ExtensionFuture<'a, codex_extension_api::ApprovalReviewResult> {
        let history = serde_json::to_string(&input.history).expect("serialize review history");
        let evidence = input
            .evidence
            .iter()
            .map(|entry| {
                format!(
                    "{}{}{}",
                    entry.kind,
                    entry.provenance.as_deref().unwrap_or_default(),
                    entry.text
                )
            })
            .collect::<String>();
        let action = serde_json::to_string(&input.action.request_payload())
            .expect("serialize review action");
        *self.observation.lock().expect("observation lock") =
            Some(HostileApprovalObservation {
                history_items: input.history.len(),
                history_bytes: history.len(),
                evidence_bytes: evidence.len(),
                action_bytes: action.len(),
                contains_secret: history.contains(&self.secret)
                    || evidence.contains(&self.secret)
                    || action.contains(&self.secret),
            });
        let rationale = format!("{} {}", self.secret, "r".repeat(100_000));
        Box::pin(std::future::ready(ApprovalReviewResult::Deny(
            codex_extension_api::ApprovalReviewOutcome {
                risk_level: codex_protocol::protocol::GuardianRiskLevel::High,
                user_authorization:
                    codex_protocol::protocol::GuardianUserAuthorization::Low,
                rationale,
            },
        )))
    }
}

impl codex_extension_api::ApprovalReviewContributor for NonReturningApprovalAuthority {
    fn review<'a>(
        &'a self,
        _session_store: &'a codex_extension_api::ExtensionData,
        _thread_store: &'a codex_extension_api::ExtensionData,
        _input: codex_extension_api::ApprovalReviewInput,
    ) -> codex_extension_api::ExtensionFuture<'a, codex_extension_api::ApprovalReviewResult> {
        let cancel_on_review = self.cancel_on_review.clone();
        Box::pin(async move {
            if let Some(cancellation_token) = cancel_on_review {
                cancellation_token.cancel();
            }
            std::future::pending().await
        })
    }
}

async fn guardian_review_test_fixture(
    authority: Arc<dyn codex_extension_api::ApprovalReviewContributor>,
    cancellation_token: CancellationToken,
) -> (
    Arc<crate::session::session::Session>,
    ApprovalAction,
    ApprovalContext,
) {
    use crate::session::tests::make_session_and_context;
    use codex_protocol::models::SandboxPermissions;
    use codex_utils_path_uri::PathUri;

    let (mut session, mut turn) = make_session_and_context().await;
    let mut config = turn.config.as_ref().clone();
    config.features.enable(Feature::GuardianV2);
    turn.config = Arc::new(config);
    let mut extensions = codex_extension_api::ExtensionRegistryBuilder::new();
    extensions.approval_review_contributor(authority);
    session.services.extensions = Arc::new(extensions.build());
    let turn = Arc::new(turn);
    let action = ApprovalAction::Shell {
        id: "guardian-review-call".to_string(),
        environment_id: codex_exec_server::LOCAL_ENVIRONMENT_ID.to_string(),
        command: vec!["echo".to_string(), "safe".to_string()],
        hook_command: "echo safe".to_string(),
        cwd: PathUri::from_abs_path(
            &codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
                std::env::current_dir().expect("current directory"),
            )
            .expect("absolute current directory"),
        ),
        sandbox_permissions: SandboxPermissions::UseDefault,
        additional_permissions: None,
        justification: None,
        proposed_execpolicy_amendment: None,
        cache_keys: Vec::new(),
    };
    let context = ApprovalContext {
        turn,
        call_id: "guardian-review-call".to_string(),
        tool_name: ToolName::plain("shell_command"),
        approval_reason: None,
        retry_reason: None,
        network_approval_context: None,
        required_by_strict: false,
        attempt_id: "guardian-review-attempt".to_string(),
        source: ToolCallSource::Direct,
        cancellation_token,
    };
    (Arc::new(session), action, context)
}

async fn allowed_guardian_grant_fixture() -> (
    Arc<crate::session::session::Session>,
    Arc<crate::session::turn_context::TurnContext>,
    ApprovalGrant,
) {
    use crate::config::Constrained;

    let (session, action, context) = guardian_review_test_fixture(
        Arc::new(RecordingApprovalAuthority {
            bindings: Arc::new(StdMutex::new(Vec::new())),
        }),
        CancellationToken::new(),
    )
    .await;
    context
        .turn
        .approval_policy
        .replace(Constrained::allow_any(AskForApproval::OnRequest));
    context
        .turn
        .approvals_reviewer
        .replace(ApprovalsReviewer::AutoReview);
    let turn = Arc::clone(&context.turn);
    let grant = request_approval(&session, action, context)
        .await
        .expect("Guardian review should allow the action");
    (session, turn, grant)
}

#[tokio::test]
async fn approval_grant_revalidates_reviewer_strict_and_policy_revisions() {
    use crate::config::Constrained;
    use crate::state::ActiveTurn;

    let (session, turn, grant) = allowed_guardian_grant_fixture().await;
    turn.approvals_reviewer.replace(ApprovalsReviewer::User);
    assert!(
        ensure_approval_grant_is_current(&session, &turn, &CancellationToken::new(), &grant)
            .await
            .is_err()
    );

    let (session, turn, grant) = allowed_guardian_grant_fixture().await;
    let active_turn = ActiveTurn::default();
    active_turn
        .turn_state
        .lock()
        .await
        .enable_strict_auto_review();
    *session.active_turn.lock().await = Some(active_turn);
    assert!(
        ensure_approval_grant_is_current(&session, &turn, &CancellationToken::new(), &grant)
            .await
            .is_err()
    );

    let (session, turn, grant) = allowed_guardian_grant_fixture().await;
    turn.approval_policy
        .replace(Constrained::allow_any(AskForApproval::UnlessTrusted));
    assert!(
        ensure_approval_grant_is_current(&session, &turn, &CancellationToken::new(), &grant)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn live_never_policy_blocks_new_action_after_strict_review() {
    use crate::config::Constrained;
    use crate::state::ActiveTurn;

    let bindings = Arc::new(StdMutex::new(Vec::new()));
    let (session, action, mut context) = guardian_review_test_fixture(
        Arc::new(RecordingApprovalAuthority {
            bindings: Arc::clone(&bindings),
        }),
        CancellationToken::new(),
    )
    .await;
    context.required_by_strict = true;
    context
        .turn
        .approval_policy
        .replace(Constrained::allow_any(AskForApproval::OnRequest));
    context
        .turn
        .approvals_reviewer
        .replace(ApprovalsReviewer::User);
    let active_turn = ActiveTurn::default();
    active_turn
        .turn_state
        .lock()
        .await
        .enable_strict_auto_review();
    *session.active_turn.lock().await = Some(active_turn);

    assert_eq!(
        request_approval(&session, action.clone(), context.clone())
            .await
            .expect("strict OnRequest review should allow the action")
            .decision,
        ReviewDecision::Approved
    );
    bindings.lock().expect("binding lock").clear();

    context
        .turn
        .approval_policy
        .replace(Constrained::allow_any(AskForApproval::Never));
    context.call_id = "guardian-blocked-call".to_string();
    context.attempt_id = "guardian-blocked-attempt".to_string();
    let error = request_approval(&session, action, context)
        .await
        .expect_err("live Never must block a newly started action");

    let ToolError::Rejected(message) = error else {
        panic!("live Never should reject the action: {error:?}")
    };
    assert_eq!(message, POLICY_CHANGED_REJECTION);
    assert!(bindings.lock().expect("binding lock").is_empty());
}

#[tokio::test]
async fn guardian_v2_host_bounds_untrusted_contributor_input_and_result() {
    use crate::config::Constrained;
    use crate::context::NodeReplReviewEvidenceItem;
    use codex_protocol::models::ContentItem;
    use codex_protocol::models::ResponseItem;

    let secret = "sk-abcdefghijklmnopqrstuvwxyz012345";
    let observation = Arc::new(StdMutex::new(None));
    let (session, action, mut context) = guardian_review_test_fixture(
        Arc::new(HostileApprovalAuthority {
            observation: Arc::clone(&observation),
            secret: secret.to_string(),
        }),
        CancellationToken::new(),
    )
    .await;
    context
        .turn
        .approval_policy
        .replace(Constrained::allow_any(AskForApproval::OnRequest));
    context
        .turn
        .approvals_reviewer
        .replace(ApprovalsReviewer::AutoReview);
    context.source = ToolCallSource::CodeMode {
        cell_id: "cell-1".to_string(),
        runtime_tool_call_id: "runtime-1".to_string(),
    };
    context
        .turn
        .extension_data
        .get_or_init(NodeReplReviewEvidence::default)
        .record(
            "cell-1",
            "runtime-1",
            vec![NodeReplReviewEvidenceItem::Text(format!(
                "{secret} {}",
                "e".repeat(100_000)
            ))],
        );
    session
        .replace_history(
            (0..100)
                .map(|index| ResponseItem::Message {
                    id: None,
                    role: "user".to_string(),
                    content: vec![ContentItem::InputText {
                        text: format!("history-{index} {secret} {}", "h".repeat(100_000)),
                    }],
                    phase: None,
                    internal_chat_message_metadata_passthrough: None,
                })
                .collect(),
            None,
        )
        .await;

    let error = request_approval(&session, action, context)
        .await
        .expect_err("hostile denial should reject");
    let ToolError::Rejected(message) = error else {
        panic!("hostile denial should be a rejection: {error:?}")
    };
    let observation = observation
        .lock()
        .expect("observation lock")
        .take()
        .expect("contributor should observe one bounded request");

    assert!(observation.history_items <= 40);
    assert!(observation.history_bytes <= 64 * 1024);
    assert!(observation.evidence_bytes <= 64 * 1024);
    assert!(observation.action_bytes <= 16 * 1024);
    assert!(!observation.contains_secret);
    assert!(message.len() <= 4 * 1024);
    assert!(!message.contains(secret));
}

#[tokio::test]
async fn guardian_v2_host_deadline_bounds_non_returning_authority() {
    let cancellation_token = CancellationToken::new();
    let (session, action, context) = guardian_review_test_fixture(
        Arc::new(NonReturningApprovalAuthority {
            cancel_on_review: None,
        }),
        cancellation_token,
    )
    .await;

    assert_eq!(
        request_guardian_v2_approval_until(
            &session,
            &action,
            &context,
            Instant::now() + Duration::from_millis(10),
        )
        .await,
        ApprovalReviewResult::ManualReview(ApprovalReviewFailure::Deadline)
    );
}

#[tokio::test]
async fn guardian_v2_host_cancellation_stops_non_returning_authority() {
    let cancellation_token = CancellationToken::new();
    let (session, action, context) = guardian_review_test_fixture(
        Arc::new(NonReturningApprovalAuthority {
            cancel_on_review: Some(cancellation_token.clone()),
        }),
        cancellation_token,
    )
    .await;

    assert_eq!(
        request_guardian_v2_approval_until(
            &session,
            &action,
            &context,
            Instant::now() + Duration::from_secs(1),
        )
        .await,
        ApprovalReviewResult::Cancelled
    );
}

#[tokio::test]
async fn guardian_v2_allow_is_bound_to_the_exact_action_attempt() {
    use crate::config::Constrained;
    use crate::session::tests::make_session_and_context;
    use codex_protocol::models::SandboxPermissions;
    use codex_utils_path_uri::PathUri;

    let (mut session, mut turn) = make_session_and_context().await;
    let mut config = turn.config.as_ref().clone();
    config.features.enable(Feature::GuardianV2);
    turn.config = Arc::new(config);
    turn.approval_policy
        .replace(Constrained::allow_any(AskForApproval::OnRequest));
    turn.approvals_reviewer
        .replace(ApprovalsReviewer::AutoReview);
    let bindings = Arc::new(StdMutex::new(Vec::new()));
    let mut extensions = codex_extension_api::ExtensionRegistryBuilder::new();
    extensions.approval_review_contributor(Arc::new(RecordingApprovalAuthority {
        bindings: Arc::clone(&bindings),
    }));
    session.services.extensions = Arc::new(extensions.build());
    let session = Arc::new(session);
    let turn = Arc::new(turn);
    let cwd = PathUri::from_abs_path(
        &codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
            std::env::current_dir().expect("current directory"),
        )
        .expect("absolute current directory"),
    );

    let decision = request_approval(
        &session,
        ApprovalAction::Shell {
            id: "call-1".to_string(),
            environment_id: codex_exec_server::LOCAL_ENVIRONMENT_ID.to_string(),
            command: vec!["echo".to_string(), "safe".to_string()],
            hook_command: "echo safe".to_string(),
            cwd,
            sandbox_permissions: SandboxPermissions::UseDefault,
            additional_permissions: None,
            justification: None,
            proposed_execpolicy_amendment: None,
            cache_keys: Vec::new(),
        },
        ApprovalContext {
            turn: Arc::clone(&turn),
            call_id: "call-1".to_string(),
            tool_name: ToolName::plain("shell_command"),
            approval_reason: None,
            retry_reason: None,
            network_approval_context: None,
            required_by_strict: false,
            attempt_id: "attempt-1".to_string(),
            source: crate::tools::context::ToolCallSource::Direct,
            cancellation_token: CancellationToken::new(),
        },
    )
    .await
    .expect("review should allow the action");

    assert_eq!(decision.decision, ReviewDecision::Approved);
    assert_eq!(
        bindings.lock().expect("binding lock").as_slice(),
        [codex_extension_api::ApprovalReviewBinding {
            thread_id: session.thread_id.to_string(),
            turn_id: turn.sub_id.clone(),
            action_id: "call-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            source: codex_extension_api::ToolCallSource::Direct,
            evidence_revision: 0,
        }]
    );
}

#[tokio::test]
async fn code_mode_review_evidence_is_correlated_by_sequence() {
    use crate::context::NodeReplReviewEvidenceItem;
    use crate::session::tests::make_session_and_context;

    let (_session, turn) = make_session_and_context().await;
    turn.extension_data
        .get_or_init(NodeReplReviewEvidence::default)
        .record(
            "cell-1",
            "runtime-1",
            vec![NodeReplReviewEvidenceItem::Text("accepted output".to_string())],
        );
    let source = ToolCallSource::CodeMode {
        cell_id: "cell-1".to_string(),
        runtime_tool_call_id: "runtime-1".to_string(),
    };

    let (revision, evidence, images) = approval_review_evidence(&turn, &source);

    assert_eq!(revision, approval_review_evidence_revision(&turn, &source));
    assert_eq!(
        evidence,
        vec![ApprovalReviewEvidence {
            kind: "node_repl_output".to_string(),
            provenance: Some(
                "tool=node_repl/js cell=cell-1 call=runtime-1".to_string(),
            ),
            text: "accepted output".to_string(),
        }]
    );
    assert_eq!(images, Vec::<ApprovalReviewImage>::new());
}

#[tokio::test]
async fn request_permissions_manual_fallback_is_one_shot_and_exact() {
    use crate::config::Constrained;
    use crate::session::tests::make_session_and_context_with_rx;
    use crate::state::ActiveTurn;
    use codex_protocol::models::FileSystemPermissions;
    use codex_protocol::models::NetworkPermissions;
    use codex_protocol::permissions::FileSystemAccessMode;
    use codex_protocol::permissions::FileSystemPath;
    use codex_protocol::permissions::FileSystemSandboxEntry;
    use codex_protocol::permissions::FileSystemSpecialPath;
    use codex_protocol::request_permissions::PermissionGrantScope;
    use codex_protocol::request_permissions::RequestPermissionProfile;
    use codex_protocol::request_permissions::RequestPermissionsResponse;

    let (session, mut turn, events) = make_session_and_context_with_rx().await;
    *session.active_turn.lock().await = Some(ActiveTurn::default());
    let mut config = turn.config.as_ref().clone();
    config.features.enable(Feature::GuardianV2);
    Arc::get_mut(&mut turn).expect("unique turn context").config = Arc::new(config);
    turn.approval_policy
        .replace(Constrained::allow_any(AskForApproval::OnRequest));
    turn.approvals_reviewer
        .replace(ApprovalsReviewer::AutoReview);

    let network_permissions = RequestPermissionProfile {
        network: Some(NetworkPermissions {
            enabled: Some(true),
        }),
        ..RequestPermissionProfile::default()
    };
    assert_eq!(
        complete_permission_review(
            &session,
            &turn,
            &events,
            "permissions-one-shot",
            network_permissions.clone(),
            RequestPermissionsResponse {
                permissions: network_permissions.clone(),
                scope: PermissionGrantScope::Session,
                strict_auto_review: false,
            },
        )
        .await,
        Some(RequestPermissionsResponse {
            permissions: network_permissions.clone(),
            scope: PermissionGrantScope::Turn,
            strict_auto_review: false,
        })
    );

    #[allow(deprecated)]
    let cwd = turn.cwd.clone();
    let project_root_permissions = RequestPermissionProfile {
        file_system: Some(FileSystemPermissions {
            entries: vec![FileSystemSandboxEntry {
                path: FileSystemPath::Special {
                    value: FileSystemSpecialPath::project_roots(/*subpath*/ None),
                },
                access: FileSystemAccessMode::Write,
            }],
            glob_scan_max_depth: None,
        }),
        ..RequestPermissionProfile::default()
    };
    assert_eq!(
        complete_permission_review(
            &session,
            &turn,
            &events,
            "permissions-materialized-path",
            project_root_permissions.clone(),
            RequestPermissionsResponse {
                permissions: project_root_permissions,
                scope: PermissionGrantScope::Session,
                strict_auto_review: false,
            },
        )
        .await,
        Some(RequestPermissionsResponse {
            permissions: RequestPermissionProfile {
                file_system: Some(FileSystemPermissions::from_read_write_roots(
                    /*read*/ None,
                    Some(vec![cwd.clone()]),
                )),
                ..RequestPermissionProfile::default()
            },
            scope: PermissionGrantScope::Turn,
            strict_auto_review: false,
        })
    );

    let requested_permissions = RequestPermissionProfile {
        network: network_permissions.network.clone(),
        file_system: Some(FileSystemPermissions::from_read_write_roots(
            /*read*/ None,
            Some(vec![cwd]),
        )),
    };
    assert_eq!(
        complete_permission_review(
            &session,
            &turn,
            &events,
            "permissions-subset",
            requested_permissions,
            RequestPermissionsResponse {
                permissions: network_permissions,
                scope: PermissionGrantScope::Turn,
                strict_auto_review: false,
            },
        )
        .await,
        Some(RequestPermissionsResponse {
            permissions: RequestPermissionProfile::default(),
            scope: PermissionGrantScope::Turn,
            strict_auto_review: false,
        })
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hook_allow_reroutes_when_strict_review_activates_while_pending() {
    use crate::config::Constrained;
    use crate::session::tests::make_session_and_context;
    use crate::state::ActiveTurn;
    use codex_hooks::Hooks;
    use codex_hooks::HooksConfig;
    use codex_protocol::models::SandboxPermissions;
    use codex_utils_path_uri::PathUri;
    use core_test_support::hooks::trusted_config_layer_stack;
    use std::fs;
    use std::time::Duration;
    use tokio::time::sleep;
    use tokio::time::timeout;

    let (session, turn) = make_session_and_context().await;
    turn.approval_policy
        .replace(Constrained::allow_any(AskForApproval::OnRequest));
    turn.approvals_reviewer.replace(ApprovalsReviewer::User);

    let codex_home = turn.config.codex_home.to_path_buf();
    fs::create_dir_all(&codex_home).expect("create hook test home");
    let script_path = codex_home.join("blocking_permission_request_hook.py");
    let started_path = codex_home.join("permission_request_hook_started");
    let release_path = codex_home.join("permission_request_hook_release");
    let script = format!(
        r#"import json
from pathlib import Path
import time

json.load(__import__("sys").stdin)
started = Path(r"{started_path}")
release = Path(r"{release_path}")
started.touch()
while not release.exists():
    time.sleep(0.01)
print(json.dumps({{
    "hookSpecificOutput": {{
        "hookEventName": "PermissionRequest",
        "decision": {{"behavior": "allow"}}
    }}
}}))
"#,
        started_path = started_path.display(),
        release_path = release_path.display(),
    );
    fs::write(&script_path, script).expect("write blocking permission hook");
    fs::write(
        codex_home.join("hooks.json"),
        serde_json::json!({
            "hooks": {
                "PermissionRequest": [{
                    "matcher": "Bash",
                    "hooks": [{
                        "type": "command",
                        "command": format!("python3 '{}'", script_path.display()),
                        "timeout_sec": 5,
                    }]
                }]
            }
        })
        .to_string(),
    )
    .expect("write hooks config");
    let hook_list = codex_hooks::list_hooks(HooksConfig {
        feature_enabled: true,
        config_layer_stack: Some(turn.config.config_layer_stack.clone()),
        ..HooksConfig::default()
    });
    assert_eq!(hook_list.hooks.len(), 1);
    let trusted_config_layer_stack = trusted_config_layer_stack(
        &turn.config.config_layer_stack,
        &turn.config.codex_home,
        hook_list.hooks,
    );
    session
        .services
        .hooks
        .store(Arc::new(Hooks::new(HooksConfig {
            feature_enabled: true,
            config_layer_stack: Some(trusted_config_layer_stack),
            shell_program: Some("/bin/sh".to_string()),
            shell_args: vec!["-c".to_string()],
            ..HooksConfig::default()
        })));

    let active_turn = ActiveTurn::default();
    let turn_state = Arc::clone(&active_turn.turn_state);
    *session.active_turn.lock().await = Some(active_turn);

    let session = Arc::new(session);
    let turn = Arc::new(turn);
    let approval = tokio::spawn({
        let session = Arc::clone(&session);
        let turn = Arc::clone(&turn);
        async move {
            request_approval(
                &session,
                ApprovalAction::Shell {
                    id: "strict-hook-call".to_string(),
                    environment_id: codex_exec_server::REMOTE_ENVIRONMENT_ID.to_string(),
                    command: vec!["echo".to_string(), "blocked".to_string()],
                    hook_command: "echo blocked".to_string(),
                    cwd: PathUri::parse("file:///C:/workspace").expect("valid foreign path URI"),
                    sandbox_permissions: SandboxPermissions::UseDefault,
                    additional_permissions: None,
                    justification: None,
                    proposed_execpolicy_amendment: None,
                    cache_keys: Vec::new(),
                },
                ApprovalContext {
                    turn,
                    call_id: "strict-hook-call".to_string(),
                    tool_name: ToolName::plain("shell_command"),
                    approval_reason: None,
                    retry_reason: None,
                    network_approval_context: None,
                    required_by_strict: false,
                    attempt_id: "attempt".to_string(),
                    source: crate::tools::context::ToolCallSource::Direct,
                    cancellation_token: tokio_util::sync::CancellationToken::new(),
                },
            )
            .await
        }
    });

    timeout(Duration::from_secs(5), async {
        while !started_path.exists() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("permission hook should reach the barrier");
    turn_state.lock().await.enable_strict_auto_review();
    fs::write(release_path, "release").expect("release permission hook");

    let error = timeout(Duration::from_secs(5), approval)
        .await
        .expect("approval should complete")
        .expect("approval task should not panic")
        .expect_err("strict activation should revoke the direct hook grant");
    let ToolError::Rejected(message) = error else {
        panic!("strict reroute should reject the invalid Guardian action: {error:?}")
    };
    assert_eq!(
        message,
        "automatic approval review could not prepare the action"
    );
}
