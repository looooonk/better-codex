use codex_code_mode_protocol::grpc::CLIENT_ID_METADATA_KEY;
use tonic::Request;
use tonic::Status;
use tonic::transport::server::TcpConnectInfo;
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
    LoopbackTcp,
}

impl PrincipalPolicy {
    pub(super) fn principal<T>(&self, request: &Request<T>) -> Result<GrpcPrincipal, Status> {
        match self {
            #[cfg(test)]
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
