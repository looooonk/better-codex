use std::sync::Arc;
use std::sync::Barrier;
use std::time::Duration;

use codex_code_mode_protocol::CellId;
use codex_code_mode_protocol::CodeModeNestedToolCall;
use codex_code_mode_protocol::CodeModeToolKind;
use codex_code_mode_protocol::grpc as proto;
use codex_code_mode_protocol::grpc::code_mode_host_server::CodeModeHost;
use codex_code_mode_protocol::host::MAX_FRAME_BYTES;
use codex_protocol::ToolName;
use futures::FutureExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tonic::Code;
use tonic::Request;
use tonic::Status;
use uuid::Uuid;

use super::ExecutionAdmission;
use super::GrpcCodeModeHost;
use super::execution_stream;
use super::tests::execute_events;
use super::tests::execute_request;
use super::tests::open_session;
use super::tests::tool;
use super::validation::MAX_IDENTIFIER_BYTES;
use super::validation::MAX_TOOL_DEFINITIONS;
use super::validation::MAX_TOOL_DESCRIPTION_BYTES;
use super::validation::MAX_TOOL_ERROR_BYTES;
use super::validation::MAX_TOOL_FILTERS;
use super::waits::WaitRegistration;
use crate::MAX_ACTIVE_CELLS;
use crate::MAX_IN_FLIGHT_REQUESTS;
use crate::MAX_RECENT_REQUEST_IDS;
use crate::OUTGOING_CHANNEL_CAPACITY;

fn assert_invalid<T>(result: Result<T, Status>) {
    match result {
        Ok(_) => panic!("expected invalid gRPC input to be rejected"),
        Err(error) => assert_eq!(error.code(), Code::InvalidArgument),
    }
}

fn invocation(cell_id: &str, name: &str) -> CodeModeNestedToolCall {
    CodeModeNestedToolCall {
        cell_id: CellId::new(cell_id.to_string()),
        runtime_tool_call_id: "runtime-call".to_string(),
        tool_name: ToolName::plain(name),
        tool_kind: CodeModeToolKind::Function,
        input: None,
    }
}

#[tokio::test]
async fn rejects_values_beyond_every_grpc_metadata_cap() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _events) = open_session(&host).await;
    let oversized_id = "x".repeat(MAX_IDENTIFIER_BYTES + 1);

    assert_invalid(
        host.close_session(Request::new(proto::CloseSessionRequest {
            session_id: oversized_id.clone(),
        }))
        .await,
    );
    assert_invalid(
        host.close_session(Request::new(proto::CloseSessionRequest {
            session_id: "not-a-uuid".to_string(),
        }))
        .await,
    );
    assert_invalid(
        host.complete_tool_call(Request::new(proto::CompleteToolCallRequest {
            session_id: session_id.clone(),
            invocation_id: "not-a-uuid".to_string(),
            outcome: Some(proto::complete_tool_call_request::Outcome::Succeeded(
                proto::ToolCallSucceeded {
                    output_json: b"null".to_vec(),
                },
            )),
        }))
        .await,
    );
    assert_invalid(
        host.acknowledge_notification(Request::new(
            proto::AcknowledgeNotificationRequest {
                session_id: session_id.clone(),
                notification_id: "not-a-uuid".to_string(),
            },
        ))
        .await,
    );
    assert_invalid(
        host.execute(Request::new(execute_request(
            &session_id,
            &oversized_id,
            "text(\"unused\");",
        )))
        .await,
    );
    let mut oversized_tool_call =
        execute_request(&session_id, "oversized-tool-call", "text(\"unused\");");
    oversized_tool_call.tool_call_id = oversized_id.clone();
    assert_invalid(host.execute(Request::new(oversized_tool_call)).await);
    assert_invalid(
        host.subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: vec![proto::ToolName {
                name: oversized_id.clone(),
                namespace: None,
            }],
        }))
        .await,
    );
    assert_invalid(
        host.subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: vec![
                proto::ToolName {
                    name: "echo".to_string(),
                    namespace: None,
                };
                MAX_TOOL_FILTERS + 1
            ],
        }))
        .await,
    );

    let mut oversized_description =
        execute_request(&session_id, "description", "text(\"unused\");");
    oversized_description.enabled_tools = vec![proto::ToolDefinition {
        description: "x".repeat(MAX_TOOL_DESCRIPTION_BYTES + 1),
        ..tool("echo")
    }];
    assert_invalid(host.execute(Request::new(oversized_description)).await);

    let mut too_many_tools = execute_request(&session_id, "tools", "text(\"unused\");");
    too_many_tools.enabled_tools = vec![tool("echo"); MAX_TOOL_DEFINITIONS + 1];
    assert_invalid(host.execute(Request::new(too_many_tools)).await);

    let completion = |message| proto::CompleteToolCallRequest {
        session_id: session_id.clone(),
        invocation_id: Uuid::new_v4().to_string(),
        outcome: Some(proto::complete_tool_call_request::Outcome::Failed(
            proto::ToolCallFailed { message },
        )),
    };
    assert_eq!(
        host.complete_tool_call(Request::new(completion("x".repeat(MAX_TOOL_ERROR_BYTES))))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_invalid(
        host.complete_tool_call(Request::new(completion("x".repeat(MAX_TOOL_ERROR_BYTES + 1))))
            .await,
    );
    assert!(host.state.session(&session_id).is_ok());
}

