use std::sync::Arc;
use std::time::Duration;

use codex_code_mode_protocol::CellId;
use codex_code_mode_protocol::CodeModeNestedToolCall;
use codex_code_mode_protocol::CodeModeToolKind;
use codex_code_mode_protocol::grpc as proto;
use codex_code_mode_protocol::grpc::code_mode_host_server::CodeModeHost;
use codex_code_mode_protocol::host::MAX_FRAME_BYTES;
use codex_protocol::ToolName;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tonic::Code;
use tonic::Request;
use tonic::Status;
use uuid::Uuid;

use super::ExecutionAdmission;
use super::GrpcCodeModeHost;
use super::tests::execute_events;
use super::tests::execute_request;
use super::tests::open_session;
use super::tests::tool;
use super::validation::MAX_IDENTIFIER_BYTES;
use super::validation::MAX_TOOL_DEFINITIONS;
use super::validation::MAX_TOOL_DESCRIPTION_BYTES;
use super::validation::MAX_TOOL_ERROR_BYTES;
use super::validation::MAX_TOOL_FILTERS;
use crate::MAX_ACTIVE_CELLS;
use crate::MAX_IN_FLIGHT_REQUESTS;

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
        host.subscribe_to_tool_calls(Request::new(proto::SubscribeToToolCallsRequest {
            session_id: session_id.clone(),
            tool_names: vec![proto::ToolName {
                name: oversized_id,
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
