use std::io;
use std::future::Future as _;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use anyhow::Context as _;
use anyhow::Result;
use futures::Stream;
use futures::StreamExt;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::time::Sleep;
use tonic::transport::Server;
use tonic::transport::server::Connected;
use tonic::transport::server::TcpConnectInfo;
use tonic::transport::server::TcpIncoming;
use url::Url;
use uuid::Uuid;

use crate::grpc::authenticated_loopback_grpc_service;
use crate::run_stdio;
use crate::grpc::session::MAX_OPEN_GRPC_SESSIONS;
use crate::grpc::routing::MAX_SUBSCRIPTIONS_PER_SESSION;
use crate::transport_admission::MAX_DECODED_REQUEST_BYTES;
use crate::transport_admission::MAX_OUTBOUND_RESPONSE_BYTES;
use crate::transport_admission::MAX_RAW_REQUEST_ALLOCATION_BYTES;
use crate::transport_admission::MAX_STREAMING_RESPONSES;
use crate::transport_admission::MAX_UNARY_RESPONSES;
use crate::transport_admission::MAX_OPEN_RESPONSES;
use crate::transport_admission::MAX_SUBSCRIBE_RESPONSES;

const MAX_ACCEPTED_CONNECTIONS: usize = 8;
const MAX_CONCURRENT_STREAMS_PER_CONNECTION: u32 = 32;
const MAX_AGGREGATE_TRANSPORT_BYTES: usize = 256 * 1_024 * 1_024;
const HTTP2_STREAM_WINDOW_BYTES: u32 = 64 * 1_024;
const HTTP2_CONNECTION_WINDOW_BYTES: u32 = 512 * 1_024;
const HTTP2_MAX_HEADER_BYTES: u32 = 8 * 1_024;
const HTTP2_MAX_FRAME_BYTES: u32 = 16 * 1_024;
const CONNECTION_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(/*secs*/ 30);
const CONNECTION_KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 10);
pub(crate) const CONNECTION_FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 2);

pub const DEFAULT_LISTEN_URL: &str = "stdio";

const MAX_HTTP2_CONNECTION_WINDOW_BYTES: usize =
    MAX_ACCEPTED_CONNECTIONS * HTTP2_CONNECTION_WINDOW_BYTES as usize;
const MAX_HTTP2_STREAM_WINDOW_BYTES: usize = MAX_ACCEPTED_CONNECTIONS
    * MAX_CONCURRENT_STREAMS_PER_CONNECTION as usize
    * HTTP2_STREAM_WINDOW_BYTES as usize;
const MAX_HTTP2_HEADER_BYTES: usize = MAX_ACCEPTED_CONNECTIONS
    * MAX_CONCURRENT_STREAMS_PER_CONNECTION as usize
    * HTTP2_MAX_HEADER_BYTES as usize;
const RESERVED_CONTROL_STREAMS: usize = MAX_UNARY_RESPONSES;
const _: () = assert!(
    MAX_RAW_REQUEST_ALLOCATION_BYTES
        + MAX_DECODED_REQUEST_BYTES
        + MAX_OUTBOUND_RESPONSE_BYTES
        + MAX_HTTP2_CONNECTION_WINDOW_BYTES
        + MAX_HTTP2_STREAM_WINDOW_BYTES
        + MAX_HTTP2_HEADER_BYTES
        + crate::grpc::events::MAX_HOST_EVENT_BYTES
        + crate::grpc::session::MAX_HOST_TOOL_BYTES
        <= MAX_AGGREGATE_TRANSPORT_BYTES
);
const _: () = assert!(
    MAX_STREAMING_RESPONSES + RESERVED_CONTROL_STREAMS
        <= MAX_CONCURRENT_STREAMS_PER_CONNECTION as usize
);
const _: () = assert!(MAX_OPEN_GRPC_SESSIONS <= MAX_OPEN_RESPONSES);
const _: () = assert!(
    MAX_OPEN_GRPC_SESSIONS * MAX_SUBSCRIPTIONS_PER_SESSION
        <= MAX_SUBSCRIBE_RESPONSES
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ListenTransport {
    Stdio,
    Grpc(SocketAddr),
}

/// Runs the code-mode host on standard I/O or a bounded loopback gRPC listener.
pub async fn run_transport(listen: &str) -> Result<()> {
    match parse_listen_transport(listen)? {
        ListenTransport::Stdio => run_stdio().await,
        ListenTransport::Grpc(address) => run_grpc(address).await,
    }
}

fn parse_listen_transport(listen: &str) -> Result<ListenTransport> {
    if matches!(listen, "stdio" | "stdio://") {
        return Ok(ListenTransport::Stdio);
    }
    let authority = listen
        .strip_prefix("grpc://")
        .ok_or_else(|| anyhow::anyhow!("listen URL must use stdio or grpc"))?;
    if authority.is_empty()
        || authority.contains('/')
        || authority.contains('?')
        || authority.contains('#')
    {
        anyhow::bail!("gRPC listen URL must contain only an IP address and port");
    }
    let url = Url::parse(listen).map_err(|_| anyhow::anyhow!("invalid gRPC listen URL"))?;
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("gRPC listen URL must not contain credentials");
    }
    let address = url
        .host_str()
        .and_then(|host| {
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<IpAddr>()
                .ok()
        })
        .zip(url.port())
        .map(|(ip, port)| SocketAddr::new(ip, port))
        .ok_or_else(|| anyhow::anyhow!("gRPC listen URL requires an IP address and port"))?;
    if !address.ip().is_loopback() {
        anyhow::bail!("gRPC listen URL must use a loopback address");
    }
    Ok(ListenTransport::Grpc(address))
}