#[tokio::test]
async fn rejects_heap_limit_until_runtime_enforces_it() {
    let host = GrpcCodeModeHost::new();

    assert_invalid(
        host.open_session(Request::new(proto::OpenSessionRequest {
            cell_execution_limits: Some(proto::SessionCellExecutionLimits {
                max_yield_time_ms: None,
                max_heap_size_bytes: Some(16 * 1_024 * 1_024),
            }),
        }))
        .await,
    );
}

#[tokio::test]
async fn request_and_cell_admission_fail_closed_at_capacity() {
    let host = GrpcCodeModeHost::new();
    let request_permits = (0..MAX_IN_FLIGHT_REQUESTS)
        .map(|_| host.state.request_permit().expect("reserve request permit"))
        .collect::<Vec<_>>();
    let error = match host
        .open_session(Request::new(proto::OpenSessionRequest {
            cell_execution_limits: None,
        }))
        .await
    {
        Ok(_) => panic!("request capacity must be enforced"),
        Err(error) => error,
    };
    assert_eq!(error.code(), Code::ResourceExhausted);
    drop(request_permits);

    let (session_id, _events) = open_session(&host).await;
    let _cell_permits = (0..MAX_ACTIVE_CELLS)
        .map(|_| host.state.cell_permit().expect("reserve cell permit"))
        .collect::<Vec<_>>();
    let error = match host
        .execute(Request::new(execute_request(
            &session_id,
            "capacity",
            "text(\"unused\");",
        )))
        .await
    {
        Ok(_) => panic!("cell capacity must be enforced"),
        Err(error) => error,
    };
    assert_eq!(error.code(), Code::ResourceExhausted);
}

#[tokio::test]
async fn abandoning_execution_releases_its_reservation() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _events) = open_session(&host).await;
    let session = host.state.session(&session_id).expect("open session");
    let execution_id = "execution-abandoned-before-admission".to_string();
    session
        .reserve_execution(&execution_id)
        .expect("reserve execution");

    drop(ExecutionAdmission {
        session: Arc::clone(&session),
        execution_id: Some(execution_id.clone()),
    });

    let error = session
        .admit_execution(
            execution_id,
            "cell".to_string(),
            host.state.cell_permit().expect("reserve cell permit"),
        )
        .expect_err("abandoned execution must not admit a runtime cell");
    assert_eq!(error.code(), Code::Cancelled);
}

