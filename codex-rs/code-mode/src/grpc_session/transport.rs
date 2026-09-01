use std::io;
use std::net::IpAddr;

use codex_code_mode_protocol::grpc::code_mode_host_client::CodeModeHostClient;
use codex_code_mode_protocol::grpc::CLIENT_ID_METADATA_KEY;
use codex_code_mode_protocol::host::MAX_FRAME_BYTES;
use codex_http_client::ClientRouteClass;
use codex_http_client::HttpClientFactory;
use http_body_util::BodyExt;
use tonic::body::Body;
use tonic::codegen::http::Request;
use tonic::codegen::http::Response;
use tonic::codegen::http::Uri;
use tower::service_fn;
use tower::util::BoxCloneSyncService;
use uuid::Uuid;

use super::GrpcClient;

const MAX_ENDPOINT_BYTES: usize = 2_048;

pub(super) type GrpcTransport = BoxCloneSyncService<Request<Body>, Response<Body>, io::Error>;

pub(super) struct SharedTransport {
    endpoint: String,
    http_client_factory: HttpClientFactory,
    client_id: Uuid,
    client: tokio::sync::OnceCell<GrpcClient>,
}

impl SharedTransport {
    pub(super) fn new(endpoint: String, http_client_factory: HttpClientFactory) -> Self {
        Self {
            endpoint,
            http_client_factory,
            client_id: Uuid::new_v4(),
            client: tokio::sync::OnceCell::new(),
        }
    }

    pub(super) async fn client(&self) -> Result<GrpcClient, String> {
        self.client
            .get_or_try_init(|| async {
                let target = validate_endpoint(&self.endpoint)?;
                let origin: Uri = target
                    .as_str()
                    .parse()
                    .map_err(|_| "invalid gRPC code-mode host origin".to_string())?;
                let endpoint = target.to_string();
                let client_id = self.client_id;
                let http_client_factory = self.http_client_factory.clone();
                let client = tokio::task::spawn_blocking(move || {
                    http_client_factory
                        .build_reqwest_client(
                            reqwest::Client::builder()
                                .http2_prior_knowledge()
                                .redirect(reqwest::redirect::Policy::none()),
                            &endpoint,
                            ClientRouteClass::Other,
                        )
                        .map_err(|error| {
                            format!("failed to configure gRPC code-mode host transport: {error}")
                        })
                })
                .await
                .map_err(|error| format!("gRPC code-mode host transport task failed: {error}"))??;
                let transport = service_fn(move |mut request: Request<Body>| {
                    let client = client.clone();
                    async move {
                        let client_id = client_id.to_string().parse().map_err(io::Error::other)?;
                        request
                            .headers_mut()
                            .insert(CLIENT_ID_METADATA_KEY, client_id);
                        let request = request
                            .map(|body| reqwest::Body::wrap_stream(body.into_data_stream()));
                        let request =
                            reqwest::Request::try_from(request).map_err(io::Error::other)?;
                        let response: Response<reqwest::Body> =
                            client.execute(request).await.map_err(io::Error::other)?.into();
                        Ok::<_, io::Error>(response.map(Body::new))
                    }
                });
                let client =
                    CodeModeHostClient::with_origin(BoxCloneSyncService::new(transport), origin);
                Ok(client
                    .max_decoding_message_size(MAX_FRAME_BYTES)
                    .max_encoding_message_size(MAX_FRAME_BYTES))
            })
            .await
            .cloned()
    }
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
