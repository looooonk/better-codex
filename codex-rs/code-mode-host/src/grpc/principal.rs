use std::net::SocketAddr;

use tonic::Request;
use tonic::Status;
use tonic::transport::server::TcpConnectInfo;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GrpcPrincipal {
    InProcess,
    LoopbackTcp(SocketAddr),
}

#[derive(Clone, Copy)]
pub(super) enum PrincipalPolicy {
    InProcess,
    LoopbackTcp,
}

impl PrincipalPolicy {
    pub(super) fn principal<T>(&self, request: &Request<T>) -> Result<GrpcPrincipal, Status> {
        match self {
            Self::InProcess => Ok(GrpcPrincipal::InProcess),
            Self::LoopbackTcp => {
                let remote_addr = request
                    .extensions()
                    .get::<TcpConnectInfo>()
                    .and_then(TcpConnectInfo::remote_addr)
                    .ok_or_else(|| {
                        Status::unauthenticated(
                            "code-mode gRPC requests require a bound TCP caller",
                        )
                    })?;
                if !remote_addr.ip().is_loopback() {
                    return Err(Status::permission_denied(
                        "code-mode gRPC is restricted to loopback callers",
                    ));
                }
                Ok(GrpcPrincipal::LoopbackTcp(remote_addr))
            }
        }
    }
}