#[tokio::test]
async fn outbound_stream_error_keeps_execution_admission_armed() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _events) = open_session(&host).await;
    let session = host.state.session(&session_id).expect("open session");
    let execution_id = "execution-oversized-outcome".to_string();
    session
        .reserve_execution(&execution_id)
        .expect("reserve execution");
    session
        .admit_execution(
            execution_id.clone(),
            "cell-oversized-outcome".to_string(),
            host.state.cell_permit().expect("reserve cell permit"),
        )
        .expect("admit execution");
    let (sender, receiver) = mpsc::channel(/*buffer*/ 1);
    sender
        .send(Err(Status::resource_exhausted("oversized outcome")))
        .await
        .expect("queue outbound error");
    let mut stream = execution_stream(
        receiver,
        ExecutionAdmission {
            session: Arc::clone(&session),
            execution_id: Some(execution_id),
        },
    );

    assert_eq!(
        stream
            .next()
            .await
            .expect("outbound error")
            .expect_err("outbound result must fail")
            .code(),
        Code::ResourceExhausted
    );
    drop(stream);

    assert!(session.state.lock().unwrap().cells.is_empty());
    assert!(host.state.cell_permit().is_ok());
}

#[tokio::test]
async fn close_is_a_cell_admission_barrier() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _lease) = open_session(&host).await;
    let session = host.state.session(&session_id).expect("open session");
    let execution_id = "execution-close-race".to_string();
    session
        .reserve_execution(&execution_id)
        .expect("reserve execution");
    let state = session.state.lock().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let admit_barrier = Arc::clone(&barrier);
    let admit_session = Arc::clone(&session);
    let permit = host.state.cell_permit().expect("reserve cell permit");
    let admission = std::thread::spawn(move || {
        admit_barrier.wait();
        admit_session.admit_execution(execution_id, "cell-close-race".to_string(), permit)
    });

    barrier.wait();
    session.closed.cancel();
    drop(state);

    let error = admission
        .join()
        .expect("cell admission thread")
        .expect_err("closed session must reject cell admission");
    assert_eq!(error.code(), Code::Cancelled);
    assert!(session.state.lock().unwrap().cells.is_empty());
}

#[tokio::test]
async fn close_is_a_tool_dispatch_barrier() {
    let host = GrpcCodeModeHost::new();
    let (session_id, lease) = open_session(&host).await;
    let session = host.state.session(&session_id).expect("open session");
    let execution_id = "execution-dispatch-race".to_string();
    session
        .reserve_execution(&execution_id)
        .expect("reserve execution");
    session
        .admit_execution(
            execution_id.clone(),
            "cell-dispatch-race".to_string(),
            host.state.cell_permit().expect("reserve cell permit"),
        )
        .expect("admit execution");
    let state = session.state.lock().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let dispatch_barrier = Arc::clone(&barrier);
    let dispatch_session = Arc::clone(&session);
    let dispatch = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("build dispatch runtime");
        let cancellation = CancellationToken::new();
        let (response, _receiver) = oneshot::channel();
        dispatch_barrier.wait();
        runtime.block_on(dispatch_session.dispatch_tool(
            invocation("cell-dispatch-race", "echo"),
            execution_id,
            Uuid::new_v4(),
            /*input_json*/ None,
            response,
            &cancellation,
        ))
    });

    barrier.wait();
    session.closed.cancel();
    drop(state);

    let error = dispatch
        .join()
        .expect("tool dispatch thread")
        .expect_err("closed session must reject tool dispatch");
    assert!(error.contains("session closed"));
    assert!(session.state.lock().unwrap().pending_invocations.is_empty());
    drop(lease);
}

