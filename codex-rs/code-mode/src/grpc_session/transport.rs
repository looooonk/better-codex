use std::io;
use std::net::IpAddr;
use std::sync::Arc;

use codex_code_mode_protocol::grpc::code_mode_host_client::CodeModeHostClient;
use codex_code_mode_protocol::grpc::CAPABILITY_METADATA_KEY;
use codex_code_mode_protocol::grpc::CLIENT_ID_METADATA_KEY;
use codex_code_mode_protocol::grpc::MAX_APPLICATION_MESSAGE_BYTES;
use codex_http_client::build_reqwest_client_with_custom_ca;
use codex_http_client::ClientRouteClass;
use codex_http_client::HttpClientFactory;
use http_body_util::BodyExt;
use tonic::body::Body;
use tonic::codegen::http::Request;
use tonic::codegen::http::Response;
use tonic::codegen::http::HeaderValue;
use tonic::codegen::http::Uri;
use tonic::transport::Channel;
use tonic::transport::Endpoint;
use tokio::sync::Semaphore;
use tower::ServiceExt;
use tower::service_fn;
use tower::util::BoxCloneSyncService;
use uuid::Uuid;

use super::GrpcClient;
use super::GrpcCodeModeHostCapability;
use super::response_admission::ResponseAdmission;

const MAX_ENDPOINT_BYTES: usize = 2_048;
pub(super) const MAX_CLEANUP_TASKS: usize = 32;

pub(super) type GrpcTransport = BoxCloneSyncService<Request<Body>, Response<Body>, io::Error>;

pub(super) struct SharedTransport {
    endpoint: TransportEndpoint,
    client_id: Uuid,
    client: tokio::sync::OnceCell<GrpcClient>,
    callback_tasks: Arc<Semaphore>,
    callback_bytes: Arc<Semaphore>,
    response_admission: ResponseAdmission,
    cleanup_tasks: Arc<Semaphore>,
    capability: Option<GrpcCodeModeHostCapability>,
}

enum TransportEndpoint {
    Url {
        endpoint: String,
        http_client_factory: HttpClientFactory,
    },
    Connected(Channel),
}

impl SharedTransport {
    pub(super) fn new(
        endpoint: String,
        http_client_factory: HttpClientFactory,
        capability: Option<GrpcCodeModeHostCapability>,
    ) -> Self {
        Self {
            endpoint: TransportEndpoint::Url {
                endpoint,
                http_client_factory,
            },
            client_id: Uuid::new_v4(),
            client: tokio::sync::OnceCell::new(),
            callback_tasks: Arc::new(Semaphore::new(super::callbacks::MAX_CALLBACK_TASKS)),
            callback_bytes: Arc::new(Semaphore::new(super::callbacks::MAX_CALLBACK_BYTES)),
            response_admission: ResponseAdmission::new(),
            cleanup_tasks: Arc::new(Semaphore::new(MAX_CLEANUP_TASKS)),
            capability,
        }
    }

    pub(super) fn with_channel(channel: Channel) -> Self {
        Self {
            endpoint: TransportEndpoint::Connected(channel),
            client_id: Uuid::new_v4(),
            client: tokio::sync::OnceCell::new(),
            callback_tasks: Arc::new(Semaphore::new(super::callbacks::MAX_CALLBACK_TASKS)),
            callback_bytes: Arc::new(Semaphore::new(super::callbacks::MAX_CALLBACK_BYTES)),
            response_admission: ResponseAdmission::new(),
            cleanup_tasks: Arc::new(Semaphore::new(MAX_CLEANUP_TASKS)),
            capability: None,
        }
    }

    pub(super) fn callback_tasks(&self) -> Arc<Semaphore> {
        Arc::clone(&self.callback_tasks)
    }

    pub(super) fn callback_bytes(&self) -> Arc<Semaphore> {
        Arc::clone(&self.callback_bytes)
    }

    pub(super) fn cleanup_tasks(&self) -> Arc<Semaphore> {
        Arc::clone(&self.cleanup_tasks)
    }