async fn run_grpc(address: SocketAddr) -> Result<()> {
    let incoming = TcpIncoming::bind(address)
        .context("failed to bind code-mode gRPC listener")?
        .with_nodelay(/*nodelay*/ Some(true))
        .with_keepalive(Some(CONNECTION_KEEPALIVE_INTERVAL));
    let local_address = incoming
        .local_addr()
        .context("failed to read code-mode gRPC listener address")?;
    let capability: Arc<str> = format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
    .into();
    let mut stdout = tokio::io::stdout();
    stdout
        .write_all(format!("http://{local_address}\n").as_bytes())
        .await
        .context("failed to publish code-mode gRPC endpoint")?;
    stdout
        .write_all(capability.as_bytes())
        .await
        .context("failed to publish code-mode gRPC capability")?;
    stdout
        .write_all(b"\n")
        .await
        .context("failed to terminate code-mode gRPC capability")?;
    stdout
        .flush()
        .await
        .context("failed to flush code-mode gRPC endpoint")?;

    Server::builder()
        .max_concurrent_streams(Some(MAX_CONCURRENT_STREAMS_PER_CONNECTION))
        .initial_stream_window_size(Some(HTTP2_STREAM_WINDOW_BYTES))
        .initial_connection_window_size(Some(HTTP2_CONNECTION_WINDOW_BYTES))
        .http2_adaptive_window(/*enabled*/ Some(false))
        .http2_keepalive_interval(Some(CONNECTION_KEEPALIVE_INTERVAL))
        .http2_keepalive_timeout(Some(CONNECTION_KEEPALIVE_TIMEOUT))
        .http2_max_pending_accept_reset_streams(/*max*/ Some(16))
        .http2_max_local_error_reset_streams(/*max*/ Some(16))
        .http2_max_header_list_size(Some(HTTP2_MAX_HEADER_BYTES))
        .max_frame_size(Some(HTTP2_MAX_FRAME_BYTES))
        .add_service(authenticated_loopback_grpc_service(capability))
        .serve_with_incoming(bounded_incoming(incoming))
        .await
        .context("code-mode gRPC listener failed")
}

pub(crate) fn bounded_incoming(
    incoming: TcpIncoming,
) -> impl Stream<Item = io::Result<BoundedTcpConnection>> + Send + 'static {
    let permits = Arc::new(Semaphore::new(MAX_ACCEPTED_CONNECTIONS));
    futures::stream::unfold((incoming, permits), |(mut incoming, permits)| async move {
        let permit = Arc::clone(&permits).acquire_owned().await.ok()?;
        let next = incoming.next().await?;
        let item = next.map(|stream| BoundedTcpConnection {
            stream,
            authenticated: Arc::new(AtomicBool::new(false)),
            first_byte_timeout: Some(Box::pin(tokio::time::sleep(
                CONNECTION_FIRST_BYTE_TIMEOUT,
            ))),
            _permit: permit,
        });
        Some((item, (incoming, permits)))
    })
}

pub(crate) struct BoundedTcpConnection {
    stream: TcpStream,
    authenticated: Arc<AtomicBool>,
    first_byte_timeout: Option<Pin<Box<Sleep>>>,
    _permit: OwnedSemaphorePermit,
}

impl AsyncRead for BoundedTcpConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.authenticated.load(Ordering::Acquire) {
            self.first_byte_timeout = None;
        } else if self
            .first_byte_timeout
            .as_mut()
            .is_some_and(|timeout| timeout.as_mut().poll(cx).is_ready())
        {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "code-mode gRPC connection was not authorized in time",
            )));
        }
        Pin::new(&mut self.stream).poll_read(cx, buffer)
    }
}

impl AsyncWrite for BoundedTcpConnection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.stream).poll_write(cx, buffer)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.stream).poll_write_vectored(cx, buffers)
    }

    fn is_write_vectored(&self) -> bool {
        self.stream.is_write_vectored()
    }
}

impl Connected for BoundedTcpConnection {
    type ConnectInfo = BoundedConnectInfo;

    fn connect_info(&self) -> Self::ConnectInfo {
        BoundedConnectInfo {
            tcp: self.stream.connect_info(),
            authenticated: Arc::clone(&self.authenticated),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BoundedConnectInfo {
    tcp: TcpConnectInfo,
    authenticated: Arc<AtomicBool>,
}

impl BoundedConnectInfo {
    pub(crate) fn remote_addr(&self) -> Option<SocketAddr> {
        self.tcp.remote_addr()
    }

    pub(crate) fn authenticate(&self) {
        self.authenticated.store(true, Ordering::Release);
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