#[tokio::test]
async fn close_after_tool_reservation_does_not_commit_or_deliver() {
    let host = GrpcCodeModeHost::new();
    let (session_id, lease) = open_session(&host).await;
    let mut subscription = host
        .subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: Vec::new(),
        }))
        .await
        .unwrap()
        .into_inner();
    let session = host.state.session(&session_id).unwrap();
    session
        .reserve_execution("execution-reserved-close")
        .unwrap();
    session
        .admit_execution(
            "execution-reserved-close".to_string(),
            "cell-reserved-close".to_string(),
            host.state.cell_permit().unwrap(),
        )
        .unwrap();
    let sender = session.state.lock().unwrap().subscriptions[0].sender.clone();
    for _ in 0..OUTGOING_CHANNEL_CAPACITY {
        sender.try_send(Ok(proto::ToolCall::default())).unwrap();
    }
    let baseline_senders = sender.strong_count();
    let invocation_id = Uuid::new_v4();
    let dispatch_session = Arc::clone(&session);
    let dispatch = tokio::spawn(async move {
        let (response, _receiver) = oneshot::channel();
        dispatch_session
            .dispatch_tool(
                invocation("cell-reserved-close", "echo"),
                "execution-reserved-close".to_string(),
                invocation_id,
                /*input_json*/ None,
                response,
                &CancellationToken::new(),
            )
            .await
    });
    while sender.strong_count() <= baseline_senders {
        tokio::task::yield_now().await;
    }
    let state = session.state.lock().unwrap();
    subscription.next().await.unwrap().unwrap();
    while sender.capacity() != 0 {
        tokio::task::yield_now().await;
    }
    session.closed.cancel();
    drop(state);

    assert!(dispatch.await.unwrap().unwrap_err().contains("session closed"));
    let state = session.state.lock().unwrap();
    assert!(state.pending_invocations.is_empty());
    assert!(
        state
            .cells
            .get("cell-reserved-close")
            .is_none_or(|execution| execution.tool_call_sequence == 0)
    );
    drop(state);
    while let Some(Ok(call)) = subscription.next().await {
        assert_ne!(call.invocation_id, invocation_id.to_string());
    }
    drop(lease);
}

