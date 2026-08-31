use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use codex_api::ResponsesApiRequest;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_login::AgentIdentityAuthPolicy;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::ExternalAuth;
use codex_login::ExternalAuthFuture;
use codex_login::ExternalAuthRefreshContext;
use codex_model_provider::create_model_provider;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::GuardianAssessmentAction;
use codex_protocol::protocol::GuardianAssessmentOutcome;
use codex_protocol::protocol::GuardianRiskLevel;
use codex_protocol::protocol::GuardianUserAuthorization;
use codex_protocol::protocol::SessionSource;
use core_test_support::responses;
use core_test_support::responses::WebSocketConnectionConfig;
use core_test_support::responses::WebSocketTestServer;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_output_text_delta;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::time::Instant;

use super::*;
use crate::request::GuardianReviewAction;
use crate::request::GuardianReviewRequest;
use crate::review::GuardianReviewClient;
use crate::review::GuardianReviewOutcome;

const VALID_OUTCOME: &str = r#"{"score":0.25,"risk_level":"low","user_authorization":"high","outcome":"allow","rationale":"safe"}"#;

struct SamplerFixture {
    sampler: Arc<LunaSampler>,
    _servers: [WebSocketTestServer; 2],
}

#[derive(Clone)]
struct StaticExternalAuth(CodexAuth);

impl ExternalAuth for StaticExternalAuth {
    fn resolve(&self) -> ExternalAuthFuture<'_, CodexAuth> {
        Box::pin(std::future::ready(Ok(self.0.clone())))
    }

    fn refresh(&self, _context: ExternalAuthRefreshContext) -> ExternalAuthFuture<'_, CodexAuth> {
        Box::pin(std::future::ready(Ok(self.0.clone())))
    }
}

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
    sampler_config_with_auth_manager(
        base_url,
        AuthManager::from_auth_for_testing(CodexAuth::from_api_key("test-api-key")),
    )
}

