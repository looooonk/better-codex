use std::sync::Arc;

use anyhow::Result;
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
use codex_protocol::protocol::SessionSource;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use tokio::net::TcpListener;
use tokio::net::TcpStream;

use super::*;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unavailable_initial_sampler_recovers_after_auth_arrives() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let first = responses::start_websocket_server(vec![vec![vec![]]]).await;
    let second = responses::start_websocket_server(vec![vec![vec![]]]).await;
    let auth_manager = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("expired"));
    let _ = auth_manager.logout().await?;
    let state = GuardianV2ThreadState::new(LunaSamplerConfig {
        provider: create_model_provider(
            ModelProviderInfo::create_openai_provider(Some(
                proxy_websocket_servers(&[&first, &second]).await?,
            )),
            Some(Arc::clone(&auth_manager)),
        ),
        http_client_factory: HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        agent_identity_policy: AgentIdentityAuthPolicy::JwtOnly,
        session_source: SessionSource::Exec,
        session_id: "session-1".to_string(),
        thread_id: "thread-1".to_string(),
        originator: Some("guardian-v2-test".to_string()),
        service_tier: None,
    })
    .await;
    assert!(state.client.lock().await.is_none());

    auth_manager
        .set_external_auth(Arc::new(StaticExternalAuth(CodexAuth::from_api_key(
            "recovered",
        ))))
        .await?;
    let _client = state.client().await?;

    assert_eq!(
        first.single_handshake().header("authorization").as_deref(),
        Some("Bearer recovered")
    );
    assert_eq!(
        second.single_handshake().header("authorization").as_deref(),
        Some("Bearer recovered")
    );
    Ok(())
}
