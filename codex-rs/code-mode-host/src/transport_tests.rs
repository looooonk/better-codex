use std::net::SocketAddr;

use futures::StreamExt;
use pretty_assertions::assert_eq;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;
use tonic::Request;
use tonic::transport::Endpoint;
use tonic::transport::Server;
use tonic::transport::server::TcpIncoming;
use uuid::Uuid;

use super::ListenTransport;
use super::MAX_ACCEPTED_CONNECTIONS;
use super::MAX_AGGREGATE_TRANSPORT_BYTES;
use super::bounded_incoming;
use super::parse_listen_transport;
use crate::grpc::authenticated_loopback_grpc_service;
use crate::transport_admission::MAX_DECODED_REQUEST_BYTES;
use crate::transport_admission::MAX_OUTBOUND_RESPONSE_BYTES;
use crate::transport_admission::MAX_RAW_REQUEST_ALLOCATION_BYTES;
use codex_code_mode_protocol::grpc as proto;
use codex_code_mode_protocol::grpc::CAPABILITY_METADATA_KEY;
use codex_code_mode_protocol::grpc::CLIENT_ID_METADATA_KEY;
use codex_code_mode_protocol::grpc::bounded_code_mode_host_client;

const TEST_CAPABILITY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn listen_url_accepts_stdio_and_loopback_grpc() {
    assert_eq!(
        parse_listen_transport(super::DEFAULT_LISTEN_URL).unwrap(),
        ListenTransport::Stdio
    );
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
        MAX_RAW_REQUEST_ALLOCATION_BYTES
            + MAX_DECODED_REQUEST_BYTES
            + MAX_OUTBOUND_RESPONSE_BYTES
            + super::MAX_HTTP2_CONNECTION_WINDOW_BYTES
            + super::MAX_HTTP2_STREAM_WINDOW_BYTES
            + super::MAX_HTTP2_HEADER_BYTES
            + crate::grpc::events::MAX_HOST_EVENT_BYTES
            + crate::grpc::session::MAX_HOST_TOOL_BYTES
            <= MAX_AGGREGATE_TRANSPORT_BYTES
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
    let blocked = tokio::time::timeout(
        std::time::Duration::from_millis(/*millis*/ 20),
        bounded.next(),
    )
    .await;
    assert!(blocked.is_err());
    accepted.pop();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 1), bounded.next(),)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
}

#[tokio::test]
async fn continuous_unauthorized_input_cannot_extend_connection_admission() {
    let incoming = TcpIncoming::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap()).unwrap();
    let address = incoming.local_addr().unwrap();
    let mut bounded = Box::pin(bounded_incoming(incoming));
    let writer = tokio::spawn(async move {
        let mut client = TcpStream::connect(address).await.unwrap();
        loop {
            if client.write_all(&[0; 1_024]).await.is_err() {
                return;
            }
            tokio::task::yield_now().await;
        }
    });
    let mut connection = bounded.next().await.unwrap().unwrap();
    let error = tokio::time::timeout(
        super::CONNECTION_FIRST_BYTE_TIMEOUT + std::time::Duration::from_secs(/*secs*/ 1),
        async {
            let mut buffer = [0; 1_024];
            loop {
                if let Err(error) = connection.read_exact(&mut buffer).await {
                    break error;
                }
            }
        },
    )
    .await
    .expect("unauthorized connection should reach its absolute deadline");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    writer.abort();
}

