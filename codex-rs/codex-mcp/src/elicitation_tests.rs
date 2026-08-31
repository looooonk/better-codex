use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use codex_protocol::mcp::RequestId as ProtocolRequestId;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_rmcp_client::Elicitation;
use codex_rmcp_client::ElicitationAction;
use codex_rmcp_client::ElicitationResponse;
use pretty_assertions::assert_eq;
use rmcp::model::ElicitRequestParams;
use rmcp::model::ElicitationSchema;
use rmcp::model::RequestId;
use serde_json::json;

use super::*;

struct LifecycleRegistration(Arc<AtomicUsize>);

impl Drop for LifecycleRegistration {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn lifecycle(active: Arc<AtomicUsize>) -> ElicitationLifecycle {
    ElicitationLifecycle::new(move || {
        active.fetch_add(1, Ordering::SeqCst);
        LifecycleRegistration(Arc::clone(&active))
    })
}

fn elicitation() -> Elicitation {
    Elicitation::Mcp(ElicitRequestParams::FormElicitationParams {
        meta: None,
        message: "Which runtime?".to_string(),
        requested_schema: ElicitationSchema::builder()
            .required_property(
                "runtime",
                rmcp::model::PrimitiveSchemaDefinition::String(
                    rmcp::model::StringSchema::new(),
                ),
            )
            .build()
            .expect("schema should build"),
    })
}

fn routed_id(event: codex_protocol::protocol::Event) -> RequestId {
    let EventMsg::ElicitationRequest(request) = event.msg else {
        panic!("expected elicitation request event");
    };
    let ProtocolRequestId::String(id) = request.id else {
        panic!("expected Codex-owned string request ID");
    };
    RequestId::String(id.into())
}

#[tokio::test]
async fn cancelled_request_is_removed_without_affecting_its_peer() {
    let router = ElicitationRequestRouter::default();
    let active = Arc::new(AtomicUsize::new(0));
    let manager = ElicitationRequestManager::new(
        AskForApproval::OnRequest,
        PermissionProfile::default(),
        /*reviewer*/ None,
        Some(lifecycle(Arc::clone(&active))),
        router,
    );
    let (tx_event, rx_event) = async_channel::bounded(2);
    let sender_a = manager.make_sender("server".to_string(), tx_event.clone());
    let sender_b = manager.make_sender("server".to_string(), tx_event);

    let pending_a = tokio::spawn(sender_a(RequestId::Number(1), elicitation()));
    let id_a = routed_id(rx_event.recv().await.expect("request A"));
    let pending_b = tokio::spawn(sender_b(RequestId::Number(2), elicitation()));
    let id_b = routed_id(rx_event.recv().await.expect("request B"));
    assert_eq!(active.load(Ordering::SeqCst), 2);

    pending_a.abort();
    pending_a.await.expect_err("request A must be cancelled");
    assert_eq!(active.load(Ordering::SeqCst), 1);
    manager
        .resolve(
            "server".to_string(),
            id_a,
            ElicitationResponse {
                action: ElicitationAction::Decline,
                content: None,
                meta: None,
            },
        )
        .await
        .expect_err("cancelled request must no longer be routable");

    let response = ElicitationResponse {
        action: ElicitationAction::Accept,
        content: Some(json!({"runtime": "b"})),
        meta: None,
    };
    manager
        .resolve("server".to_string(), id_b, response.clone())
        .await
        .expect("surviving request must remain routable");
    assert_eq!(
        pending_b
            .await
            .expect("request B task")
            .expect("request B response"),
        response
    );
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn closed_event_channel_cleans_up_pending_elicitation() {
    let active = Arc::new(AtomicUsize::new(0));
    let manager = ElicitationRequestManager::new(
        AskForApproval::OnRequest,
        PermissionProfile::default(),
        /*reviewer*/ None,
        Some(lifecycle(Arc::clone(&active))),
        ElicitationRequestRouter::default(),
    );
    let (tx_event, rx_event) = async_channel::bounded(1);
    drop(rx_event);

    let error = manager
        .make_sender("server".to_string(), tx_event)(RequestId::Number(7), elicitation())
        .await
        .expect_err("closed event channel must fail the elicitation");

    assert_eq!(
        error.to_string(),
        "failed to deliver MCP elicitation request"
    );
    assert!(
        manager
            .router
            .requests
            .lock()
            .expect("router lock")
            .is_empty()
    );
    assert_eq!(active.load(Ordering::SeqCst), 0);
}