#[tokio::test]
async fn terminal_outcome_precedes_cell_closed_and_permit_release() {
    let host = GrpcCodeModeHost::new();
    let (session_id, mut events) = open_session(&host).await;
    let (cell_id, mut execution) = execute_events(
        &host,
        execute_request(&session_id, "execution-terminal-order", "text(\"done\");"),
    )
    .await;
    let session = host.state.session(&session_id).unwrap();
    tokio::time::timeout(Duration::from_secs(/*secs*/ 2), async {
        loop {
            if session
                .state
                .lock()
                .unwrap()
                .cells
                .get(&cell_id)
                .is_some_and(|execution| execution.runtime_closed)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert!(events.next().now_or_never().is_none());
    assert!(matches!(
        execution.next().await.unwrap().unwrap().event,
        Some(proto::execute_event::Event::Outcome(proto::ExecutionOutcome {
            outcome: Some(proto::execution_outcome::Outcome::Completed(_)),
            ..
        }))
    ));
    assert_eq!(
        events.next().await.unwrap().unwrap(),
        proto::SessionEvent {
            event: Some(proto::session_event::Event::CellClosed(proto::CellClosed {
                execution_id: "execution-terminal-order".to_string(),
                cell_id,
                final_tool_call_sequence: 0,
            })),
        }
    );
}

#[tokio::test]
async fn consumed_wait_tombstone_survives_reinsertion_churn() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _lease) = open_session(&host).await;
    let session = host.state.session(&session_id).expect("open session");
    let wait_id = "wait-reinserted".to_string();
    session
        .cancel_wait(&wait_id)
        .await
        .expect("pre-cancel wait");
    let error = match WaitRegistration::new(Arc::clone(&session), wait_id.clone()) {
        Ok(_) => panic!("pre-cancelled wait must not be admitted"),
        Err(error) => error,
    };
    assert_eq!(error.code(), Code::Cancelled);
    {
        let mut state = session.state.lock().unwrap();
        for index in 0..MAX_RECENT_REQUEST_IDS {
            assert!(state.seen_waits.remember(format!("seen-wait-{index}")));
        }
    }
    for index in 0..MAX_RECENT_REQUEST_IDS - 2 {
        let cancelled_id = format!("cancelled-wait-{index}");
        session
            .cancel_wait(&cancelled_id)
            .await
            .expect("churn cancellation tombstones");
    }
    session
        .cancel_wait(&wait_id)
        .await
        .expect("reinsert cancellation tombstone");
    session
        .cancel_wait("cancelled-wait-last")
        .await
        .expect("reach tombstone capacity");

    assert!(
        session
            .state
            .lock()
            .unwrap()
            .cancelled_waits
            .contains(&wait_id)
    );
}

#[tokio::test]
async fn dropping_unread_buffered_execution_outcome_retires_cell() {
    let host = GrpcCodeModeHost::new();
    let (session_id, mut events) = open_session(&host).await;
    let execution = host
        .execute(Request::new(execute_request(
            &session_id,
            "execution-abandoned",
            "await new Promise(() => {});",
        )))
        .await
        .expect("start execution")
        .into_inner();
    let _reserved_permits = (0..MAX_IN_FLIGHT_REQUESTS - 1)
        .map(|_| host.state.request_permit().expect("reserve request permit"))
        .collect::<Vec<_>>();
    let _execution_permit = tokio::time::timeout(Duration::from_secs(/*secs*/ 2), async {
        loop {
            if let Ok(permit) = host.state.request_permit() {
                return permit;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("execution outcome should be buffered before dropping its unread stream");

    drop(execution);

    assert_eq!(
        tokio::time::timeout(Duration::from_secs(/*secs*/ 2), events.next())
            .await
            .expect("abandoned cell should close")
            .expect("cell closed event")
            .expect("session event"),
        proto::SessionEvent {
            event: Some(proto::session_event::Event::CellClosed(proto::CellClosed {
                execution_id: "execution-abandoned".to_string(),
                cell_id: "1".to_string(),
                final_tool_call_sequence: 0,
            })),
        }
    );
}

#[tokio::test]
async fn dropping_session_event_stream_closes_its_lease() {
    let host = GrpcCodeModeHost::new();
    let (session_id, lease) = open_session(&host).await;
    let session = host.state.session(&session_id).expect("open session");

    drop(lease);

    tokio::time::timeout(Duration::from_secs(/*secs*/ 2), session.closed.cancelled())
        .await
        .expect("dropping the event stream must close its session lease");
}

#[tokio::test]
async fn dropping_lease_after_host_drop_closes_session() {
    let host = GrpcCodeModeHost::new();
    let (session_id, lease) = open_session(&host).await;
    let session = host.state.session(&session_id).expect("open session");

    drop(host);
    drop(lease);

    tokio::time::timeout(Duration::from_secs(/*secs*/ 2), session.closed.cancelled())
        .await
        .expect("dropping a lease must close its session after the host disappears");
}

#[tokio::test]
async fn session_closure_cancels_pending_termination() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _events) = open_session(&host).await;
    let (cell_id, mut execution) = execute_events(
        &host,
        execute_request(
            &session_id,
            "execution-termination",
            "await new Promise(() => {});",
        ),
    )
    .await;
    execution.next().await.expect("execution outcome").unwrap();
    let session = host.state.session(&session_id).expect("open session");
    let termination = host.terminate(Request::new(proto::TerminateRequest {
        session_id,
        cell_id,
    }));
    tokio::pin!(termination);
    assert!(termination.as_mut().now_or_never().is_none());

    session.closed.cancel();
    let error = termination
        .await
        .expect_err("closed sessions must bound termination");

    assert_eq!(error.code(), Code::Cancelled);
}

#[tokio::test]
async fn closing_session_releases_buffered_cell_permits() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _events) = open_session(&host).await;
    let session = host.state.session(&session_id).expect("open session");
    let mut permits = (0..MAX_ACTIVE_CELLS)
        .map(|_| host.state.cell_permit().expect("reserve cell permit"))
        .collect::<Vec<_>>();
    session
        .send_event_now(
            proto::session_event::Event::CellClosed(proto::CellClosed {
                execution_id: "execution-queued".to_string(),
                cell_id: "1".to_string(),
                final_tool_call_sequence: 0,
            }),
            permits.pop(),
        )
        .expect("queue cell closure");

    host.close_session(Request::new(proto::CloseSessionRequest { session_id }))
        .await
        .expect("close session");

    assert!(host.state.cell_permit().is_ok());
}

#[tokio::test]
async fn oversized_tool_invocation_does_not_consume_sequence_or_close_session() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _events) = open_session(&host).await;
    let mut subscription = host
        .subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: Vec::new(),
        }))
        .await
        .expect("subscribe to tools")
        .into_inner();
    let (cell_id, mut execution) = execute_events(
        &host,
        execute_request(
            &session_id,
            "execution-oversized",
            "await new Promise(() => {});",
        ),
    )
    .await;
    execution.next().await.expect("execution outcome").unwrap();
    let session = host.state.session(&session_id).expect("open session");
    let cancellation = CancellationToken::new();
    let (response, _receiver) = oneshot::channel();

    let error = session
        .dispatch_tool(
            invocation(&cell_id, "echo"),
            "execution-oversized".to_string(),
            Uuid::new_v4(),
            Some(vec![0; MAX_FRAME_BYTES]),
            response,
            &cancellation,
        )
        .await
        .expect_err("oversized invocation must be rejected");
    assert!(error.contains("gRPC message limit"));
    assert!(!session.closed.is_cancelled());

    let (response, _receiver) = oneshot::channel();
    let invocation_id = Uuid::new_v4();
    session
        .dispatch_tool(
            invocation(&cell_id, "echo"),
            "execution-oversized".to_string(),
            invocation_id,
            Some(b"{}".to_vec()),
            response,
            &cancellation,
        )
        .await
        .expect("dispatch bounded invocation");
    let delivered = subscription
        .next()
        .await
        .expect("tool invocation")
        .expect("valid tool invocation");
    assert_eq!(delivered.invocation_id, invocation_id.to_string());
    assert_eq!(delivered.sequence, 1);
}

