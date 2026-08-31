use std::time::Duration;

use anyhow::Result;
use codex_api::ResponsesApiRequest;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_login::AgentIdentityAuthPolicy;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider::create_model_provider;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SessionSource;
use core_test_support::responses;
use core_test_support::responses::WebSocketConnectionConfig;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_output_text_delta;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::time::Instant;

use super::*;

async fn proxy_websocket_servers(servers: &[&responses::WebSocketTestServer]) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let targets = servers
        .iter()
        .map(|server| server.uri().trim_start_matches("ws://").to_owned())
        .collect::<Vec<_>>();
    tokio::spawn(async move {
        for target in targets {
            let Ok((mut incoming, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let Ok(mut outgoing) = TcpStream::connect(target).await else {
                    return;
                };
                let _ = tokio::io::copy_bidirectional(&mut incoming, &mut outgoing).await;
            });
        }
    });
    Ok(format!("http://{address}/v1"))
}

fn sampler_config(base_url: String) -> LunaSamplerConfig {
    LunaSamplerConfig {
        provider: create_model_provider(
            ModelProviderInfo::create_openai_provider(Some(base_url)),
            Some(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
                "test-api-key",
            ))),
        ),
        http_client_factory: HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        agent_identity_policy: AgentIdentityAuthPolicy::JwtOnly,
        session_source: SessionSource::Exec,
        session_id: "session-1".to_owned(),
        thread_id: "thread-1".to_owned(),
        originator: Some("guardian-v2-test".to_owned()),
        service_tier: None,
    }
}

fn sample_request(deadline: Instant) -> LunaSamplingRequest {
    LunaSamplingRequest {
        request: ResponsesApiRequest {
            model: model().to_string(),
            instructions: String::new(),
            input: vec![ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Return a risk score".to_string(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            }],
            tools: None,
            tool_choice: "none".to_string(),
            parallel_tool_calls: false,
            reasoning: None,
            store: false,
            stream: true,
            stream_options: None,
            include: Vec::new(),
            service_tier: None,
            prompt_cache_key: Some("guardian-v2:thread-1".to_string()),
            text: None,
            client_metadata: Some(request_metadata("session-1", "thread-1", "turn-1")),
        },
        deadline,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sampler_reuses_a_drained_connection() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let scripted_requests = vec![
        vec![
            ev_output_text_delta(r#"{"score":0.25}"#),
            ev_completed("response-1"),
        ],
        vec![
            ev_assistant_message("sample-2", r#"{"score":0.75}"#),
            ev_completed("response-2"),
        ],
    ];
    let idle_server = responses::start_websocket_server(vec![scripted_requests.clone()]).await;
    let server = responses::start_websocket_server(vec![scripted_requests]).await;
    let sampler = LunaSampler::connect(sampler_config(
        proxy_websocket_servers(&[&idle_server, &server]).await?,
    ))
    .await?;

    let first = sampler
        .sample(sample_request(Instant::now() + Duration::from_secs(2)))
        .await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while sampler
            .idle_connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
            < INITIAL_WEBSOCKET_CONNECTIONS
        {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let second = sampler
        .sample(sample_request(Instant::now() + Duration::from_secs(2)))
        .await?;

    assert_eq!(first, r#"{"score":0.25}"#);
    assert_eq!(second, r#"{"score":0.75}"#);
    assert_eq!(server.single_connection().len(), 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_deadline_bounds_pool_lease_stream_and_drain() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let config = WebSocketConnectionConfig {
        requests: vec![Vec::new()],
        response_headers: Vec::new(),
        accept_delay: None,
        close_after_requests: false,
    };
    let idle_server = responses::start_websocket_server_with_headers(vec![config.clone()]).await;
    let server = responses::start_websocket_server_with_headers(vec![config]).await;
    let sampler = LunaSampler::connect(sampler_config(
        proxy_websocket_servers(&[&idle_server, &server]).await?,
    ))
    .await?;

    let error = sampler
        .sample(sample_request(Instant::now() + Duration::from_millis(50)))
        .await
        .expect_err("stream should exceed the single deadline");

    assert!(matches!(error, LunaSamplerError::Deadline));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retryable_stream_error_retries_only_once_on_another_connection() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let healthy = responses::start_websocket_server(vec![vec![vec![
        ev_assistant_message("response-1", r#"{"score":0.25}"#),
        ev_completed("response-1"),
    ]]])
    .await;
    let expired = responses::start_websocket_server(vec![vec![vec![json!({
        "type": "error",
        "status": 400,
        "error": {
            "type": "invalid_request_error",
            "code": "websocket_connection_limit_reached",
            "message": "Responses websocket connection limit reached (60 minutes)."
        }
    })]]])
    .await;
    let sampler = LunaSampler::connect(sampler_config(
        proxy_websocket_servers(&[&healthy, &expired]).await?,
    ))
    .await?;

    let output = sampler
        .sample(sample_request(Instant::now() + Duration::from_secs(2)))
        .await?;

    assert_eq!(output, r#"{"score":0.25}"#);
    assert_eq!(expired.single_connection().len(), 1);
    assert_eq!(healthy.single_connection().len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retryable_stream_error_does_not_reset_the_deadline() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let stalled = responses::start_websocket_server_with_headers(vec![
        WebSocketConnectionConfig {
            requests: vec![Vec::new()],
            response_headers: Vec::new(),
            accept_delay: None,
            close_after_requests: false,
        },
    ])
    .await;
    let expired = responses::start_websocket_server(vec![vec![vec![json!({
        "type": "error",
        "status": 400,
        "error": {
            "type": "invalid_request_error",
            "code": "websocket_connection_limit_reached",
            "message": "Responses websocket connection limit reached (60 minutes)."
        }
    })]]])
    .await;
    let sampler = LunaSampler::connect(sampler_config(
        proxy_websocket_servers(&[&stalled, &expired]).await?,
    ))
    .await?;

    let error = sampler
        .sample(sample_request(Instant::now() + Duration::from_millis(50)))
        .await
        .expect_err("the retry should share the original deadline");

    assert!(matches!(error, LunaSamplerError::Deadline));
    assert_eq!(expired.single_connection().len(), 1);
    Ok(())
}

#[test]
fn output_limit_is_strictly_four_kibibytes() {
    assert!(ensure_output_bound(&"x".repeat(MAX_OUTPUT_BYTES)).is_ok());
    assert!(matches!(
        ensure_output_bound(&"x".repeat(MAX_OUTPUT_BYTES + 1)),
        Err(LunaSamplerError::OutputTooLarge)
    ));
}