    pub(super) async fn client(&self) -> Result<GrpcClient, String> {
        self.client
            .get_or_try_init(|| async {
                let client_id = self.client_id;
                let capability = self.capability.clone();
                let client = match &self.endpoint {
                    TransportEndpoint::Url { endpoint, .. } if endpoint.starts_with("unix:") => {
                        validate_unix_endpoint(endpoint)?;
                        let channel = Endpoint::from_shared(endpoint.clone())
                            .map_err(|_| "invalid gRPC code-mode Unix socket endpoint".to_string())?
                            .connect_lazy();
                        channel_client(
                            channel,
                            client_id,
                            capability,
                            self.response_admission.clone(),
                        )
                    }
                    TransportEndpoint::Url {
                        endpoint,
                        http_client_factory,
                    } => {
                        let target = validate_endpoint(endpoint)?;
                        if target.scheme() == "http" && capability.is_none() {
                            return Err(
                                "plaintext HTTP gRPC code-mode hosts require a server-issued capability"
                                    .to_string(),
                            );
                        }
                        let origin: Uri = target
                            .as_str()
                            .parse()
                            .map_err(|_| "invalid gRPC code-mode host origin".to_string())?;
                        let target_for_client = target.clone();
                        let http_client_factory = http_client_factory.clone();
                        let client = tokio::task::spawn_blocking(move || {
                            build_transport_client(
                                &target_for_client,
                                &http_client_factory,
                                reqwest::Client::builder()
                                    .http2_prior_knowledge()
                                    .redirect(reqwest::redirect::Policy::none()),
                            )
                        })
                        .await
                        .map_err(|error| {
                            format!("gRPC code-mode host transport task failed: {error}")
                        })??;
                        let response_admission = self.response_admission.clone();
                        let transport = service_fn(move |mut request: Request<Body>| {
                            let client = client.clone();
                            let capability = capability.clone();
                            let response_admission = response_admission.clone();
                            async move {
                                let path = request.uri().path().to_string();
                                identify_request(&mut request, client_id, capability.as_ref())?;
                                let request = request.map(|body| {
                                    reqwest::Body::wrap_stream(body.into_data_stream())
                                });
                                let request = reqwest::Request::try_from(request)
                                    .map_err(io::Error::other)?;
                                let response: Response<reqwest::Body> = client
                                    .execute(request)
                                    .await
                                    .map_err(io::Error::other)?
                                    .into();
                                response_admission.wrap(&path, response.map(Body::new))
                            }
                        });
                        CodeModeHostClient::with_origin(
                            BoxCloneSyncService::new(transport),
                            origin,
                        )
                    }
                    TransportEndpoint::Connected(channel) => {
                        channel_client(
                            channel.clone(),
                            client_id,
                            capability,
                            self.response_admission.clone(),
                        )
                    }
                };
                Ok(client
                    .max_decoding_message_size(MAX_APPLICATION_MESSAGE_BYTES)
                    .max_encoding_message_size(MAX_APPLICATION_MESSAGE_BYTES))
            })
            .await
            .cloned()
    }
}

fn channel_client(
    channel: Channel,
    client_id: Uuid,
    capability: Option<GrpcCodeModeHostCapability>,
    response_admission: ResponseAdmission,
) -> GrpcClient {
    let transport = service_fn(move |mut request: Request<Body>| {
        let channel = channel.clone();
        let capability = capability.clone();
        let response_admission = response_admission.clone();
        async move {
            let path = request.uri().path().to_string();
            identify_request(&mut request, client_id, capability.as_ref())?;
            let response = channel.oneshot(request).await.map_err(io::Error::other)?;
            response_admission.wrap(&path, response)
        }
    });
    CodeModeHostClient::new(BoxCloneSyncService::new(transport))
}

fn identify_request<T>(
    request: &mut Request<T>,
    client_id: Uuid,
    capability: Option<&GrpcCodeModeHostCapability>,
) -> Result<(), io::Error> {
    request.headers_mut().insert(
        CLIENT_ID_METADATA_KEY,
        client_id.to_string().parse().map_err(io::Error::other)?,
    );
    if let Some(capability) = capability {
        let mut value: HeaderValue = capability.as_str().parse().map_err(io::Error::other)?;
        value.set_sensitive(true);
        request.headers_mut().insert(
            CAPABILITY_METADATA_KEY,
            value,
        );
    }
    Ok(())
}

fn build_transport_client(
    target: &reqwest::Url,
    http_client_factory: &HttpClientFactory,
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::Client, String> {
    if target
        .host_str()
        .and_then(|host| {
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<IpAddr>()
                .ok()
        })
        .is_some_and(|ip| ip.is_loopback())
    {
        return build_reqwest_client_with_custom_ca(builder.no_proxy()).map_err(|error| {
            format!("failed to configure direct gRPC code-mode host transport: {error}")
        });
    }
    http_client_factory
        .build_reqwest_client(builder, target.as_str(), ClientRouteClass::Other)
        .map_err(|error| format!("failed to configure gRPC code-mode host transport: {error}"))
}

fn validate_unix_endpoint(endpoint: &str) -> Result<(), String> {
    if endpoint.len() > MAX_ENDPOINT_BYTES {
        return Err("gRPC code-mode host URL is too long".to_string());
    }
    let path = endpoint
        .strip_prefix("unix://")
        .or_else(|| endpoint.strip_prefix("unix:"))
        .ok_or_else(|| "invalid gRPC code-mode Unix socket endpoint".to_string())?;
    if !path.starts_with('/') || path.contains('?') || path.contains('#') || path.contains('\0') {
        return Err("invalid gRPC code-mode Unix socket endpoint".to_string());
    }
    Ok(())
}

fn validate_endpoint(endpoint: &str) -> Result<reqwest::Url, String> {
    if endpoint.len() > MAX_ENDPOINT_BYTES {
        return Err("gRPC code-mode host URL is too long".to_string());
    }
    let target = reqwest::Url::parse(endpoint)
        .map_err(|_| "invalid gRPC code-mode host URL".to_string())?;
    if !matches!(target.scheme(), "http" | "https") {
        return Err("gRPC code-mode host URL must use http or https".to_string());
    }
    if !target.username().is_empty() || target.password().is_some() {
        return Err("gRPC code-mode host URL must not include credentials".to_string());
    }
    if target.path() != "/" || target.query().is_some() || target.fragment().is_some() {
        return Err(
            "gRPC code-mode host URL must not include a path, query, or fragment".to_string(),
        );
    }
    if target.scheme() == "http"
        && !target
            .host_str()
            .and_then(|host| {
                host.trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| ip.is_loopback())
    {
        return Err(
            "plaintext gRPC code-mode hosts must use a loopback IP address".to_string(),
        );
    }
    Ok(target)
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