#[tokio::test]
async fn unmatched_subscription_failure_allows_recovery() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _events) = open_session(&host).await;
    let _unrelated = host
        .subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: vec![proto::ToolName {
                name: "other".to_string(),
                namespace: None,
            }],
        }))
        .await
        .expect("subscribe unrelated tool stream")
        .into_inner();
    let (cell_id, mut execution) = execute_events(
        &host,
        execute_request(
            &session_id,
            "execution-unmatched",
            "await new Promise(() => {});",
        ),
    )
    .await;
    execution.next().await.expect("execution outcome").unwrap();
    let session = host.state.session(&session_id).expect("open session");
    let cancellation = CancellationToken::new();
    let (response, _receiver) = oneshot::channel();

    session
        .dispatch_tool(
            invocation(&cell_id, "echo"),
            "execution-unmatched".to_string(),
            Uuid::new_v4(),
            Some(b"{}".to_vec()),
            response,
            &cancellation,
        )
        .await
        .expect_err("unmatched tool must fail dispatch");
    assert!(!session.closed.is_cancelled());

    let mut matching = host
        .subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id,
            tool_names: vec![proto::ToolName {
                name: "echo".to_string(),
                namespace: None,
            }],
        }))
        .await
        .expect("subscribe matching tool stream")
        .into_inner();
    let (response, _receiver) = oneshot::channel();
    let invocation_id = Uuid::new_v4();
    session
        .dispatch_tool(
            invocation(&cell_id, "echo"),
            "execution-unmatched".to_string(),
            invocation_id,
            Some(b"{}".to_vec()),
            response,
            &cancellation,
        )
        .await
        .expect("dispatch after adding matching subscription");
    let delivered = matching
        .next()
        .await
        .expect("tool invocation")
        .expect("valid tool invocation");
    assert_eq!(delivered.invocation_id, invocation_id.to_string());
    assert_eq!(delivered.sequence, 1);
}

