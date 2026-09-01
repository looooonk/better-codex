use std::io;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use anyhow::Context as _;
use anyhow::Result;
use codex_code_mode_protocol::host::MAX_FRAME_BYTES;
use futures::Stream;
use futures::StreamExt;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tonic::transport::Server;
use tonic::transport::server::Connected;
use tonic::transport::server::TcpConnectInfo;
use tonic::transport::server::TcpIncoming;
use tower::limit::GlobalConcurrencyLimitLayer;
use url::Url;

use crate::loopback_grpc_service;
use crate::run_stdio;

const MAX_ACCEPTED_CONNECTIONS: usize = 16;
const MAX_CONCURRENT_STREAMS_PER_CONNECTION: u32 = 64;
const MAX_CONCURRENT_REQUESTS_PER_CONNECTION: usize = 4;
const MAX_GLOBAL_DECODING_REQUESTS: usize = 4;
const MAX_AGGREGATE_DECODING_BYTES: usize = 256 * 1_024 * 1_024;

const _: () = assert!(MAX_GLOBAL_DECODING_REQUESTS * MAX_FRAME_BYTES <= MAX_AGGREGATE_DECODING_BYTES);

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
        .and_then(|host| host.parse::<IpAddr>().ok())
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
        .with_nodelay(Some(true));
    let local_address = incoming
        .local_addr()
        .context("failed to read code-mode gRPC listener address")?;
    let mut stdout = tokio::io::stdout();
    stdout
        .write_all(format!("http://{local_address}\n").as_bytes())
        .await
        .context("failed to publish code-mode gRPC endpoint")?;
    stdout
        .flush()
        .await
        .context("failed to flush code-mode gRPC endpoint")?;

    Server::builder()
        .layer(GlobalConcurrencyLimitLayer::new(
            MAX_GLOBAL_DECODING_REQUESTS,
        ))
        .concurrency_limit_per_connection(MAX_CONCURRENT_REQUESTS_PER_CONNECTION)
        .load_shed(true)
        .max_concurrent_streams(Some(MAX_CONCURRENT_STREAMS_PER_CONNECTION))
        .add_service(loopback_grpc_service())
        .serve_with_incoming(bounded_incoming(incoming))
        .await
        .context("code-mode gRPC listener failed")
}

fn bounded_incoming(
    incoming: TcpIncoming,
) -> impl Stream<Item = io::Result<BoundedTcpConnection>> + Send + 'static {
    let permits = Arc::new(Semaphore::new(MAX_ACCEPTED_CONNECTIONS));
    futures::stream::unfold((incoming, permits), |(mut incoming, permits)| async move {
        let permit = Arc::clone(&permits).acquire_owned().await.ok()?;
        let next = incoming.next().await?;
        let item = next.map(|stream| BoundedTcpConnection {
            stream,
            _permit: permit,
        });
        Some((item, (incoming, permits)))
    })
}

struct BoundedTcpConnection {
    stream: TcpStream,
    _permit: OwnedSemaphorePermit,
}

impl AsyncRead for BoundedTcpConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
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
    type ConnectInfo = TcpConnectInfo;

    fn connect_info(&self) -> Self::ConnectInfo {
        self.stream.connect_info()
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
