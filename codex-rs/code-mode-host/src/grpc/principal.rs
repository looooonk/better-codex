use codex_code_mode_protocol::grpc::CLIENT_ID_METADATA_KEY;
use tonic::Request;
use tonic::Status;
use tonic::transport::server::TcpConnectInfo;
#[cfg(unix)]
use tonic::transport::server::UdsConnectInfo;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GrpcPrincipal {
    #[cfg(test)]
    InProcess,
    LoopbackClient(Uuid),
}

#[derive(Clone, Copy)]
pub(super) enum PrincipalPolicy {
    #[cfg(test)]
    InProcess,
    LocalTransport,
}

impl PrincipalPolicy {
    pub(super) fn principal<T>(&self, request: &Request<T>) -> Result<GrpcPrincipal, Status> {
        match self {
            #[cfg(test)]
            Self::InProcess => Ok(GrpcPrincipal::InProcess),
            Self::LocalTransport => {
                let remote_addr = request
                    .extensions()
                    .get::<TcpConnectInfo>()
                    .and_then(TcpConnectInfo::remote_addr);
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
                Ok(GrpcPrincipal::LoopbackClient(client_id))
            }
        }
    }
}