#[tokio::test]
async fn missing_selected_subscription_retries_alternate_match() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _events) = open_session(&host).await;
    let mut first = host
        .subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: Vec::new(),
        }))
        .await
        .expect("subscribe first tool stream")
        .into_inner();
    let mut second = host
        .subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: Vec::new(),
        }))
        .await
        .expect("subscribe second tool stream")
        .into_inner();
    let (cell_id, mut execution) = execute_events(
        &host,
        execute_request(
            &session_id,
            "execution-retry",
            "await new Promise(() => {});",
        ),
    )
    .await;
    execution.next().await.expect("execution outcome").unwrap();
    let session = host.state.session(&session_id).expect("open session");
    let subscriptions = session
        .state
        .lock()
        .unwrap()
        .subscriptions
        .iter()
        .map(|subscription| (subscription.id, subscription.sender.clone()))
        .collect::<Vec<_>>();
    for (_, sender) in &subscriptions {
        for _ in 0..OUTGOING_CHANNEL_CAPACITY {
            sender
                .try_send(Ok(proto::ToolCall::default()))
                .expect("fill subscription queue");
        }
    }
    let cancellation = CancellationToken::new();
    let (response, _receiver) = oneshot::channel();
    let invocation_id = Uuid::new_v4();
    let dispatch = session.dispatch_tool(
        invocation(&cell_id, "echo"),
        "execution-retry".to_string(),
        invocation_id,
        /*input_json*/ None,
        response,
        &cancellation,
    );
    tokio::pin!(dispatch);
    assert!(dispatch.as_mut().now_or_never().is_none());
    session
        .state
        .lock()
        .unwrap()
        .subscriptions
        .retain(|subscription| subscription.id != subscriptions[0].0);

    first.next().await.expect("free first reservation").unwrap();
    assert!(dispatch.as_mut().now_or_never().is_none());
    second
        .next()
        .await
        .expect("free surviving reservation")
        .unwrap();
    dispatch.await.expect("retry surviving subscription");

    for _ in 1..OUTGOING_CHANNEL_CAPACITY {
        second.next().await.expect("drain buffered call").unwrap();
    }
    assert_eq!(
        second
            .next()
            .await
            .expect("retried invocation")
            .unwrap()
            .invocation_id,
        invocation_id.to_string()
    );
}

