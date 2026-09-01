use std::time::Duration;

use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use pretty_assertions::assert_eq;
use tokio::net::TcpListener;
use tokio::time::timeout;
use uuid::Uuid;

use super::SharedTransport;
use super::build_transport_client;
use super::identify_request;
use super::validate_endpoint;
use super::validate_unix_endpoint;
use crate::grpc_session::GrpcCodeModeHostCapability;
use codex_code_mode_protocol::grpc::CAPABILITY_METADATA_KEY;

#[test]
fn endpoint_accepts_loopback_http_and_https() {
    assert_eq!(
        validate_endpoint("http://127.0.0.1:45123")
            .unwrap()
            .as_str(),
        "http://127.0.0.1:45123/"
    );
    assert_eq!(
        validate_endpoint("https://code-mode.example:45123")
            .unwrap()
            .as_str(),
        "https://code-mode.example:45123/"
    );
    assert_eq!(
        validate_endpoint("http://[::1]:45123").unwrap().as_str(),
        "http://[::1]:45123/"
    );
}

#[test]
fn endpoint_rejects_plaintext_remote_hosts() {
    for endpoint in [
        "http://example.com:45123",
        "http://localhost:45123",
        "http://192.0.2.1:45123",
        "http://[2001:db8::1]:45123",
    ] {
        assert_eq!(
            validate_endpoint(endpoint).unwrap_err(),
            "plaintext gRPC code-mode hosts must use a loopback IP address"
        );
    }
}

#[test]
fn endpoint_rejects_components_without_echoing_secrets() {
    for endpoint in [
        "http://alice:super-secret@127.0.0.1:45123",
        "http://127.0.0.1:45123/super-secret",
        "http://127.0.0.1:45123?token=super-secret",
        "http://127.0.0.1:45123#super-secret",
        "not-a-url-super-secret",
    ] {
        let error = validate_endpoint(endpoint).unwrap_err();
        assert!(!error.contains("super-secret"));
    }
}

#[test]
fn unix_endpoints_require_bounded_absolute_paths() {
    for endpoint in ["unix:/tmp/code-mode.sock", "unix:///tmp/code-mode.sock"] {
        assert_eq!(validate_unix_endpoint(endpoint), Ok(()));
    }
    for endpoint in [
        "unix:relative.sock",
        "unix://relative.sock",
        "unix:/tmp/code-mode.sock?token=super-secret",
        "unix:/tmp/code-mode.sock#super-secret",
    ] {
        let error = validate_unix_endpoint(endpoint).unwrap_err();
        assert!(!error.contains("super-secret"));
    }
}

#[test]
fn capability_header_is_marked_sensitive() {
    let capability = GrpcCodeModeHostCapability::new("a1".repeat(32)).unwrap();
    let mut request = tonic::codegen::http::Request::new(());
    identify_request(&mut request, Uuid::new_v4(), Some(&capability)).unwrap();

    assert!(request.headers()[CAPABILITY_METADATA_KEY].is_sensitive());
}

#[tokio::test]
async fn loopback_transports_ignore_configured_proxy() {
    for scheme in ["http", "https"] {
        let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target =
            reqwest::Url::parse(&format!("{scheme}://{}", destination.local_addr().unwrap()))
                .unwrap();
        let client = build_transport_client(
            &target,
            &HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
            reqwest::Client::builder().http2_prior_knowledge().proxy(
                reqwest::Proxy::all(format!("http://{}", proxy.local_addr().unwrap())).unwrap(),
            ),
        )
        .unwrap();
        let request = tokio::spawn(async move { client.get(target).send().await });

        let accepted_directly = timeout(Duration::from_secs(/*secs*/ 1), async {
            tokio::select! {
                result = destination.accept() => {
                    result.unwrap();
                    true
                }
                result = proxy.accept() => {
                    result.unwrap();
                    false
                }
            }
        })
        .await
        .unwrap();
        assert!(accepted_directly);
        request.abort();
    }
}

#[tokio::test]
async fn endpoint_only_http_transport_is_rejected_before_connecting() {
    let transport = SharedTransport::new(
        "http://127.0.0.1:1".to_string(),
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        /*capability*/ None,
    );

    let Err(error) = transport.client().await else {
        panic!("endpoint-only HTTP transport should fail");
    };
    assert_eq!(
        error,
        "plaintext HTTP gRPC code-mode hosts require a server-issued capability"
    );
}

#[tokio::test]
async fn endpoint_only_https_transport_remains_available_for_trusted_deployments() {
    let transport = SharedTransport::new(
        "https://code-mode.example".to_string(),
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        /*capability*/ None,
    );

    assert!(transport.client().await.is_ok());
}
