use codex_code_mode_protocol::grpc::CAPABILITY_METADATA_KEY;
use codex_code_mode_protocol::grpc::CLIENT_ID_METADATA_KEY;
use std::sync::Arc;
use tonic::Request;
use tonic::Status;
use tonic::transport::server::TcpConnectInfo;
#[cfg(unix)]
use tonic::transport::server::UdsConnectInfo;
use uuid::Uuid;

use crate::transport::BoundedConnectInfo;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GrpcPrincipal {
    InProcess,
    LoopbackClient(Uuid),
}

#[derive(Debug)]
pub(super) struct RequestPrincipal {
    principal: GrpcPrincipal,
    connection: Option<BoundedConnectInfo>,
}

impl RequestPrincipal {
    pub(super) fn identity(&self) -> GrpcPrincipal {
        self.principal
    }

    pub(super) fn authorize(self) {
        if let Some(connection) = self.connection {
            connection.authenticate();
        }
    }
}

#[derive(Clone)]
pub(super) enum PrincipalPolicy {
    InProcess,
    TrustedLocalTransport,
    AuthenticatedLocalTransport(Arc<str>),
}

impl PrincipalPolicy {
    pub(super) fn principal<T>(&self, request: &Request<T>) -> Result<RequestPrincipal, Status> {
        match self {
            Self::InProcess => Ok(RequestPrincipal {
                principal: GrpcPrincipal::InProcess,
                connection: None,
            }),
            Self::TrustedLocalTransport | Self::AuthenticatedLocalTransport(_) => {
                let bounded = request.extensions().get::<BoundedConnectInfo>();
                let remote_addr = bounded
                    .and_then(BoundedConnectInfo::remote_addr)
                    .or_else(|| {
                        request
                            .extensions()
                            .get::<TcpConnectInfo>()
                            .and_then(TcpConnectInfo::remote_addr)
                    });
                if remote_addr.is_some_and(|address| !address.ip().is_loopback()) {
                    return Err(Status::permission_denied(
                        "code-mode gRPC is restricted to loopback callers",
                    ));
                }
                #[cfg(unix)]
                let is_unix = request.extensions().get::<UdsConnectInfo>().is_some();
                #[cfg(not(unix))]
                let is_unix = false;
                if remote_addr.is_none() && !is_unix {
                    return Err(Status::unauthenticated(
                        "code-mode gRPC requests require a bound local caller",
                    ));
                }
                if let Self::AuthenticatedLocalTransport(expected) = self {
                    let actual = request
                        .metadata()
                        .get(CAPABILITY_METADATA_KEY)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default();
                    if !constant_time_matches(expected.as_bytes(), actual.as_bytes()) {
                        return Err(Status::unauthenticated(
                            "code-mode gRPC capability is missing or invalid",
                        ));
                    }
                }
                let client_id = request
                    .metadata()
                    .get(CLIENT_ID_METADATA_KEY)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| Uuid::parse_str(value).ok())
                    .ok_or_else(|| {
                        Status::unauthenticated(
                            "code-mode gRPC requests require a valid client identity",
                        )
                    })?;
                Ok(RequestPrincipal {
                    principal: GrpcPrincipal::LoopbackClient(client_id),
                    connection: bounded.cloned(),
                })
            }
        }
    }
}

pub(crate) fn constant_time_matches(expected: &[u8], actual: &[u8]) -> bool {
    let mut difference = expected.len() ^ actual.len();
    for (index, expected) in expected.iter().enumerate() {
        difference |= usize::from(*expected ^ actual.get(index).copied().unwrap_or_default());
    }
    difference == 0
}