#[tokio::test]
async fn unauthenticated_connections_expire_before_starving_a_provider() {
    let incoming = TcpIncoming::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap()).unwrap();
    let address = incoming.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(authenticated_loopback_grpc_service(TEST_CAPABILITY.into()))
            .serve_with_incoming_shutdown(
                bounded_incoming(incoming),
                server_shutdown.cancelled_owned(),
            )
            .await
    });
    let mut silent = Vec::new();
    for index in 0..MAX_ACCEPTED_CONNECTIONS {
        let mut connection = TcpStream::connect(address).await.unwrap();
        if index % 3 == 1 {
            connection.write_all(b"P").await.unwrap();
        } else if index % 3 == 2 {
            connection
                .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\0\0\0\x04\0\0\0\0\0")
                .await
                .unwrap();
        }
        silent.push(connection);
    }
    tokio::time::sleep(std::time::Duration::from_millis(/*millis*/ 20)).await;

    let channel = tokio::time::timeout(
        super::CONNECTION_FIRST_BYTE_TIMEOUT + std::time::Duration::from_secs(/*secs*/ 2),
        Endpoint::from_shared(format!("http://{address}"))
            .unwrap()
            .connect(),
    )
    .await
    .expect("silent connection admission should expire")
    .expect("provider should complete its HTTP/2 handshake");
    let mut client = bounded_code_mode_host_client(channel);
    let client_id = Uuid::new_v4();
    let mut request = Request::new(proto::OpenSessionRequest {
        cell_execution_limits: None,
    });
    request.metadata_mut().insert(
        CLIENT_ID_METADATA_KEY,
        client_id.to_string().parse().unwrap(),
    );
    request
        .metadata_mut()
        .insert(CAPABILITY_METADATA_KEY, TEST_CAPABILITY.parse().unwrap());
    let mut events = client.open_session(request).await.unwrap().into_inner();
    assert!(matches!(
        events.message().await.unwrap().unwrap().event,
        Some(proto::session_event::Event::Opened(_))
    ));

    drop(events);
    drop(silent);
    shutdown.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn invalid_capabilities_do_not_extend_connection_admission() {
    let incoming = TcpIncoming::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap()).unwrap();
    let address = incoming.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(authenticated_loopback_grpc_service(TEST_CAPABILITY.into()))
            .serve_with_incoming_shutdown(
                bounded_incoming(incoming),
                server_shutdown.cancelled_owned(),
            )
            .await
    });
    let mut invalid_clients = Vec::new();
    for _ in 0..MAX_ACCEPTED_CONNECTIONS {
        let channel = Endpoint::from_shared(format!("http://{address}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        let mut client = bounded_code_mode_host_client(channel);
        let mut request = Request::new(proto::OpenSessionRequest {
            cell_execution_limits: None,
        });
        request.metadata_mut().insert(
            CLIENT_ID_METADATA_KEY,
            Uuid::new_v4().to_string().parse().unwrap(),
        );
        request.metadata_mut().insert(
            CAPABILITY_METADATA_KEY,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            client.open_session(request).await.unwrap_err().code(),
            tonic::Code::Unauthenticated
        );
        invalid_clients.push(client);
    }

    let channel = tokio::time::timeout(
        super::CONNECTION_FIRST_BYTE_TIMEOUT + std::time::Duration::from_secs(/*secs*/ 2),
        Endpoint::from_shared(format!("http://{address}"))
            .unwrap()
            .connect(),
    )
    .await
    .expect("unauthorized connections should expire")
    .expect("provider should complete its HTTP/2 handshake");
    let mut client = bounded_code_mode_host_client(channel);
    let mut request = Request::new(proto::OpenSessionRequest {
        cell_execution_limits: None,
    });
    request.metadata_mut().insert(
        CLIENT_ID_METADATA_KEY,
        Uuid::new_v4().to_string().parse().unwrap(),
    );
    request
        .metadata_mut()
        .insert(CAPABILITY_METADATA_KEY, TEST_CAPABILITY.parse().unwrap());
    let mut events = client.open_session(request).await.unwrap().into_inner();
    assert!(matches!(
        events.message().await.unwrap().unwrap().event,
        Some(proto::session_event::Event::Opened(_))
    ));

    drop(events);
    drop(invalid_clients);
    shutdown.cancel();
    server.await.unwrap().unwrap();
}