#[tokio::test]
async fn filtered_subscription_backpressure_is_independent() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _events) = open_session(&host).await;
    let mut slow = host
        .subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: vec![proto::ToolName {
                name: "slow".to_string(),
                namespace: None,
            }],
        }))
        .await
        .expect("subscribe slow tool stream")
        .into_inner();
    let mut fast = host
        .subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: vec![proto::ToolName {
                name: "fast".to_string(),
                namespace: None,
            }],
        }))
        .await
        .expect("subscribe fast tool stream")
        .into_inner();
    let (cell_id, mut execution) = execute_events(
        &host,
        execute_request(
            &session_id,
            "execution-backpressure",
            "await new Promise(() => {});",
        ),
    )
    .await;
    execution.next().await.expect("execution outcome").unwrap();
    let session = host.state.session(&session_id).expect("open session");
    let cancellation = CancellationToken::new();
    let mut responses = Vec::new();

    for _ in 0..OUTGOING_CHANNEL_CAPACITY {
        let (response, receiver) = oneshot::channel();
        responses.push(receiver);
        session
            .dispatch_tool(
                invocation(&cell_id, "slow"),
                "execution-backpressure".to_string(),
                Uuid::new_v4(),
                /*input_json*/ None,
                response,
                &cancellation,
            )
            .await
            .expect("fill slow subscription");
    }

    let blocked_session = Arc::clone(&session);
    let blocked_cell = cell_id.clone();
    let blocked_cancellation = cancellation.clone();
    let (response, receiver) = oneshot::channel();
    responses.push(receiver);
    let blocked = tokio::spawn(async move {
        blocked_session
            .dispatch_tool(
                invocation(&blocked_cell, "slow"),
                "execution-backpressure".to_string(),
                Uuid::new_v4(),
                /*input_json*/ None,
                response,
                &blocked_cancellation,
            )
            .await
    });
    tokio::task::yield_now().await;
    assert!(!blocked.is_finished());

    let (response, receiver) = oneshot::channel();
    responses.push(receiver);
    let invocation_id = Uuid::new_v4();
    tokio::time::timeout(
        Duration::from_secs(/*secs*/ 1),
        session.dispatch_tool(
            invocation(&cell_id, "fast"),
            "execution-backpressure".to_string(),
            invocation_id,
            /*input_json*/ None,
            response,
            &cancellation,
        ),
    )
    .await
    .expect("saturated subscription must not block another tool")
    .expect("dispatch fast tool");
    assert_eq!(
        fast.next()
            .await
            .expect("fast invocation")
            .expect("valid fast invocation")
            .invocation_id,
        invocation_id.to_string()
    );

    slow.next()
        .await
        .expect("slow invocation")
        .expect("valid slow invocation");
    tokio::time::timeout(Duration::from_secs(/*secs*/ 1), blocked)
        .await
        .expect("draining the subscription should release its blocked invocation")
        .expect("blocked dispatch task")
        .expect("blocked dispatch result");
}

#[tokio::test]
async fn dropping_subscription_with_unread_call_retires_session() {
    let host = GrpcCodeModeHost::new();
    let (session_id, _events) = open_session(&host).await;
    let idle = host
        .subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: Vec::new(),
        }))
        .await
        .expect("subscribe idle tool stream")
        .into_inner();
    drop(idle);

    let session = host.state.session(&session_id).expect("open session");
    tokio::time::timeout(Duration::from_secs(/*secs*/ 2), async {
        while !session.state.lock().unwrap().subscriptions.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("idle subscription should be removed without retiring its session");
    assert!(!session.closed.is_cancelled());

    let first = host
        .subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: Vec::new(),
        }))
        .await
        .expect("subscribe first tool stream")
        .into_inner();
    let mut second = host
        .subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: Vec::new(),
        }))
        .await
        .expect("subscribe second tool stream")
        .into_inner();
    let mut request = execute_request(
        &session_id,
        "execution-subscription-drop",
        r#"await tools.echo({attempt: 1});"#,
    );
    request.yield_time_ms = Some(/*value*/ 10_000);
    request.enabled_tools = vec![tool("echo")];
    let (_cell_id, mut execution) = execute_events(&host, request).await;
    let first_subscription_id = session.state.lock().unwrap().subscriptions[0].id;
    tokio::time::timeout(Duration::from_secs(/*secs*/ 2), async {
        loop {
            let owned = session
                .state
                .lock()
                .unwrap()
                .pending_invocations
                .values()
                .any(|invocation| invocation.subscription_id == first_subscription_id);
            if owned {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first subscription should own an unread tool call");
    drop(first);

    tokio::time::timeout(Duration::from_secs(/*secs*/ 2), async {
        while host.state.session(&session_id).is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("losing an unread call must retire its lease without leaving a sequence gap");
    assert!(
        tokio::time::timeout(Duration::from_secs(/*secs*/ 2), second.next())
            .await
            .expect("other subscriptions should close with their lease")
            .is_none()
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(/*secs*/ 2), execution.next())
            .await
            .expect("execution should retire with its lease")
            .is_none()
    );
}