fn sampler_config_with_auth_manager(
    base_url: String,
    auth_manager: Arc<AuthManager>,
) -> LunaSamplerConfig {
    LunaSamplerConfig {
        provider: create_model_provider(
            ModelProviderInfo::create_openai_provider(Some(base_url)),
            Some(auth_manager),
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

fn review_request() -> GuardianReviewRequest {
    GuardianReviewRequest {
        action: GuardianReviewAction {
            review_id: "review-1".to_string(),
            turn_id: "turn-1".to_string(),
            action_id: "action-1".to_string(),
            source: codex_extension_api::ToolCallSource::CodeMode {
                cell_id: "cell-1".to_string(),
                runtime_tool_call_id: "runtime-call-1".to_string(),
            },
            evidence_revision: 1,
            action: GuardianAssessmentAction::McpToolCall {
                server: "node_repl".to_string(),
                tool_name: "js".to_string(),
                connector_id: None,
                connector_name: None,
                tool_title: None,
            },
            request_payload: json!({"script": "return 1"}),
        },
        history: Vec::new(),
        evidence: Vec::new(),
        images: Vec::new(),
    }
}

async fn sampler_with_events(events: Vec<Value>) -> Result<SamplerFixture> {
    let scripted_requests = vec![vec![events]];
    let idle_server = responses::start_websocket_server(scripted_requests.clone()).await;
    let server = responses::start_websocket_server(scripted_requests).await;
    let sampler = Arc::new(
        LunaSampler::connect(sampler_config(
            proxy_websocket_servers(&[&idle_server, &server]).await?,
        ))
        .await?,
    );
    Ok(SamplerFixture {
        sampler,
        _servers: [idle_server, server],
    })
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
async fn account_switch_discards_authenticated_idle_connections() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let first_idle = responses::start_websocket_server(vec![vec![vec![]]]).await;
    let second_idle = responses::start_websocket_server(vec![vec![vec![]]]).await;
    let switched = responses::start_websocket_server(vec![vec![vec![
        ev_output_text_delta(VALID_OUTCOME),
        ev_completed("response-1"),
    ]]])
    .await;
    let auth_manager = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("account-a"));
    let sampler = LunaSampler::connect(sampler_config_with_auth_manager(
        proxy_websocket_servers(&[&first_idle, &second_idle, &switched]).await?,
        Arc::clone(&auth_manager),
    ))
    .await?;

    auth_manager
        .set_external_auth(Arc::new(StaticExternalAuth(CodexAuth::from_api_key(
            "account-b",
        ))))
        .await?;
    let output = sampler
        .sample(sample_request(Instant::now() + Duration::from_secs(2)))
        .await?;

    assert_eq!(output, VALID_OUTCOME);
    assert_eq!(
        first_idle.single_handshake().header("authorization").as_deref(),
        Some("Bearer account-a")
    );
    assert_eq!(
        second_idle
            .single_handshake()
            .header("authorization")
            .as_deref(),
        Some("Bearer account-a")
    );
    assert_eq!(
        switched.single_handshake().header("authorization").as_deref(),
        Some("Bearer account-b")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_fails_closed_and_later_auth_recovers() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let first_idle = responses::start_websocket_server(vec![vec![vec![]]]).await;
    let second_idle = responses::start_websocket_server(vec![vec![vec![]]]).await;
    let recovered = responses::start_websocket_server(vec![vec![vec![
        ev_output_text_delta(VALID_OUTCOME),
        ev_completed("response-1"),
    ]]])
    .await;
    let auth_manager = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("account-a"));
    let sampler = LunaSampler::connect(sampler_config_with_auth_manager(
        proxy_websocket_servers(&[&first_idle, &second_idle, &recovered]).await?,
        Arc::clone(&auth_manager),
    ))
    .await?;

    let _ = auth_manager.logout().await?;
    let error = sampler
        .sample(sample_request(Instant::now() + Duration::from_secs(2)))
        .await
        .expect_err("logout must not reuse an authenticated connection");
    assert!(matches!(error, LunaSamplerError::MissingAuthentication));

    auth_manager
        .set_external_auth(Arc::new(StaticExternalAuth(CodexAuth::from_api_key(
            "account-c",
        ))))
        .await?;
    let output = sampler
        .sample(sample_request(Instant::now() + Duration::from_secs(2)))
        .await?;

    assert_eq!(output, VALID_OUTCOME);
    assert_eq!(
        recovered.single_handshake().header("authorization").as_deref(),
        Some("Bearer account-c")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn valid_multi_delta_outcome_is_parsed_after_completion() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let fixture = sampler_with_events(vec![
        ev_output_text_delta(r#"{"score":0.25,"risk_level":"low","#),
        ev_output_text_delta(
            r#"user_authorization":"high","outcome":"allow","rationale":"safe"}"#,
        ),
        ev_completed("response-1"),
    ])
    .await?;
    let reviewer = GuardianReviewClient::new(Arc::clone(&fixture.sampler))
        .with_review_timeout(Duration::from_secs(2));

    let outcome = reviewer.review(review_request()).await?;

    assert_eq!(
        outcome,
        GuardianReviewOutcome {
            score: 0.25,
            risk_level: GuardianRiskLevel::Low,
            user_authorization: GuardianUserAuthorization::High,
            outcome: GuardianAssessmentOutcome::Allow,
            rationale: "safe".to_string(),
        }
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn valid_json_prefix_with_trailing_data_requires_manual_review() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let fixture = sampler_with_events(vec![
        ev_output_text_delta(VALID_OUTCOME),
        ev_output_text_delta(" trailing data"),
        ev_completed("response-1"),
    ])
    .await?;
    let reviewer = GuardianReviewClient::new(Arc::clone(&fixture.sampler))
        .with_review_timeout(Duration::from_secs(2));

    let error = reviewer
        .review(review_request())
        .await
        .expect_err("trailing data must invalidate the complete response");

    assert!(matches!(error, crate::request::GuardianReviewError::InvalidOutput));
    assert!(error.requires_manual_review());
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_single_delta_is_rejected_before_append() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let oversized = "x".repeat(MAX_OUTPUT_BYTES + 1);
    let fixture = sampler_with_events(vec![
        ev_output_text_delta(&oversized),
        ev_completed("response-1"),
    ])
    .await?;

    let error = fixture
        .sampler
        .sample(sample_request(Instant::now() + Duration::from_secs(2)))
        .await
        .expect_err("oversized delta should be rejected");

    assert!(matches!(error, LunaSamplerError::OutputTooLarge));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_single_output_item_is_rejected_before_append() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let oversized = "x".repeat(MAX_OUTPUT_BYTES + 1);
    let fixture = sampler_with_events(vec![
        ev_assistant_message("response-1", &oversized),
        ev_completed("response-1"),
    ])
    .await?;

    let error = fixture
        .sampler
        .sample(sample_request(Instant::now() + Duration::from_secs(2)))
        .await
        .expect_err("oversized output item should be rejected");

    assert!(matches!(error, LunaSamplerError::OutputTooLarge));
    Ok(())
}

#[test]
fn output_limit_is_strictly_four_kibibytes() {
    let mut output = String::new();
    assert!(append_bounded_output(&mut output, &"x".repeat(MAX_OUTPUT_BYTES)).is_ok());
    assert!(matches!(
        append_bounded_output(&mut output, "x"),
        Err(LunaSamplerError::OutputTooLarge)
    ));
    assert_eq!(output.len(), MAX_OUTPUT_BYTES);
}
