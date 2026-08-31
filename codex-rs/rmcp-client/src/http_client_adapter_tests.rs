use std::io::ErrorKind;

use codex_exec_server::HttpRedirectPolicy;
use futures::StreamExt;
use futures::stream;
use pretty_assertions::assert_eq;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;
use rmcp::model::ClientJsonRpcMessage;
use rmcp::model::ClientRequest;
use rmcp::model::DiscoverRequest;
use rmcp::model::DiscoverRequestParams;
use rmcp::model::ErrorCode;
use rmcp::model::ErrorData;
use rmcp::model::JsonRpcMessage;
use rmcp::model::RequestId;
use rmcp::model::ServerJsonRpcMessage;
use rmcp::transport::common::http_header::HEADER_MCP_PROTOCOL_VERSION;
use sse_stream::Sse;

use super::HttpHeader;
use super::SseEventSizeLimit;
use super::body_preview;
use super::mcp_redirect_policy;
use super::next_correlated_discovery_response;
use super::protocol_headers;
use crate::http_discovery::correlated_discovery_response;

fn discovery_request(id: &str) -> ClientJsonRpcMessage {
    ClientJsonRpcMessage::request(
        ClientRequest::from(DiscoverRequest::new(DiscoverRequestParams {})),
        RequestId::String(id.to_string().into()),
    )
}

fn server_error(id: Option<&str>, code: ErrorCode, message: &str) -> ServerJsonRpcMessage {
    ServerJsonRpcMessage::error(
        ErrorData::new(code, message.to_string(), None),
        id.map(|id| RequestId::String(id.to_string().into())),
    )
}

#[test]
fn legacy_requests_keep_redirect_compatibility() {
    assert_eq!(
        mcp_redirect_policy(&HeaderMap::new()),
        HttpRedirectPolicy::Follow
    );
}

#[test]
fn modern_protocol_requests_stop_redirects() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HEADER_MCP_PROTOCOL_VERSION,
        HeaderValue::from_static("2026-07-28"),
    );

    assert_eq!(mcp_redirect_policy(&headers), HttpRedirectPolicy::Stop);
}

#[test]
fn json_discovery_rejects_a_wrong_response_id() {
    let request = discovery_request("discover-1");
    let wrong_response = server_error(
        Some("other-request"),
        ErrorCode::METHOD_NOT_FOUND,
        "method not found",
    );

    assert!(
        correlated_discovery_response(
            &request,
            wrong_response,
            /*allow_idless_http_prevalidation*/ false,
        )
        .is_none()
    );
}

#[test]
fn only_evidenced_idless_http_prevalidation_errors_are_correlated() {
    let request = discovery_request("discover-1");
    let error = server_error(
        /*id*/ None,
        ErrorCode(-32000),
        "Bad Request: No valid session ID provided",
    );

    assert!(
        correlated_discovery_response(
            &request,
            error.clone(),
            /*allow_idless_http_prevalidation*/ false,
        )
        .is_none()
    );
    let response = correlated_discovery_response(
        &request, error, /*allow_idless_http_prevalidation*/ true,
    )
    .expect("known id-less prevalidation rejection should trigger legacy fallback");
    let JsonRpcMessage::Error(response) = response else {
        panic!("legacy fallback should return a JSON-RPC error");
    };
    assert_eq!(
        (response.id, response.error.code),
        (
            Some(RequestId::String("discover-1".to_string().into())),
            ErrorCode::METHOD_NOT_FOUND,
        )
    );
}

#[tokio::test]
async fn sse_discovery_ignores_wrong_ids_until_the_matching_response() {
    let request = discovery_request("discover-1");
    let wrong = serde_json::to_string(&server_error(
        Some("other-request"),
        ErrorCode::INTERNAL_ERROR,
        "wrong request",
    ))
    .expect("wrong response should serialize");
    let matching = serde_json::to_string(&server_error(
        Some("discover-1"),
        ErrorCode::METHOD_NOT_FOUND,
        "method not found",
    ))
    .expect("matching response should serialize");
    let mut events = stream::iter([
        Ok::<_, sse_stream::Error>(Sse::default().data(wrong)),
        Ok(Sse::default().data(matching)),
    ])
    .boxed();

    let response = next_correlated_discovery_response(&request, &mut events)
        .await
        .expect("matching response should be returned");
    let JsonRpcMessage::Error(response) = response else {
        panic!("matching response should retain its JSON-RPC error");
    };
    assert_eq!(
        response.id,
        Some(RequestId::String("discover-1".to_string().into()))
    );
}

#[test]
fn server_body_previews_redact_credentials() {
    let preview =
        body_preview("authorization: Bearer abcdefghijklmnopsecret\napi_key=supersecretvalue");

    assert_eq!(
        preview,
        "authorization: Bearer [REDACTED_SECRET]\napi_key=[REDACTED_SECRET]"
    );
}

#[test]
fn event_terminators_reset_the_size_limit() {
    let mut limit = SseEventSizeLimit::new(Some(8));

    limit
        .observe(b"data: a\n\ndata: b\n\n")
        .expect("events must have independent size limits");

    assert_eq!((limit.retained_bytes, limit.line_bytes), (0, 0));
}

#[test]
fn oversized_events_are_rejected_permanently() {
    let mut limit = SseEventSizeLimit::new(Some(8));

    let first = limit
        .observe(b"data: abc")
        .expect_err("an oversized event must be rejected");
    let second = limit
        .observe(b"")
        .expect_err("a rejected event must not resume");

    assert_eq!(
        (first.kind(), first.to_string(), second.kind()),
        (
            ErrorKind::InvalidData,
            "MCP response body exceeds 8 bytes".to_string(),
            ErrorKind::InvalidData,
        )
    );
}

#[test]
fn legacy_sse_streams_remain_unlimited() {
    let mut limit = SseEventSizeLimit::new(/*maximum_bytes*/ None);

    limit
        .observe(&[b'x'; 128])
        .expect("legacy SSE streams must not gain a message limit");

    assert_eq!(
        (
            limit.retained_bytes,
            limit.line_bytes,
            limit.line_is_comment,
            limit.previous_was_carriage_return,
            limit.failed,
        ),
        (0, 0, false, false, false)
    );
}

#[test]
fn protocol_headers_preserve_utf8_values() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-plugin-name",
        HeaderValue::from_str("café").expect("valid HTTP field value"),
    );

    assert_eq!(
        protocol_headers(&headers),
        vec![HttpHeader {
            name: "x-plugin-name".to_string(),
            value: "café".to_string(),
        }]
    );
}
