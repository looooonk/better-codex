use std::net::SocketAddr;

use futures::StreamExt;
use pretty_assertions::assert_eq;
use tokio::net::TcpStream;
use tonic::transport::server::TcpIncoming;

use super::ListenTransport;
use super::MAX_ACCEPTED_CONNECTIONS;
use super::MAX_AGGREGATE_DECODING_BYTES;
use super::MAX_GLOBAL_DECODING_REQUESTS;
use super::bounded_incoming;
use super::parse_listen_transport;
use codex_code_mode_protocol::host::MAX_FRAME_BYTES;

#[test]
fn listen_url_accepts_stdio_and_loopback_grpc() {
    assert_eq!(parse_listen_transport("stdio").unwrap(), ListenTransport::Stdio);
    assert_eq!(
        parse_listen_transport("grpc://127.0.0.1:0").unwrap(),
        ListenTransport::Grpc("127.0.0.1:0".parse().unwrap())
    );
    assert_eq!(
        parse_listen_transport("grpc://[::1]:45123").unwrap(),
        ListenTransport::Grpc("[::1]:45123".parse().unwrap())
    );
}

#[test]
fn listen_url_rejects_untrusted_authority_and_components_without_echoing_secrets() {
    for value in [
        "grpc://0.0.0.0:45123",
        "grpc://localhost:45123",
        "grpc://alice:super-secret@127.0.0.1:45123",
        "grpc://127.0.0.1:45123/path",
        "grpc://127.0.0.1:45123?secret=super-secret",
        "grpc://127.0.0.1:45123#super-secret",
    ] {
        let error = parse_listen_transport(value).unwrap_err().to_string();
        assert!(!error.contains("super-secret"));
    }
}

#[test]
fn aggregate_decode_admission_is_bounded() {
    assert!(
        MAX_GLOBAL_DECODING_REQUESTS * MAX_FRAME_BYTES <= MAX_AGGREGATE_DECODING_BYTES
    );
}

#[tokio::test]
async fn accepted_connection_limit_releases_on_drop() {
    let incoming = TcpIncoming::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap()).unwrap();
    let address = incoming.local_addr().unwrap();
    let mut bounded = Box::pin(bounded_incoming(incoming));
    let mut clients = Vec::new();
    let mut accepted = Vec::new();
    for _ in 0..MAX_ACCEPTED_CONNECTIONS {
        clients.push(TcpStream::connect(address).await.unwrap());
        accepted.push(bounded.next().await.unwrap().unwrap());
    }

    clients.push(TcpStream::connect(address).await.unwrap());
    let blocked = tokio::time::timeout(std::time::Duration::from_millis(20), bounded.next()).await;
    assert!(blocked.is_err());
    accepted.pop();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), bounded.next())
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
}
