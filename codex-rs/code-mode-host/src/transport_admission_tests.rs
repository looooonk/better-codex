use std::convert::Infallible;

use codex_code_mode_protocol::grpc::CAPABILITY_METADATA_KEY;
use futures::stream;
use http_body::Frame;
use http_body_util::Full;
use http_body_util::StreamBody;
use pretty_assertions::assert_eq;
use tonic::body::Body;
use tonic::codegen::Bytes;
use tonic::codegen::http::Request;
use tonic::codegen::http::Response;
use tower::Layer;
use tower::ServiceExt;

use super::CLOSE_PATH;
use super::COMPLETE_TOOL_PATH;
use super::EXECUTE_BODY_BYTES;
use super::EXECUTE_PATH;
use super::GrpcAdmissionLayer;
use super::SUBSCRIBE_PATH;
use super::preflight_message;
use super::read_body;
use crate::grpc::validation::MAX_TOOL_DEFINITIONS;
use crate::grpc::validation::MAX_TOOL_FILTERS;

const TEST_CAPABILITY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn preflight_rejects_repeated_messages_before_prost_allocation() {
    let execute = [0x2a, 0].repeat(MAX_TOOL_DEFINITIONS + 1);
    assert!(preflight_message(EXECUTE_PATH, &execute).is_err());

    let subscribe = [0x12, 0].repeat(MAX_TOOL_FILTERS + 1);
    assert!(preflight_message(SUBSCRIBE_PATH, &subscribe).is_err());
}

#[test]
fn preflight_skips_unknown_fields_without_counting_nested_tags() {
    let mut execute = vec![0x22, 0x02, 0x2a, 0x00];
    execute.extend([0x2a, 0x00].repeat(MAX_TOOL_DEFINITIONS));
    assert!(preflight_message(EXECUTE_PATH, &execute).is_ok());
}

#[tokio::test]
async fn body_allocation_stops_at_the_route_boundary() {
    let boundary = vec![0; EXECUTE_BODY_BYTES + 5];
    assert_eq!(
        read_body(
            Body::new(Full::new(Bytes::from(boundary))),
            EXECUTE_BODY_BYTES,
        )
        .await
        .unwrap()
        .len(),
        EXECUTE_BODY_BYTES + 5,
    );
    let oversized = vec![0; EXECUTE_BODY_BYTES + 6];
    assert!(
        read_body(
            Body::new(Full::new(Bytes::from(oversized))),
            EXECUTE_BODY_BYTES,
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn slow_execute_body_does_not_block_reserved_control_admission() {
    let layer = GrpcAdmissionLayer::new();
    let slow_layer = layer.clone();
    let slow = tokio::spawn(async move {
        let frames = stream::pending::<Result<Frame<Bytes>, tonic::Status>>();
        slow_layer
            .admit(EXECUTE_PATH, Body::new(StreamBody::new(frames)))
            .await
    });
    tokio::task::yield_now().await;

    let control = tokio::time::timeout(
        std::time::Duration::from_secs(/*secs*/ 1),
        layer.admit(
            CLOSE_PATH,
            Body::new(Full::new(Bytes::from_static(&[0, 0, 0, 0, 0]))),
        ),
    )
    .await
    .expect("control admission must not wait behind an execute upload");
    assert!(control.is_ok());
    slow.abort();
}

#[tokio::test]
async fn invalid_capability_is_rejected_before_slow_body_admission() {
    let layer = GrpcAdmissionLayer::authenticated(TEST_CAPABILITY.into());
    let service = tower::service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(Response::new(Body::empty()))
    });
    let invalid = [EXECUTE_PATH, COMPLETE_TOOL_PATH].map(|path| {
        let frames = stream::pending::<Result<Frame<Bytes>, tonic::Status>>();
        let request = Request::builder()
            .uri(path)
            .header(CAPABILITY_METADATA_KEY, "invalid")
            .body(Body::new(StreamBody::new(frames)))
            .unwrap();
        tokio::spawn(layer.clone().layer(service).oneshot(request))
    });
    tokio::task::yield_now().await;

    let valid = Request::builder()
        .uri(EXECUTE_PATH)
        .header(CAPABILITY_METADATA_KEY, TEST_CAPABILITY)
        .body(Body::new(Full::new(Bytes::from_static(&[0, 0, 0, 0, 0]))))
        .unwrap();
    let valid = tokio::time::timeout(
        std::time::Duration::from_secs(/*secs*/ 1),
        layer.layer(service).oneshot(valid),
    )
    .await
    .expect("valid admission must not wait behind unauthorized uploads")
    .unwrap();
    assert!(valid.headers().get("grpc-status").is_none());

    for response in invalid {
        assert_eq!(
            response
                .await
                .unwrap()
                .unwrap()
                .headers()
                .get("grpc-status")
                .unwrap(),
            "16",
        );
    }
}

#[tokio::test]
async fn repeated_entries_are_rejected_by_the_layer_before_decode() {
    let message = [0x2a, 0].repeat(MAX_TOOL_DEFINITIONS + 1);
    let mut body = vec![0];
    body.extend_from_slice(&(message.len() as u32).to_be_bytes());
    body.extend_from_slice(&message);

    assert!(
        GrpcAdmissionLayer::new()
            .admit(EXECUTE_PATH, Body::new(Full::new(Bytes::from(body))))
            .await
            .is_err()
    );
}

#[test]
fn normal_responses_cannot_consume_critical_headroom() {
    let layer = GrpcAdmissionLayer::new();
    let mut normal = Vec::new();
    for _ in 0..super::NORMAL_RESPONSE_PERMITS {
        normal.push(
            layer
                .response_permit("/codex.code_mode.v1.CodeModeHost/Wait")
                .unwrap(),
        );
    }
    assert!(
        layer
            .response_permit("/codex.code_mode.v1.CodeModeHost/Wait")
            .is_err()
    );
    for path in [
        super::CLOSE_PATH,
        super::ACKNOWLEDGE_PATH,
        super::CANCEL_WAIT_PATH,
    ] {
        let permit = layer.response_permit(path).unwrap();
        drop(permit);
    }
}

#[test]
fn streaming_routes_preserve_every_control_response_slot() {
    let layer = GrpcAdmissionLayer::new();
    let mut permits = Vec::new();
    for (path, count) in [
        (super::OPEN_PATH, super::MAX_OPEN_RESPONSES),
        (super::SUBSCRIBE_PATH, super::MAX_SUBSCRIBE_RESPONSES),
        (super::EXECUTE_PATH, super::MAX_EXECUTE_RESPONSES),
        (
            "/codex.code_mode.v1.CodeModeHost/Wait",
            super::NORMAL_RESPONSE_PERMITS,
        ),
        (super::CLOSE_PATH, super::CRITICAL_RESPONSE_PERMITS),
    ] {
        for _ in 0..count {
            permits.push(layer.response_permit(path).unwrap());
        }
        assert!(layer.response_permit(path).is_err());
    }
    assert_eq!(
        permits.len(),
        super::MAX_STREAMING_RESPONSES + super::MAX_UNARY_RESPONSES
    );
}
