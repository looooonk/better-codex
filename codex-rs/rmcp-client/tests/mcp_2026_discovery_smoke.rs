mod streamable_http_test_support;

use std::time::Duration;

use codex_config::types::AuthKeyringBackendKind;
use codex_config::types::OAuthCredentialsStoreMode;
use codex_exec_server::Environment;
use codex_rmcp_client::ElicitationAction;
use codex_rmcp_client::ElicitationResponse;
use codex_rmcp_client::McpProtocolMode;
use codex_rmcp_client::RmcpClient;
use futures::FutureExt;
use pretty_assertions::assert_eq;
use rmcp::model::ClientCapabilities;
use rmcp::model::Implementation;
use rmcp::model::InitializeRequestParams;
use rmcp::model::ProtocolVersion;

use streamable_http_test_support::spawn_streamable_http_server;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn modern_http_client_negotiates_discovery() -> anyhow::Result<()> {
    let (_server, base_url) = spawn_streamable_http_server().await?;
    let client = RmcpClient::new_streamable_http_client_with_protocol_mode(
        "modern-discovery-test",
        &format!("{base_url}/mcp"),
        Some("test-bearer".to_string()),
        /*http_headers*/ None,
        /*env_http_headers*/ None,
        OAuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
        Environment::default_for_tests().get_http_client(),
        /*auth_provider*/ None,
        McpProtocolMode::V20260728,
    )
    .await?;

    let server_info = client
        .initialize(
            InitializeRequestParams::new(
                ClientCapabilities::default(),
                Implementation::new("modern-discovery-test", "1.0.0"),
            )
            .with_protocol_version(ProtocolVersion::V_2025_06_18),
            Some(Duration::from_secs(5)),
            Box::new(|_, _| {
                async {
                    Ok(ElicitationResponse {
                        action: ElicitationAction::Decline,
                        content: None,
                        meta: None,
                    })
                }
                .boxed()
            }),
        )
        .await?;
    let tools = client
        .list_tools(/*params*/ None, Some(Duration::from_secs(5)))
        .await?;

    assert_eq!(client.protocol_mode(), McpProtocolMode::V20260728);
    assert_eq!(server_info.protocol_version, ProtocolVersion::V_2026_07_28);
    assert!(!tools.tools.is_empty());

    client.shutdown().await;
    Ok(())
}
