use super::*;
use crate::codex_apps_cache::CodexAppsToolsCache;
use crate::codex_apps_cache::CodexAppsToolsCacheContext;
use crate::elicitation::ElicitationLifecycle;
use crate::elicitation::ElicitationRequestManager;
use crate::elicitation::ElicitationRequestRouter;
use crate::elicitation::elicitation_is_rejected_by_policy;
use crate::rmcp_client::AsyncManagedClient;
use crate::rmcp_client::CODEX_APPS_RECONNECT_INITIAL_BACKOFF;
use crate::rmcp_client::CodexAppsStartupReconnect;
use crate::rmcp_client::ManagedClient;
use crate::rmcp_client::ManagedClientFuture;
use crate::rmcp_client::StartupOutcomeError;
use crate::server::EffectiveMcpServer;
use crate::server::McpServerMetadata;
use crate::server::McpServerOrigin;
use crate::tool_catalog_cache::McpToolCatalogCacheContext;
use crate::tools::ToolFilter;
use crate::tools::ToolInfo;
use crate::tools::filter_tools;
use crate::tools::normalize_tools_for_model_with_prefix;
use codex_config::AppToolApproval;
use codex_config::Constrained;
use codex_config::McpServerConfig;
use codex_config::McpServerToolConfig;
use codex_config::types::AuthKeyringBackendKind;
use codex_config::types::OAuthCredentialsStoreMode;
use codex_exec_server::EnvironmentManager;
use codex_protocol::ToolName;
use codex_protocol::mcp::McpServerInfo;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::GranularApprovalConfig;
use codex_rmcp_client::InProcessTransportFactory;
use codex_rmcp_client::RmcpClient;
use futures::FutureExt;
use futures::future::BoxFuture;
use pretty_assertions::assert_eq;
use rmcp::model::ElicitRequestParams;
use rmcp::model::ElicitationAction;
use rmcp::model::ElicitationCapability;
use rmcp::model::JsonObject;
use rmcp::model::MetaObject;
use rmcp::model::NumberOrString;
use rmcp::model::Tool;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tempfile::tempdir;
use tokio::io::DuplexStream;

fn create_test_tool(server_name: &str, tool_name: &str) -> ToolInfo {
    ToolInfo {
        server_name: server_name.to_string(),
        supports_parallel_tool_calls: false,
        server_origin: None,
        callable_name: tool_name.to_string(),
        callable_namespace: server_name.to_string(),
        namespace_description: None,
        tool: Tool::new(
            tool_name.to_string(),
            format!("Test tool: {tool_name}"),
            Arc::new(JsonObject::default()),
        ),
        openai_file_input_optional_fields: Default::default(),
        connector_id: None,
        connector_name: None,
        plugin_display_names: Vec::new(),
    }
}

fn create_codex_apps_tools_cache_context(
    codex_home: PathBuf,
    account_id: Option<&str>,
    chatgpt_user_id: Option<&str>,
) -> CodexAppsToolsCacheContext {
    CodexAppsToolsCache::default().context(
        codex_home,
        CodexAppsToolsCacheKey {
            account_id: account_id.map(ToOwned::to_owned),
            chatgpt_user_id: chatgpt_user_id.map(ToOwned::to_owned),
            is_workspace_account: false,
        },
    )
}

fn create_test_server_info(title: &str) -> McpServerInfo {
    McpServerInfo {
        name: "codex-apps".to_string(),
        title: Some(title.to_string()),
        version: "1.0.0".to_string(),
        description: None,
        icons: None,
        website_url: None,
    }
}

struct TestInProcessTransportFactory;

impl InProcessTransportFactory for TestInProcessTransportFactory {
    fn open(&self) -> BoxFuture<'static, io::Result<DuplexStream>> {
        async {
            let (client_stream, _server_stream) = tokio::io::duplex(1);
            Ok(client_stream)
        }
        .boxed()
    }
}

async fn create_test_managed_client(tools: Vec<ToolInfo>) -> ManagedClient {
    ManagedClient {
        client: Arc::new(
            RmcpClient::new_in_process_client(Arc::new(TestInProcessTransportFactory))
                .await
                .expect("create in-process RMCP client"),
        ),
        server_info: create_test_server_info("Ready"),
        tools,
        tool_filter: ToolFilter::default(),
        tool_timeout: None,
        server_instructions: None,
        server_supports_sandbox_state_meta_capability: false,
        codex_apps_tools_cache_context: None,
    }
}

async fn create_ready_async_managed_client(tools: Vec<ToolInfo>) -> AsyncManagedClient {
    AsyncManagedClient {
        client: futures::future::ready::<Result<ManagedClient, StartupOutcomeError>>(Ok(
            create_test_managed_client(tools).await,
        ))
        .boxed()
        .shared(),
        is_codex_apps_mcp_server: false,
        cached_server_info: None,
        codex_apps_tools_cache_context: None,
        tool_catalog_cache_context: None,
        tool_filter: ToolFilter::default(),
        startup_complete: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        startup_reconnect: None,
        tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
        cancel_token: CancellationToken::new(),
    }
}

fn create_failed_cache_observer(
    server_name: &str,
    codex_apps_tools_cache_context: Option<CodexAppsToolsCacheContext>,
    tool_catalog_cache_context: Option<McpToolCatalogCacheContext>,
) -> AsyncManagedClient {
    AsyncManagedClient {
        client: futures::future::ready::<Result<ManagedClient, StartupOutcomeError>>(Err(
            StartupOutcomeError::Failed {
                error: "pending shared-cache observer".to_string(),
                is_authentication_required: false,
            },
        ))
        .boxed()
        .shared(),
        is_codex_apps_mcp_server: server_name == CODEX_APPS_MCP_SERVER_NAME,
        cached_server_info: None,
        codex_apps_tools_cache_context,
        tool_catalog_cache_context,
        tool_filter: ToolFilter::default(),
        startup_complete: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        startup_reconnect: None,
        tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
        cancel_token: CancellationToken::new(),
    }
}

fn create_transitioning_async_managed_client(
    server_name: &str,
    live_client: ManagedClient,
    codex_apps_tools_cache_context: Option<CodexAppsToolsCacheContext>,
    catalog_revision: Arc<tokio::sync::RwLock<u64>>,
    release: Arc<tokio::sync::Notify>,
) -> AsyncManagedClient {
    let startup_complete = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let startup_complete_for_future = Arc::clone(&startup_complete);
    let catalog_revision_for_future = Arc::clone(&catalog_revision);
    let client = async move {
        release.notified().await;
        let mut catalog_revision = catalog_revision_for_future.write().await;
        *catalog_revision += 1;
        startup_complete_for_future.store(true, std::sync::atomic::Ordering::Release);
        Ok::<ManagedClient, StartupOutcomeError>(live_client)
    }
    .boxed()
    .shared();
    if codex_apps_tools_cache_context.is_some() {
        let startup_task = client.clone();
        tokio::spawn(async move {
            let _ = startup_task.await;
        });
    }
    AsyncManagedClient {
        client,
        is_codex_apps_mcp_server: server_name == CODEX_APPS_MCP_SERVER_NAME,
        cached_server_info: None,
        codex_apps_tools_cache_context,
        tool_catalog_cache_context: None,
        tool_filter: ToolFilter::default(),
        startup_complete,
        startup_reconnect: None,
        tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
        cancel_token: CancellationToken::new(),
    }
}

fn create_test_manager_with_failed_apps_startup(
    cached_tools: Vec<ToolInfo>,
    reconnect_factory: Arc<dyn Fn() -> ManagedClientFuture + Send + Sync>,
) -> McpConnectionManager {
    let client: ManagedClientFuture = futures::future::ready(Err(StartupOutcomeError::Failed {
        error: "startup failed".to_string(),
        is_authentication_required: false,
    }))
    .boxed()
    .shared();
    let codex_home = tempdir().expect("tempdir");
    let cache_context = create_codex_apps_tools_cache_context(
        codex_home.path().to_path_buf(),
        Some("reconnect-test-account"),
        Some("reconnect-test-user"),
    );
    cache_context.store_current_tools_for_test(cached_tools);
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    let catalog_revision = Arc::clone(&manager.tool_catalog_revision);
    manager.clients.insert(
        CODEX_APPS_MCP_SERVER_NAME.to_string(),
        AsyncManagedClient {
            client,
            is_codex_apps_mcp_server: true,
            cached_server_info: None,
            codex_apps_tools_cache_context: Some(cache_context),
            tool_catalog_cache_context: None,
            tool_filter: ToolFilter::default(),
            startup_complete: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            startup_reconnect: Some(Arc::new(CodexAppsStartupReconnect::new(
                reconnect_factory,
                Arc::clone(&catalog_revision),
            ))),
            tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
            cancel_token: CancellationToken::new(),
        },
    );
    manager
}

fn model_tool_names(tools: &[ToolInfo]) -> HashSet<ToolName> {
    tools
        .iter()
        .map(ToolInfo::canonical_tool_name)
        .collect::<HashSet<_>>()
}

fn model_tool_name_len(name: &ToolName) -> usize {
    name.namespace
        .as_deref()
        .map_or(0, |namespace| namespace.len() + "__".len())
        + name.name.len()
}

fn is_code_mode_compatible_tool_name(name: &ToolName) -> bool {
    name.namespace
        .as_deref()
        .into_iter()
        .chain(std::iter::once(name.name.as_str()))
        .flat_map(str::chars)
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[test]
fn elicitation_granular_policy_defaults_to_prompting() {
    assert!(!elicitation_is_rejected_by_policy(
        AskForApproval::OnRequest
    ));
    assert!(!elicitation_is_rejected_by_policy(
        AskForApproval::UnlessTrusted
    ));
    assert!(elicitation_is_rejected_by_policy(AskForApproval::Granular(
        GranularApprovalConfig {
            sandbox_approval: true,
            rules: true,
            skill_approval: true,
            request_permissions: true,
            mcp_elicitations: false,
        }
    )));
}

#[test]
fn elicitation_granular_policy_respects_never_and_config() {
    assert!(elicitation_is_rejected_by_policy(AskForApproval::Never));
    assert!(elicitation_is_rejected_by_policy(AskForApproval::Granular(
        GranularApprovalConfig {
            sandbox_approval: true,
            rules: true,
            skill_approval: true,
            request_permissions: true,
            mcp_elicitations: false,
        }
    )));
}

#[tokio::test]
async fn disabled_permissions_auto_accept_elicitation_with_empty_form_schema() {
    let manager = ElicitationRequestManager::new(
        AskForApproval::Never,
        PermissionProfile::Disabled,
        /*reviewer*/ None,
        /*lifecycle*/ None,
        ElicitationRequestRouter::default(),
    );
    let (tx_event, _rx_event) = async_channel::bounded(1);
    let sender = manager.make_sender("server".to_string(), tx_event);

    let response = sender(
        NumberOrString::Number(1),
        codex_rmcp_client::Elicitation::Mcp(ElicitRequestParams::FormElicitationParams {
            meta: None,
            message: "Confirm?".to_string(),
            requested_schema: rmcp::model::ElicitationSchema::builder()
                .build()
                .expect("schema should build"),
        }),
    )
    .await
    .expect("elicitation should auto accept");

    assert_eq!(
        response,
        ElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(serde_json::json!({})),
            meta: None,
        }
    );
}

#[tokio::test]
async fn disabled_permissions_do_not_auto_accept_elicitation_with_requested_fields() {
    let manager = ElicitationRequestManager::new(
        AskForApproval::Never,
        PermissionProfile::Disabled,
        /*reviewer*/ None,
        /*lifecycle*/ None,
        ElicitationRequestRouter::default(),
    );
    let (tx_event, _rx_event) = async_channel::bounded(1);
    let sender = manager.make_sender("server".to_string(), tx_event);

    let response = sender(
        NumberOrString::Number(1),
        codex_rmcp_client::Elicitation::Mcp(ElicitRequestParams::FormElicitationParams {
            meta: None,
            message: "What should I say?".to_string(),
            requested_schema:
                rmcp::model::ElicitationSchema::builder()
                    .required_property(
                        "message",
                        rmcp::model::PrimitiveSchemaDefinition::String(
                            rmcp::model::StringSchema::new(),
                        ),
                    )
                    .build()
                    .expect("schema should build"),
        }),
    )
    .await
    .expect("elicitation should auto decline");

    assert_eq!(
        response,
        ElicitationResponse {
            action: ElicitationAction::Decline,
            content: None,
            meta: None,
        }
    );
}

#[tokio::test]
async fn shared_elicitation_router_targets_the_exact_pending_request() {
    struct Registration(Arc<AtomicUsize>);

    impl Drop for Registration {
        fn drop(&mut self) {
            self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let router = ElicitationRequestRouter::default();
    let outstanding = Arc::new(AtomicUsize::new(0));
    let lifecycle = ElicitationLifecycle::new({
        let outstanding = outstanding.clone();
        move || {
            outstanding.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Registration(outstanding.clone())
        }
    });
    let manager_a = ElicitationRequestManager::new(
        AskForApproval::OnRequest,
        PermissionProfile::default(),
        /*reviewer*/ None,
        Some(lifecycle.clone()),
        router.clone(),
    );
    let manager_b = ElicitationRequestManager::new(
        AskForApproval::OnRequest,
        PermissionProfile::default(),
        /*reviewer*/ None,
        Some(lifecycle),
        router,
    );
    let (tx_event, rx_event) = async_channel::bounded(2);
    let sender_a = manager_a.make_sender("server".to_string(), tx_event.clone());
    let sender_b = manager_b.make_sender("server".to_string(), tx_event);
    let elicitation =
        codex_rmcp_client::Elicitation::Mcp(ElicitRequestParams::FormElicitationParams {
            meta: None,
            message: "Which runtime?".to_string(),
            requested_schema:
                rmcp::model::ElicitationSchema::builder()
                    .required_property(
                        "runtime",
                        rmcp::model::PrimitiveSchemaDefinition::String(
                            rmcp::model::StringSchema::new(),
                        ),
                    )
                    .build()
                    .expect("schema should build"),
        });

    let pending_a = tokio::spawn(sender_a(NumberOrString::Number(1), elicitation.clone()));
    let EventMsg::ElicitationRequest(request_a) = rx_event.recv().await.expect("request A").msg
    else {
        panic!("expected elicitation request");
    };
    let pending_b = tokio::spawn(sender_b(NumberOrString::Number(1), elicitation));
    let EventMsg::ElicitationRequest(request_b) = rx_event.recv().await.expect("request B").msg
    else {
        panic!("expected elicitation request");
    };
    assert_eq!(outstanding.load(std::sync::atomic::Ordering::SeqCst), 2);
    let (
        codex_protocol::mcp::RequestId::String(request_a_id),
        codex_protocol::mcp::RequestId::String(request_b_id),
    ) = (request_a.id, request_b.id)
    else {
        panic!("expected Codex-owned string request IDs");
    };
    assert_ne!(request_a_id, request_b_id);

    let response_a = ElicitationResponse {
        action: ElicitationAction::Accept,
        content: Some(serde_json::json!({"runtime": "a"})),
        meta: None,
    };
    manager_b
        .resolve(
            "server".to_string(),
            NumberOrString::String(request_a_id.into()),
            response_a.clone(),
        )
        .await
        .expect("runtime B should route a response to runtime A");
    let response_b = ElicitationResponse {
        action: ElicitationAction::Accept,
        content: Some(serde_json::json!({"runtime": "b"})),
        meta: None,
    };
    manager_a
        .resolve(
            "server".to_string(),
            NumberOrString::String(request_b_id.into()),
            response_b.clone(),
        )
        .await
        .expect("runtime A should route a response to runtime B");

    assert_eq!(
        pending_a
            .await
            .expect("request A task")
            .expect("request A response"),
        response_a
    );
    assert_eq!(
        pending_b
            .await
            .expect("request B task")
            .expect("request B response"),
        response_b
    );
    assert_eq!(outstanding.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn test_normalize_tools_short_non_duplicated_names() {
    let tools = vec![
        create_test_tool("server1", "tool1"),
        create_test_tool("server1", "tool2"),
    ];

    let model_tools =
        normalize_tools_for_model_with_prefix(tools, /*prefix_mcp_tool_names*/ true);

    assert_eq!(
        model_tool_names(&model_tools),
        HashSet::from([
            ToolName::namespaced("mcp__server1", "tool1"),
            ToolName::namespaced("mcp__server1", "tool2")
        ])
    );
}

#[test]
fn test_normalize_tools_duplicated_names_skipped() {
    let tools = vec![
        create_test_tool("server1", "duplicate_tool"),
        create_test_tool("server1", "duplicate_tool"),
    ];

    let model_tools =
        normalize_tools_for_model_with_prefix(tools, /*prefix_mcp_tool_names*/ true);

    // Only the first tool should remain, the second is skipped
    assert_eq!(
        model_tool_names(&model_tools),
        HashSet::from([ToolName::namespaced("mcp__server1", "duplicate_tool")])
    );
}

#[test]
fn test_normalize_tools_long_names_same_server() {
    let server_name = "my_server";

    let tools = vec![
        create_test_tool(
            server_name,
            "extremely_lengthy_function_name_that_absolutely_surpasses_all_reasonable_limits",
        ),
        create_test_tool(
            server_name,
            "yet_another_extremely_lengthy_function_name_that_absolutely_surpasses_all_reasonable_limits",
        ),
    ];

    let model_tools =
        normalize_tools_for_model_with_prefix(tools, /*prefix_mcp_tool_names*/ true);

    assert_eq!(model_tools.len(), 2);

    let names = model_tool_names(&model_tools);

    assert!(names.iter().all(|name| model_tool_name_len(name) == 64));
    assert!(
        names
            .iter()
            .all(|name| name.namespace.as_deref() == Some("mcp__my_server"))
    );
    assert!(
        names.iter().all(is_code_mode_compatible_tool_name),
        "model-visible names must be code-mode compatible: {names:?}"
    );
}

#[test]
fn test_normalize_tools_sanitizes_invalid_characters() {
    let tools = vec![create_test_tool("server.one", "tool.two-three")];

    let model_tools =
        normalize_tools_for_model_with_prefix(tools, /*prefix_mcp_tool_names*/ true);

    assert_eq!(model_tools.len(), 1);
    let tool = model_tools.into_iter().next().expect("one tool");
    let model_name = tool.canonical_tool_name();
    assert_eq!(
        model_name,
        ToolName::namespaced("mcp__server_one", "tool_two_three")
    );
    assert_eq!(
        ToolName::namespaced(tool.callable_namespace.clone(), tool.callable_name.clone()),
        model_name
    );
    // The callable parts are sanitized for model-visible tool calls, but the raw
    // MCP name is preserved for the actual MCP call.
    assert_eq!(tool.server_name, "server.one");
    assert_eq!(tool.callable_namespace, "mcp__server_one");
    assert_eq!(tool.callable_name, "tool_two_three");
    assert_eq!(tool.tool.name, "tool.two-three");

    assert!(
        is_code_mode_compatible_tool_name(&model_name),
        "model-visible name must be code-mode compatible: {model_name:?}"
    );
}

#[test]
fn test_normalize_tools_keeps_hyphenated_mcp_tools_callable() {
    let tools = vec![create_test_tool("music-studio", "get-strudel-guide")];

    let model_tools =
        normalize_tools_for_model_with_prefix(tools, /*prefix_mcp_tool_names*/ true);

    assert_eq!(model_tools.len(), 1);
    let tool = model_tools.into_iter().next().expect("one tool");
    assert_eq!(
        tool.canonical_tool_name(),
        ToolName::namespaced("mcp__music_studio", "get_strudel_guide")
    );
    assert_eq!(tool.callable_namespace, "mcp__music_studio");
    assert_eq!(tool.callable_name, "get_strudel_guide");
    assert_eq!(tool.tool.name, "get-strudel-guide");
}

#[test]
fn test_normalize_tools_disambiguates_sanitized_namespace_collisions() {
    let tools = vec![
        create_test_tool("basic-server", "lookup"),
        create_test_tool("basic_server", "query"),
    ];

    let model_tools =
        normalize_tools_for_model_with_prefix(tools, /*prefix_mcp_tool_names*/ true);

    assert_eq!(model_tools.len(), 2);
    let mut namespaces = model_tools
        .iter()
        .map(|tool| tool.callable_namespace.as_str())
        .collect::<Vec<_>>();
    namespaces.sort();
    namespaces.dedup();
    assert_eq!(namespaces.len(), 2);

    let raw_servers = model_tools
        .iter()
        .map(|tool| tool.server_name.as_str())
        .collect::<HashSet<_>>();
    assert_eq!(raw_servers, HashSet::from(["basic-server", "basic_server"]));
    let model_names = model_tool_names(&model_tools);
    assert!(
        model_names.iter().all(is_code_mode_compatible_tool_name),
        "model-visible names must be code-mode compatible: {model_names:?}"
    );
}

#[test]
fn test_normalize_tools_disambiguates_sanitized_tool_name_collisions() {
    let tools = vec![
        create_test_tool("server", "tool-name"),
        create_test_tool("server", "tool_name"),
    ];

    let model_tools =
        normalize_tools_for_model_with_prefix(tools, /*prefix_mcp_tool_names*/ true);

    assert_eq!(model_tools.len(), 2);
    let raw_tool_names = model_tools
        .iter()
        .map(|tool| tool.tool.name.to_string())
        .collect::<HashSet<_>>();
    assert_eq!(
        raw_tool_names,
        HashSet::from(["tool-name".to_string(), "tool_name".to_string()])
    );
    let callable_tool_names = model_tools
        .iter()
        .map(|tool| tool.callable_name.as_str())
        .collect::<HashSet<_>>();
    assert_eq!(callable_tool_names.len(), 2);
}

#[test]
fn tool_filter_allows_by_default() {
    let filter = ToolFilter::default();

    assert!(filter.allows("any"));
}

#[test]
fn tool_filter_applies_enabled_list() {
    let filter = ToolFilter {
        enabled: Some(HashSet::from(["allowed".to_string()])),
        disabled: HashSet::new(),
    };

    assert!(filter.allows("allowed"));
    assert!(!filter.allows("denied"));
}

#[test]
fn tool_filter_applies_disabled_list() {
    let filter = ToolFilter {
        enabled: None,
        disabled: HashSet::from(["blocked".to_string()]),
    };

    assert!(!filter.allows("blocked"));
    assert!(filter.allows("open"));
}

#[test]
fn tool_filter_applies_enabled_then_disabled() {
    let filter = ToolFilter {
        enabled: Some(HashSet::from(["keep".to_string(), "remove".to_string()])),
        disabled: HashSet::from(["remove".to_string()]),
    };

    assert!(filter.allows("keep"));
    assert!(!filter.allows("remove"));
    assert!(!filter.allows("unknown"));
}

#[test]
fn filter_tools_applies_per_server_filters() {
    let server1_tools = vec![
        create_test_tool("server1", "tool_a"),
        create_test_tool("server1", "tool_b"),
    ];
    let server2_tools = vec![create_test_tool("server2", "tool_a")];
    let server1_filter = ToolFilter {
        enabled: Some(HashSet::from(["tool_a".to_string(), "tool_b".to_string()])),
        disabled: HashSet::from(["tool_b".to_string()]),
    };
    let server2_filter = ToolFilter {
        enabled: None,
        disabled: HashSet::from(["tool_a".to_string()]),
    };

    let filtered: Vec<_> = filter_tools(server1_tools, &server1_filter)
        .into_iter()
        .chain(filter_tools(server2_tools, &server2_filter))
        .collect();

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].server_name, "server1");
    assert_eq!(filtered[0].callable_name, "tool_a");
}

#[test]
fn codex_apps_env_bearer_token_bypasses_shared_tools_cache() {
    assert!(!should_share_codex_apps_tools_cache(
        CODEX_APPS_MCP_SERVER_NAME,
        /*uses_env_bearer_token*/ true,
    ));
}

#[tokio::test]
async fn list_all_tools_uses_shared_codex_apps_cache_while_client_is_pending() {
    let codex_home = tempdir().expect("tempdir");
    let cache_context = create_codex_apps_tools_cache_context(
        codex_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    cache_context.store_current_tools_for_test(vec![create_test_tool(
        CODEX_APPS_MCP_SERVER_NAME,
        "calendar_create_event",
    )]);
    let pending_client = futures::future::pending::<Result<ManagedClient, StartupOutcomeError>>()
        .boxed()
        .shared();
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    manager.clients.insert(
        CODEX_APPS_MCP_SERVER_NAME.to_string(),
        AsyncManagedClient {
            client: pending_client,
            is_codex_apps_mcp_server: true,
            cached_server_info: None,
            codex_apps_tools_cache_context: Some(cache_context),
            tool_catalog_cache_context: None,
            tool_filter: ToolFilter::default(),
            startup_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            startup_reconnect: None,
            tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
            cancel_token: CancellationToken::new(),
        },
    );

    let tools = manager.list_all_tools().await;
    let tool = tools
        .iter()
        .find(|tool| {
            tool.canonical_tool_name()
                == ToolName::namespaced("mcp__codex_apps", "calendar_create_event")
        })
        .expect("tool from shared cache");
    assert_eq!(tool.server_name, CODEX_APPS_MCP_SERVER_NAME);
    assert_eq!(tool.callable_name, "calendar_create_event");
}

#[tokio::test]
async fn cached_catalog_is_superseded_only_after_live_startup_revision_publishes() {
    let codex_home = tempdir().expect("tempdir");
    let cache_context = create_codex_apps_tools_cache_context(
        codex_home.path().to_path_buf(),
        Some("startup-account"),
        Some("startup-user"),
    );
    cache_context.store_current_tools_for_test(vec![create_test_tool(
        CODEX_APPS_MCP_SERVER_NAME,
        "cached_search",
    )]);
    let live_client = create_test_managed_client(vec![create_test_tool(
        CODEX_APPS_MCP_SERVER_NAME,
        "live_search",
    )])
    .await;
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    let release = Arc::new(tokio::sync::Notify::new());
    manager.clients.insert(
        CODEX_APPS_MCP_SERVER_NAME.to_string(),
        create_transitioning_async_managed_client(
            CODEX_APPS_MCP_SERVER_NAME,
            live_client,
            Some(cache_context),
            Arc::clone(&manager.tool_catalog_revision),
            Arc::clone(&release),
        ),
    );
    let manager = Arc::new(manager);

    assert_eq!(manager.catalog_revision().await, 1);
    assert_eq!(
        manager
            .list_all_tools()
            .await
            .into_iter()
            .map(|tool| tool.callable_name)
            .collect::<Vec<_>>(),
        vec!["cached_search"]
    );
    let cached_binding = crate::binding::McpBinding::capture(Arc::clone(&manager)).await;
    assert!(
        cached_binding
            .tool(CODEX_APPS_MCP_SERVER_NAME, "cached_search")
            .is_some()
    );

    let old_revision = manager
        .lock_catalog_revision(/*expected*/ 1)
        .await
        .expect("initial revision");
    release.notify_one();
    tokio::task::yield_now().await;
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            manager.model_visible_tool_info(CODEX_APPS_MCP_SERVER_NAME, "cached_search"),
        )
            .await
            .expect("dispatch lookup must not wait for startup publication")
            .is_none()
    );
    drop(old_revision);

    assert!(
        manager
            .wait_for_server_startup(CODEX_APPS_MCP_SERVER_NAME)
            .await
    );
    assert_eq!(manager.catalog_revision().await, 2);
    assert!(
        manager
            .lock_catalog_revision(/*expected*/ 1)
            .await
            .is_err()
    );
    assert!(!cached_binding.is_current().await);
    assert!(
        cached_binding
            .call_tool(
                CODEX_APPS_MCP_SERVER_NAME,
                "cached_search",
                /*arguments*/ None,
                /*meta*/ None,
            )
            .await
            .is_err()
    );
    let preparation_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let preparation_ran_for_call = Arc::clone(&preparation_ran);
    assert!(
        cached_binding
            .call_tool_with_preparation(
                CODEX_APPS_MCP_SERVER_NAME,
                "cached_search",
                move || async move {
                    preparation_ran_for_call.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok((None, None))
                },
            )
            .await
            .is_err()
    );
    assert!(!preparation_ran.load(std::sync::atomic::Ordering::SeqCst));
    let live_binding = crate::binding::McpBinding::capture(Arc::clone(&manager)).await;
    assert!(
        live_binding
            .tool(CODEX_APPS_MCP_SERVER_NAME, "cached_search")
            .is_none()
    );
    assert!(
        live_binding
            .tool(CODEX_APPS_MCP_SERVER_NAME, "live_search")
            .is_some()
    );
    assert_eq!(
        manager
            .list_all_tools()
            .await
            .into_iter()
            .map(|tool| tool.callable_name)
            .collect::<Vec<_>>(),
        vec!["live_search"]
    );

    let preparation_started = Arc::new(tokio::sync::Notify::new());
    let preparation_started_wait = preparation_started.notified();
    let release_preparation = Arc::new(tokio::sync::Notify::new());
    let call = tokio::spawn({
        let preparation_started = Arc::clone(&preparation_started);
        let release_preparation = Arc::clone(&release_preparation);
        async move {
            live_binding
                .call_tool_with_preparation(
                    CODEX_APPS_MCP_SERVER_NAME,
                    "live_search",
                    move || async move {
                        preparation_started.notify_one();
                        release_preparation.notified().await;
                        Err(anyhow!("stop after preparation"))
                    },
                )
                .await
        }
    });
    preparation_started_wait.await;
    let catalog_writer = tokio::time::timeout(
        Duration::from_secs(1),
        manager.tool_catalog_revision.write(),
    )
    .await
    .expect("catalog writer must not wait for call preparation");
    drop(catalog_writer);
    release_preparation.notify_one();
    assert!(call.await.expect("tool call task").is_err());
}

#[tokio::test]
async fn hard_refresh_publication_bumps_only_for_the_accepted_cache_generation() {
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let manager = Arc::new(McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    ));
    let codex_home = tempdir().expect("tempdir");
    let cache_context = create_codex_apps_tools_cache_context(
        codex_home.path().to_path_buf(),
        Some("refresh-account"),
        Some("refresh-user"),
    );
    let older = cache_context.begin_fetch(CodexAppsToolsFetchSource::HardRefresh);
    let newer = cache_context.begin_fetch(CodexAppsToolsFetchSource::HardRefresh);
    let older_local = manager.begin_codex_apps_refresh();
    let newer_local = manager.begin_codex_apps_refresh();
    let old_revision = manager
        .lock_catalog_revision(/*expected*/ 0)
        .await
        .expect("initial revision");
    let publication = tokio::spawn({
        let manager = Arc::clone(&manager);
        let cache_context = cache_context.clone();
        async move {
            manager
                .publish_codex_apps_catalog(
                    newer_local,
                    Some(&cache_context),
                    Some(newer),
                    &create_test_server_info("Codex Apps"),
                    vec![create_test_tool(CODEX_APPS_MCP_SERVER_NAME, "newer")],
                )
                .await
        }
    });
    tokio::task::yield_now().await;
    assert!(!publication.is_finished());
    drop(old_revision);

    let published = publication.await.expect("catalog publication");
    assert_eq!(published[0].callable_name, "newer");
    assert_eq!(manager.catalog_revision().await, 1);
    assert!(
        manager
            .lock_catalog_revision(/*expected*/ 0)
            .await
            .is_err()
    );

    let stale = manager
        .publish_codex_apps_catalog(
            older_local,
            Some(&cache_context),
            Some(older),
            &create_test_server_info("Codex Apps"),
            vec![create_test_tool(CODEX_APPS_MCP_SERVER_NAME, "older")],
        )
        .await;
    assert_eq!(stale[0].callable_name, "newer");
    assert_eq!(manager.catalog_revision().await, 1);
    assert_eq!(
        manager
            .codex_apps_tools_override()
            .expect("accepted manager override")[0]
            .callable_name,
        "newer"
    );
}

#[tokio::test]
async fn local_refresh_generation_rejects_slower_uncached_result() {
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    let older = manager.begin_codex_apps_refresh();
    let newer = manager.begin_codex_apps_refresh();

    let published = manager
        .publish_codex_apps_catalog(
            newer,
            /*cache_context*/ None,
            /*fetch_ticket*/ None,
            &create_test_server_info("Codex Apps"),
            vec![create_test_tool(CODEX_APPS_MCP_SERVER_NAME, "newer")],
        )
        .await;
    assert_eq!(published[0].callable_name, "newer");
    assert_eq!(manager.catalog_revision().await, 1);

    let stale = manager
        .publish_codex_apps_catalog(
            older,
            /*cache_context*/ None,
            /*fetch_ticket*/ None,
            &create_test_server_info("Codex Apps"),
            vec![create_test_tool(CODEX_APPS_MCP_SERVER_NAME, "older")],
        )
        .await;
    assert_eq!(stale[0].callable_name, "newer");
    assert_eq!(manager.catalog_revision().await, 1);
}

#[tokio::test]
async fn losing_shared_refresh_adopts_the_newer_catalog_locally() {
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    let codex_home = tempdir().expect("tempdir");
    let cache_context = create_codex_apps_tools_cache_context(
        codex_home.path().to_path_buf(),
        Some("shared-refresh-account"),
        Some("shared-refresh-user"),
    );
    let losing_shared_ticket =
        cache_context.begin_fetch(CodexAppsToolsFetchSource::HardRefresh);
    let winning_shared_ticket =
        cache_context.begin_fetch(CodexAppsToolsFetchSource::HardRefresh);
    cache_context.publish_if_newest_accepted(
        winning_shared_ticket,
        &create_test_server_info("Codex Apps"),
        vec![create_test_tool(CODEX_APPS_MCP_SERVER_NAME, "newer")],
    );

    let adopted = manager
        .publish_codex_apps_catalog(
            manager.begin_codex_apps_refresh(),
            Some(&cache_context),
            Some(losing_shared_ticket),
            &create_test_server_info("Codex Apps"),
            vec![create_test_tool(CODEX_APPS_MCP_SERVER_NAME, "older")],
        )
        .await;

    assert_eq!(adopted[0].callable_name, "newer");
    assert_eq!(manager.catalog_revision().await, 1);
    assert_eq!(
        manager
            .codex_apps_tools_override()
            .expect("locally adopted catalog")[0]
            .callable_name,
        "newer"
    );
}

#[tokio::test]
async fn shared_catalog_generation_invalidates_another_managers_binding() {
    let codex_home = tempdir().expect("tempdir");
    let cache = CodexAppsToolsCache::default();
    let cache_context = cache.context(
        codex_home.path().to_path_buf(),
        CodexAppsToolsCacheKey {
            account_id: Some("shared-account".to_string()),
            chatgpt_user_id: Some("shared-user".to_string()),
            is_workspace_account: false,
        },
    );
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut observing_manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    observing_manager.clients.insert(
        CODEX_APPS_MCP_SERVER_NAME.to_string(),
        create_failed_cache_observer(
            CODEX_APPS_MCP_SERVER_NAME,
            Some(cache_context.clone()),
            /*tool_catalog_cache_context*/ None,
        ),
    );
    let observing_manager = Arc::new(observing_manager);
    let first_binding = crate::binding::McpBinding::capture(Arc::clone(&observing_manager)).await;
    assert!(
        first_binding
            .tool(CODEX_APPS_MCP_SERVER_NAME, "v1_tool")
            .is_none()
    );

    let publishing_manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    let shared_ticket = cache_context.begin_fetch(CodexAppsToolsFetchSource::HardRefresh);
    publishing_manager
        .publish_codex_apps_catalog(
            publishing_manager.begin_codex_apps_refresh(),
            Some(&cache_context),
            Some(shared_ticket),
            &create_test_server_info("Codex Apps"),
            vec![create_test_tool(
                CODEX_APPS_MCP_SERVER_NAME,
                "v1_tool",
            )],
        )
        .await;

    assert!(!first_binding.is_current().await);
    let next_binding = crate::binding::McpBinding::capture(observing_manager).await;
    assert!(
        next_binding
            .tool(CODEX_APPS_MCP_SERVER_NAME, "v1_tool")
            .is_some()
    );
}

#[tokio::test]
async fn shared_stdio_catalog_generation_invalidates_a_pending_managers_binding() {
    let cache = McpToolCatalogCache::default();
    let runtime_context = McpRuntimeContext::new(
        Arc::new(EnvironmentManager::without_environments()),
        PathBuf::from("/workspace"),
    );
    let config = serde_json::from_value::<McpServerConfig>(serde_json::json!({
        "command": "docs-mcp",
    }))
    .expect("MCP config");
    let cache_context = cache
        .context(
            "docs",
            &config,
            &runtime_context,
            /*resolved_environment*/ None,
            &ElicitationCapability::default(),
            /*supports_openai_form_elicitation*/ false,
        )
        .expect("cache context");
    cache_context.publish_if_newest(
        cache_context.begin_fetch(),
        &[create_test_tool("docs", "v1_tool")],
    );
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    manager.clients.insert(
        "docs".to_string(),
        create_failed_cache_observer(
            "docs",
            /*codex_apps_tools_cache_context*/ None,
            Some(cache_context.clone()),
        ),
    );
    let manager = Arc::new(manager);
    let first_binding = crate::binding::McpBinding::capture(Arc::clone(&manager)).await;
    assert!(first_binding.tool("docs", "v1_tool").is_some());

    cache_context.publish_if_newest(
        cache_context.begin_fetch(),
        &[create_test_tool("docs", "v2_tool")],
    );

    assert!(!first_binding.is_current().await);
    let next_binding = crate::binding::McpBinding::capture(manager).await;
    assert!(next_binding.tool("docs", "v1_tool").is_none());
    assert!(next_binding.tool("docs", "v2_tool").is_some());
}

#[tokio::test]
async fn first_shared_stdio_catalog_adoption_advances_local_revision() {
    let cache = McpToolCatalogCache::default();
    let runtime_context = McpRuntimeContext::new(
        Arc::new(EnvironmentManager::without_environments()),
        PathBuf::from("/workspace"),
    );
    let config = serde_json::from_value::<McpServerConfig>(serde_json::json!({
        "command": "docs-mcp",
    }))
    .expect("MCP config");
    let cache_context = cache
        .context(
            "docs",
            &config,
            &runtime_context,
            /*resolved_environment*/ None,
            &ElicitationCapability::default(),
            /*supports_openai_form_elicitation*/ false,
        )
        .expect("cache context");
    cache_context.publish_if_newest(
        cache_context.begin_fetch(),
        &[create_test_tool("docs", "first_tool")],
    );
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    manager.clients.insert(
        "docs".to_string(),
        create_failed_cache_observer(
            "docs",
            /*codex_apps_tools_cache_context*/ None,
            Some(cache_context),
        ),
    );

    assert_eq!(manager.catalog_revision().await, 1);
    assert_eq!(
        manager
            .shared_tool_catalog("docs")
            .expect("adopted shared catalog")[0]
            .callable_name,
        "first_tool"
    );
}

#[tokio::test(start_paused = true)]
async fn optional_catalogs_share_one_grace_and_publish_live_revision_later() {
    let first_client =
        create_test_managed_client(vec![create_test_tool("first", "search")]).await;
    let second_client =
        create_test_managed_client(vec![create_test_tool("second", "lookup")]).await;
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    let first_release = Arc::new(tokio::sync::Notify::new());
    let second_release = Arc::new(tokio::sync::Notify::new());
    manager.clients.insert(
        "first".to_string(),
        create_transitioning_async_managed_client(
            "first",
            first_client,
            /*codex_apps_tools_cache_context*/ None,
            Arc::clone(&manager.tool_catalog_revision),
            Arc::clone(&first_release),
        ),
    );
    manager.clients.insert(
        "second".to_string(),
        create_transitioning_async_managed_client(
            "second",
            second_client,
            /*codex_apps_tools_cache_context*/ None,
            Arc::clone(&manager.tool_catalog_revision),
            second_release,
        ),
    );

    let started_at = tokio::time::Instant::now();
    assert!(manager.list_all_tools().await.is_empty());
    assert_eq!(started_at.elapsed(), Duration::from_secs(1));

    first_release.notify_one();
    assert!(manager.wait_for_server_startup("first").await);
    assert_eq!(manager.catalog_revision().await, 1);
    let started_at = tokio::time::Instant::now();
    assert_eq!(
        manager
            .list_all_tools()
            .await
            .into_iter()
            .map(|tool| tool.callable_name)
            .collect::<Vec<_>>(),
        vec!["search"]
    );
    assert_eq!(started_at.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn required_catalog_waits_past_optional_grace() {
    let live_client =
        create_test_managed_client(vec![create_test_tool("required", "search")]).await;
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    manager.required_servers.push("required".to_string());
    let release = Arc::new(tokio::sync::Notify::new());
    manager.clients.insert(
        "required".to_string(),
        create_transitioning_async_managed_client(
            "required",
            live_client,
            /*codex_apps_tools_cache_context*/ None,
            Arc::clone(&manager.tool_catalog_revision),
            Arc::clone(&release),
        ),
    );
    let manager = Arc::new(manager);
    let list_task = tokio::spawn({
        let manager = Arc::clone(&manager);
        async move { manager.list_all_tools().await }
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert!(!list_task.is_finished());

    release.notify_one();
    assert_eq!(
        list_task
            .await
            .expect("required catalog task")
            .into_iter()
            .map(|tool| tool.callable_name)
            .collect::<Vec<_>>(),
        vec!["search"]
    );
    assert_eq!(manager.catalog_revision().await, 1);
}

#[tokio::test]
async fn list_available_server_infos_uses_cache_while_client_is_pending() {
    let pending_client = futures::future::pending::<Result<ManagedClient, StartupOutcomeError>>()
        .boxed()
        .shared();
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    let server_info = create_test_server_info("Codex Apps");
    manager.clients.insert(
        CODEX_APPS_MCP_SERVER_NAME.to_string(),
        AsyncManagedClient {
            client: pending_client,
            is_codex_apps_mcp_server: true,
            cached_server_info: Some(server_info.clone()),
            codex_apps_tools_cache_context: None,
            tool_catalog_cache_context: None,
            tool_filter: ToolFilter::default(),
            startup_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            startup_reconnect: None,
            tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
            cancel_token: CancellationToken::new(),
        },
    );

    let timeout_result = tokio::time::timeout(
        Duration::from_millis(10),
        manager.list_available_server_infos(),
    )
    .await;
    let server_infos = timeout_result.expect("server info lookup should not block on startup");
    assert_eq!(
        server_infos.get(CODEX_APPS_MCP_SERVER_NAME),
        Some(&server_info)
    );
}

#[tokio::test]
async fn list_all_tools_accepts_canonical_namespaced_tool_names() {
    let managed_client =
        create_ready_async_managed_client(vec![create_test_tool("rmcp", "echo")]).await;
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ false,
    );
    manager.clients.insert("rmcp".to_string(), managed_client);

    let tools = manager.list_all_tools().await;
    let tool = tools
        .iter()
        .find(|tool| tool.canonical_tool_name() == ToolName::namespaced("rmcp", "echo"))
        .expect("split MCP tool namespace and name should resolve");

    let expected = ("rmcp", "rmcp", "echo", "echo");
    assert_eq!(
        (
            tool.server_name.as_str(),
            tool.callable_namespace.as_str(),
            tool.callable_name.as_str(),
            tool.tool.name.as_ref(),
        ),
        expected
    );
}

#[tokio::test]
async fn list_all_tools_applies_legacy_mcp_prefix_by_default() {
    let managed_client =
        create_ready_async_managed_client(vec![create_test_tool("rmcp", "echo")]).await;
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    manager.clients.insert("rmcp".to_string(), managed_client);

    let tools = manager.list_all_tools().await;
    let tool = tools
        .iter()
        .find(|tool| tool.canonical_tool_name() == ToolName::namespaced("mcp__rmcp", "echo"))
        .expect("legacy-prefixed MCP tool name should resolve");

    let expected = ("rmcp", "mcp__rmcp", "echo", "echo");
    assert_eq!(
        (
            tool.server_name.as_str(),
            tool.callable_namespace.as_str(),
            tool.callable_name.as_str(),
            tool.tool.name.as_ref(),
        ),
        expected
    );
}

#[tokio::test]
async fn list_all_tools_blocks_while_client_is_pending_without_cached_tools() {
    let pending_client = futures::future::pending::<Result<ManagedClient, StartupOutcomeError>>()
        .boxed()
        .shared();
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    manager.clients.insert(
        CODEX_APPS_MCP_SERVER_NAME.to_string(),
        AsyncManagedClient {
            client: pending_client,
            is_codex_apps_mcp_server: true,
            cached_server_info: None,
            codex_apps_tools_cache_context: None,
            tool_catalog_cache_context: None,
            tool_filter: ToolFilter::default(),
            startup_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            startup_reconnect: None,
            tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
            cancel_token: CancellationToken::new(),
        },
    );

    let timeout_result =
        tokio::time::timeout(Duration::from_millis(10), manager.list_all_tools()).await;
    assert!(timeout_result.is_err());
}

#[tokio::test]
async fn shutdown_cancels_pending_tool_listing() {
    let cancel_token = CancellationToken::new();
    let cancel_token_for_startup = cancel_token.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let pending_client = async move {
        let _ = started_tx.send(());
        cancel_token_for_startup.cancelled().await;
        Err(StartupOutcomeError::Cancelled)
    }
    .boxed()
    .shared();
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    manager.clients.insert(
        CODEX_APPS_MCP_SERVER_NAME.to_string(),
        AsyncManagedClient {
            client: pending_client,
            is_codex_apps_mcp_server: true,
            cached_server_info: None,
            codex_apps_tools_cache_context: None,
            tool_catalog_cache_context: None,
            tool_filter: ToolFilter::default(),
            startup_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            startup_reconnect: None,
            tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
            cancel_token,
        },
    );
    let manager = Arc::new(manager);
    let manager_for_list = Arc::clone(&manager);
    let list_task = tokio::spawn(async move { manager_for_list.list_all_tools().await });

    started_rx.await.expect("tool listing should start");
    tokio::time::timeout(Duration::from_secs(1), manager.shutdown())
        .await
        .expect("shutdown should cancel speculative tool listing");
    let tools = list_task.await.expect("tool listing task should not panic");
    assert!(tools.is_empty());
}

#[tokio::test]
async fn shutdown_continues_after_caller_is_aborted() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
    let release = Arc::new(tokio::sync::Notify::new());
    let release_for_client = Arc::clone(&release);
    let blocking_client = async move {
        let _ = started_tx.send(());
        release_for_client.notified().await;
        let _ = completed_tx.send(());
        Err(StartupOutcomeError::Cancelled)
    }
    .boxed()
    .shared();
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    manager.clients.insert(
        CODEX_APPS_MCP_SERVER_NAME.to_string(),
        AsyncManagedClient {
            client: blocking_client,
            is_codex_apps_mcp_server: true,
            cached_server_info: None,
            codex_apps_tools_cache_context: None,
            tool_catalog_cache_context: None,
            tool_filter: ToolFilter::default(),
            startup_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            startup_reconnect: None,
            tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
            cancel_token: CancellationToken::new(),
        },
    );
    let manager = Arc::new(manager);
    let shutdown_task = tokio::spawn({
        let manager = Arc::clone(&manager);
        async move { manager.shutdown().await }
    });

    started_rx.await.expect("client shutdown should start");
    shutdown_task.abort();
    let shutdown_error = shutdown_task
        .await
        .expect_err("caller shutdown task should be aborted");
    assert!(shutdown_error.is_cancelled());
    release.notify_one();

    tokio::time::timeout(Duration::from_secs(1), completed_rx)
        .await
        .expect("client shutdown should survive caller cancellation")
        .expect("client shutdown completion sender should stay alive");
}

#[tokio::test]
async fn list_all_tools_does_not_block_when_shared_codex_apps_cache_is_empty() {
    let codex_home = tempdir().expect("tempdir");
    let cache_context = create_codex_apps_tools_cache_context(
        codex_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    cache_context.store_current_tools_for_test(Vec::new());
    let pending_client = futures::future::pending::<Result<ManagedClient, StartupOutcomeError>>()
        .boxed()
        .shared();
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    manager.clients.insert(
        CODEX_APPS_MCP_SERVER_NAME.to_string(),
        AsyncManagedClient {
            client: pending_client,
            is_codex_apps_mcp_server: true,
            cached_server_info: None,
            codex_apps_tools_cache_context: Some(cache_context),
            tool_catalog_cache_context: None,
            tool_filter: ToolFilter::default(),
            startup_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            startup_reconnect: None,
            tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
            cancel_token: CancellationToken::new(),
        },
    );

    let timeout_result =
        tokio::time::timeout(Duration::from_millis(10), manager.list_all_tools()).await;
    let tools = timeout_result.expect("shared empty cache should not block");
    assert!(tools.is_empty());
}

#[tokio::test]
async fn list_all_tools_uses_shared_codex_apps_cache_when_client_startup_fails() {
    let codex_home = tempdir().expect("tempdir");
    let cache_context = create_codex_apps_tools_cache_context(
        codex_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    cache_context.store_current_tools_for_test(vec![create_test_tool(
        CODEX_APPS_MCP_SERVER_NAME,
        "calendar_create_event",
    )]);
    let server_info = create_test_server_info("Codex Apps");
    let failed_client = futures::future::ready::<Result<ManagedClient, StartupOutcomeError>>(Err(
        StartupOutcomeError::Failed {
            error: "startup failed".to_string(),
            is_authentication_required: false,
        },
    ))
    .boxed()
    .shared();
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    let startup_complete = Arc::new(std::sync::atomic::AtomicBool::new(true));
    manager.clients.insert(
        CODEX_APPS_MCP_SERVER_NAME.to_string(),
        AsyncManagedClient {
            client: failed_client,
            is_codex_apps_mcp_server: true,
            cached_server_info: Some(server_info.clone()),
            codex_apps_tools_cache_context: Some(cache_context),
            tool_catalog_cache_context: None,
            tool_filter: ToolFilter::default(),
            startup_complete,
            startup_reconnect: None,
            tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
            cancel_token: CancellationToken::new(),
        },
    );

    let tools = manager.list_all_tools().await;
    let tool = tools
        .iter()
        .find(|tool| {
            tool.canonical_tool_name()
                == ToolName::namespaced("mcp__codex_apps", "calendar_create_event")
        })
        .expect("tool from shared cache");
    assert_eq!(tool.server_name, CODEX_APPS_MCP_SERVER_NAME);
    assert_eq!(tool.callable_name, "calendar_create_event");
    assert_eq!(
        manager
            .list_available_server_infos()
            .await
            .get(CODEX_APPS_MCP_SERVER_NAME),
        Some(&server_info)
    );
}

#[tokio::test]
async fn list_all_tools_reconnects_failed_codex_apps_startup_and_reuses_client() {
    let recovered_client = create_test_managed_client(vec![create_test_tool(
        CODEX_APPS_MCP_SERVER_NAME,
        "drive_search",
    )])
    .await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_reconnect = Arc::clone(&attempts);
    let reconnect_finished = Arc::new(tokio::sync::Notify::new());
    let reconnect_finished_for_factory = Arc::clone(&reconnect_finished);
    let reconnect_factory = Arc::new(move || {
        attempts_for_reconnect.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let reconnect_finished = Arc::clone(&reconnect_finished_for_factory);
        let recovered_client = recovered_client.clone();
        async move {
            reconnect_finished.notify_one();
            Ok(recovered_client)
        }
        .boxed()
        .shared()
    });
    let manager = create_test_manager_with_failed_apps_startup(Vec::new(), reconnect_factory);

    let reconnect_finished_wait = reconnect_finished.notified();
    let tools = manager.list_all_tools().await;
    assert!(tools.is_empty());
    reconnect_finished_wait.await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while manager.catalog_revision().await < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("reconnect catalog publication");
    assert_eq!(manager.catalog_revision().await, 2);

    let tools = manager.list_all_tools().await;
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>(),
        vec!["drive_search"]
    );
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);

    let tools = manager.list_all_tools().await;
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>(),
        vec!["drive_search"]
    );
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn later_tool_list_retries_after_failed_reconnect_and_keeps_cached_tools() {
    let recovered_client = create_test_managed_client(vec![create_test_tool(
        CODEX_APPS_MCP_SERVER_NAME,
        "drive_search",
    )])
    .await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_reconnect = Arc::clone(&attempts);
    let reconnect_finished = Arc::new(tokio::sync::Notify::new());
    let reconnect_finished_for_factory = Arc::clone(&reconnect_finished);
    let reconnect_factory = Arc::new(move || {
        let attempt = attempts_for_reconnect.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let reconnect_finished = Arc::clone(&reconnect_finished_for_factory);
        let recovered_client = recovered_client.clone();
        async move {
            let result = if attempt < 2 {
                Err(StartupOutcomeError::Failed {
                    error: "recreated startup failed".to_string(),
                    is_authentication_required: false,
                })
            } else {
                Ok(recovered_client)
            };
            reconnect_finished.notify_one();
            result
        }
        .boxed()
        .shared()
    });
    let manager = create_test_manager_with_failed_apps_startup(
        vec![create_test_tool(
            CODEX_APPS_MCP_SERVER_NAME,
            "cached_drive_search",
        )],
        reconnect_factory,
    );

    let first_reconnect_finished = reconnect_finished.notified();
    let tools = manager.list_all_tools().await;
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>(),
        vec!["cached_drive_search"]
    );
    first_reconnect_finished.await;
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(manager.catalog_revision().await, 1);

    let tools = manager.list_all_tools().await;
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>(),
        vec!["cached_drive_search"]
    );
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);

    tokio::time::advance(CODEX_APPS_RECONNECT_INITIAL_BACKOFF).await;
    let second_reconnect_finished = reconnect_finished.notified();
    let tools = manager.list_all_tools().await;
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>(),
        vec!["cached_drive_search"]
    );
    second_reconnect_finished.await;
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(manager.catalog_revision().await, 1);

    tokio::time::advance(CODEX_APPS_RECONNECT_INITIAL_BACKOFF).await;
    let tools = manager.list_all_tools().await;
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>(),
        vec!["cached_drive_search"]
    );
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);

    tokio::time::advance(CODEX_APPS_RECONNECT_INITIAL_BACKOFF).await;
    let third_reconnect_finished = reconnect_finished.notified();
    let tools = manager.list_all_tools().await;
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>(),
        vec!["cached_drive_search"]
    );
    third_reconnect_finished.await;
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
    tokio::time::timeout(Duration::from_secs(1), async {
        while manager.catalog_revision().await < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("successful retry catalog publication");
    assert_eq!(manager.catalog_revision().await, 2);

    let tools = manager.list_all_tools().await;
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>(),
        vec!["drive_search"]
    );
}

#[tokio::test]
async fn tool_lists_do_not_block_and_share_codex_apps_startup_reconnect() {
    let recovered_client = create_test_managed_client(vec![create_test_tool(
        CODEX_APPS_MCP_SERVER_NAME,
        "drive_search",
    )])
    .await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_reconnect = Arc::clone(&attempts);
    let reconnect_started = Arc::new(tokio::sync::Notify::new());
    let reconnect_started_for_factory = Arc::clone(&reconnect_started);
    let release_reconnect = Arc::new(tokio::sync::Notify::new());
    let release_reconnect_for_factory = Arc::clone(&release_reconnect);
    let reconnect_factory = Arc::new(move || {
        let recovered_client = recovered_client.clone();
        let attempts = Arc::clone(&attempts_for_reconnect);
        let reconnect_started = Arc::clone(&reconnect_started_for_factory);
        let release_reconnect = Arc::clone(&release_reconnect_for_factory);
        async move {
            attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            reconnect_started.notify_one();
            release_reconnect.notified().await;
            Ok(recovered_client)
        }
        .boxed()
        .shared()
    });
    let manager = Arc::new(create_test_manager_with_failed_apps_startup(
        vec![create_test_tool(
            CODEX_APPS_MCP_SERVER_NAME,
            "cached_drive_search",
        )],
        reconnect_factory,
    ));
    let reconnect_started_wait = reconnect_started.notified();
    let first_tools = tokio::time::timeout(Duration::from_millis(10), manager.list_all_tools())
        .await
        .expect("cached tools should not wait for reconnect");

    reconnect_started_wait.await;
    let second_tools = tokio::time::timeout(Duration::from_millis(10), manager.list_all_tools())
        .await
        .expect("concurrent cached tools should not wait for reconnect");
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        first_tools
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>(),
        vec!["cached_drive_search"]
    );
    assert_eq!(
        second_tools
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>(),
        vec!["cached_drive_search"]
    );

    release_reconnect.notify_one();
    tokio::task::yield_now().await;
    let tools = manager.list_all_tools().await;
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>(),
        vec!["drive_search"]
    );
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn list_all_tools_adds_server_metadata_to_tools() {
    let server_name = "docs";
    let managed_client =
        create_ready_async_managed_client(vec![create_test_tool(server_name, "search")]).await;
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    manager.server_metadata.insert(
        server_name.to_string(),
        McpServerMetadata {
            environment_id: codex_config::DEFAULT_MCP_SERVER_ENVIRONMENT_ID.to_string(),
            pollutes_memory: true,
            origin: Some(McpServerOrigin::StreamableHttp(
                "https://docs.example".to_string(),
            )),
            supports_parallel_tool_calls: true,
            default_tools_approval_mode: None,
            tool_approval_modes: HashMap::new(),
        },
    );
    manager
        .clients
        .insert(server_name.to_string(), managed_client);

    let tools = manager.list_all_tools().await;
    assert_eq!(tools.len(), 1);
    let tool = &tools[0];
    assert_eq!(tool.server_name, server_name);
    assert!(tool.supports_parallel_tool_calls);
    assert_eq!(tool.server_origin.as_deref(), Some("https://docs.example"));
}

#[tokio::test]
async fn model_catalog_uses_live_tools_instead_of_shared_cached_tools() {
    let server_name = CODEX_APPS_MCP_SERVER_NAME;
    let cached_tool = create_test_tool(server_name, "search");
    let mut live_tool = create_test_tool(server_name, "search");
    let mut meta = MetaObject::new();
    meta.0
        .insert("ui".to_string(), serde_json::json!({ "visibility": [] }));
    live_tool.tool.meta = Some(meta);

    let codex_home = tempdir().expect("tempdir");
    let cache_context = create_codex_apps_tools_cache_context(
        codex_home.path().to_path_buf(),
        Some("dispatch-account"),
        Some("dispatch-user"),
    );
    cache_context.store_current_tools_for_test(vec![cached_tool]);
    let live_client = create_test_managed_client(vec![live_tool]).await;
    let managed_client = AsyncManagedClient {
        client: futures::future::ready::<Result<ManagedClient, StartupOutcomeError>>(Ok(
            live_client,
        ))
        .boxed()
        .shared(),
        is_codex_apps_mcp_server: true,
        cached_server_info: None,
        codex_apps_tools_cache_context: Some(cache_context),
        tool_catalog_cache_context: None,
        tool_filter: ToolFilter::default(),
        startup_complete: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        startup_reconnect: None,
        tool_plugin_provenance: Arc::new(ToolPluginProvenance::default()),
        cancel_token: CancellationToken::new(),
    };
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    manager
        .clients
        .insert(server_name.to_string(), managed_client);

    assert_eq!(
        manager
            .list_all_tools()
            .await
            .into_iter()
            .map(|tool| {
                (
                    tool.tool.name.to_string(),
                    tool_is_model_visible(&tool),
                )
            })
            .collect::<Vec<_>>(),
        vec![("search".to_string(), false)]
    );
    assert!(
        manager
            .model_visible_tool_info(server_name, "search")
            .await
            .is_none()
    );
    assert!(
        manager
            .model_visible_tool_info(server_name, "missing")
            .await
            .is_none()
    );
}

#[test]
fn server_metadata_preserves_tool_approval_policy() {
    let mut config = crate::codex_apps_mcp_server_config(
        "https://docs.example",
        /*apps_mcp_product_sku*/ None,
        /*originator*/ None,
    );
    config.environment_id = "remote".to_string();
    config.default_tools_approval_mode = Some(AppToolApproval::Prompt);
    config.tools.insert(
        "search".to_string(),
        McpServerToolConfig {
            approval_mode: Some(AppToolApproval::Approve),
        },
    );
    let metadata = McpServerMetadata::from(&EffectiveMcpServer::configured(config));

    assert_eq!(metadata.environment_id, "remote");
    assert_eq!(metadata.tool_approval_mode("read"), AppToolApproval::Prompt);
    assert_eq!(
        metadata.tool_approval_mode("search"),
        AppToolApproval::Approve
    );
}

#[test]
fn host_owned_codex_apps_requires_server_metadata() {
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );

    assert!(!manager.is_host_owned_codex_apps_server(CODEX_APPS_MCP_SERVER_NAME));
}

#[test]
fn host_owned_codex_apps_matches_reserved_name_with_server_metadata() {
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let permission_profile = Constrained::allow_any(PermissionProfile::default());
    let mut manager = McpConnectionManager::new_uninitialized(
        &approval_policy,
        &permission_profile,
        /*prefix_mcp_tool_names*/ true,
    );
    let server = EffectiveMcpServer::configured(crate::codex_apps_mcp_server_config(
        "https://chatgpt.com",
        /*apps_mcp_product_sku*/ None,
        /*originator*/ None,
    ));
    manager.server_metadata.insert(
        CODEX_APPS_MCP_SERVER_NAME.to_string(),
        McpServerMetadata::from(&server),
    );

    assert!(manager.is_host_owned_codex_apps_server(CODEX_APPS_MCP_SERVER_NAME));
    assert!(!manager.is_host_owned_codex_apps_server("docs"));
}

#[tokio::test]
async fn no_local_runtime_fails_local_stdio_but_keeps_local_http_server() {
    let approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
    let (tx_event, rx_event) = async_channel::unbounded();
    drop(rx_event);
    let codex_home = tempdir().expect("tempdir");
    let mcp_servers = HashMap::from([
        (
            "stdio".to_string(),
            EffectiveMcpServer::configured(McpServerConfig {
                auth: Default::default(),
                transport: McpServerTransportConfig::Stdio {
                    command: "echo".to_string(),
                    args: Vec::new(),
                    env: None,
                    env_vars: Vec::new(),
                    cwd: None,
                },
                environment_id: codex_config::DEFAULT_MCP_SERVER_ENVIRONMENT_ID.to_string(),
                enabled: true,
                required: false,
                supports_parallel_tool_calls: false,
                disabled_reason: None,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                default_tools_approval_mode: None,
                enabled_tools: None,
                disabled_tools: None,
                scopes: None,
                oauth: None,
                oauth_resource: None,
                tools: HashMap::new(),
            }),
        ),
        (
            "http".to_string(),
            EffectiveMcpServer::configured(McpServerConfig {
                auth: Default::default(),
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "http://127.0.0.1:1".to_string(),
                    bearer_token_env_var: None,
                    http_headers: None,
                    env_http_headers: None,
                },
                environment_id: codex_config::DEFAULT_MCP_SERVER_ENVIRONMENT_ID.to_string(),
                enabled: true,
                required: false,
                supports_parallel_tool_calls: false,
                disabled_reason: None,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                default_tools_approval_mode: None,
                enabled_tools: None,
                disabled_tools: None,
                scopes: None,
                oauth: None,
                oauth_resource: None,
                tools: HashMap::new(),
            }),
        ),
    ]);

    let cancel_token = CancellationToken::new();
    let manager = McpConnectionManager::new(
        &mcp_servers,
        OAuthCredentialsStoreMode::default(),
        AuthKeyringBackendKind::default(),
        HashMap::new(),
        &approval_policy,
        String::new(),
        tx_event,
        cancel_token.clone(),
        PermissionProfile::default(),
        McpRuntimeContext::new(
            Arc::new(EnvironmentManager::without_environments()),
            PathBuf::from("/tmp"),
        ),
        codex_home.path().to_path_buf(),
        CodexAppsToolsCache::default(),
        McpToolCatalogCache::default(),
        CodexAppsToolsCacheKey {
            account_id: None,
            chatgpt_user_id: None,
            is_workspace_account: false,
        },
        /*prefix_mcp_tool_names*/ true,
        McpProtocolMode::Legacy,
        ElicitationCapability::default(),
        /*supports_openai_form_elicitation*/ false,
        ToolPluginProvenance::default(),
        /*auth*/ None,
        /*codex_apps_auth_manager*/ None,
        /*elicitation_reviewer*/ None,
        /*elicitation_lifecycle*/ None,
        ElicitationRequestRouter::default(),
    )
    .await;

    assert!(manager.clients.contains_key("stdio"));
    assert!(manager.clients.contains_key("http"));
    assert!(
        !manager
            .wait_for_server_ready("stdio", Duration::from_millis(10))
            .await
    );
    let error = match manager
        .clients
        .get("stdio")
        .expect("stdio client")
        .client()
        .await
    {
        Ok(_) => panic!("local stdio MCP startup should fail"),
        Err(error) => error,
    };
    assert_eq!(
        startup_outcome_error_message(error),
        "local stdio MCP server `stdio` requires a local environment"
    );
    cancel_token.cancel();
}

#[test]
fn elicitation_capability_uses_2025_06_18_shape_for_form_only_support() {
    let capability = Some(ElicitationCapability::default());
    assert_eq!(
        serde_json::to_value(capability).expect("serialize elicitation capability"),
        serde_json::json!({})
    );
}

#[test]
fn elicitation_capability_advertises_url_support_when_enabled() {
    let capability = Some(
        ElicitationCapability::new()
            .with_form(rmcp::model::FormElicitationCapability::new())
            .with_url(rmcp::model::UrlElicitationCapability::new()),
    );
    assert_eq!(
        serde_json::to_value(capability).expect("serialize elicitation capability"),
        serde_json::json!({
            "form": {},
            "url": {},
        })
    );
}

#[test]
fn mcp_init_error_display_prompts_for_github_pat() {
    let server_name = "github";
    let entry = McpAuthStatusEntry {
        config: Some(McpServerConfig {
            auth: Default::default(),
            transport: McpServerTransportConfig::StreamableHttp {
                url: "https://api.githubcopilot.com/mcp/".to_string(),
                bearer_token_env_var: None,
                http_headers: None,
                env_http_headers: None,
            },
            environment_id: codex_config::DEFAULT_MCP_SERVER_ENVIRONMENT_ID.to_string(),
            enabled: true,
            required: false,
            supports_parallel_tool_calls: false,
            disabled_reason: None,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            default_tools_approval_mode: None,
            enabled_tools: None,
            disabled_tools: None,
            scopes: None,
            oauth: None,
            oauth_resource: None,
            tools: HashMap::new(),
        }),
        auth_state: McpAuthState::Unsupported,
    };
    let err: StartupOutcomeError = anyhow::anyhow!("OAuth is unsupported").into();

    let display = mcp_init_error_display(server_name, Some(&entry), &err);

    let expected = format!(
        "GitHub MCP does not support OAuth. Log in by adding a personal access token (https://github.com/settings/personal-access-tokens) to your environment and config.toml:\n[mcp_servers.{server_name}]\nbearer_token_env_var = CODEX_GITHUB_PERSONAL_ACCESS_TOKEN"
    );

    assert_eq!(expected, display);
}

#[test]
fn mcp_init_error_display_prompts_for_login_when_auth_required() {
    let server_name = "example";
    let err: StartupOutcomeError = anyhow::anyhow!("Auth required for server").into();

    let display = mcp_init_error_display(server_name, /*entry*/ None, &err);

    let expected = format!(
        "The {server_name} MCP server is not logged in. Run `better-codex mcp login {server_name}`."
    );

    assert_eq!(expected, display);
}

#[test]
fn mcp_startup_failure_reason_requires_existing_oauth_and_auth_failure() {
    for (auth_state, is_authentication_required, expected) in [
        (
            Some(McpAuthState::LoggedOut(
                McpLoginRequirement::Reauthentication,
            )),
            true,
            Some(McpStartupFailureReason::ReauthenticationRequired),
        ),
        (
            Some(McpAuthState::LoggedOut(
                McpLoginRequirement::Reauthentication,
            )),
            false,
            None,
        ),
        (
            Some(McpAuthState::LoggedOut(McpLoginRequirement::Login)),
            true,
            None,
        ),
        (Some(McpAuthState::Unsupported), true, None),
        (Some(McpAuthState::BearerToken), true, None),
        (Some(McpAuthState::OAuth), true, None),
        (None, true, None),
    ] {
        let entry = auth_state.map(|auth_state| McpAuthStatusEntry {
            config: None,
            auth_state,
        });
        let error = StartupOutcomeError::Failed {
            error: "startup failed".to_string(),
            is_authentication_required,
        };

        assert_eq!(
            mcp_startup_failure_reason(entry.as_ref(), &error),
            expected,
            "auth_state={auth_state:?}, is_authentication_required={is_authentication_required}"
        );
    }
}

#[test]
fn mcp_init_error_display_reports_generic_errors() {
    let server_name = "custom";
    let entry = McpAuthStatusEntry {
        config: Some(McpServerConfig {
            auth: Default::default(),
            transport: McpServerTransportConfig::StreamableHttp {
                url: "https://example.com".to_string(),
                bearer_token_env_var: Some("TOKEN".to_string()),
                http_headers: None,
                env_http_headers: None,
            },
            environment_id: codex_config::DEFAULT_MCP_SERVER_ENVIRONMENT_ID.to_string(),
            enabled: true,
            required: false,
            supports_parallel_tool_calls: false,
            disabled_reason: None,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            default_tools_approval_mode: None,
            enabled_tools: None,
            disabled_tools: None,
            scopes: None,
            oauth: None,
            oauth_resource: None,
            tools: HashMap::new(),
        }),
        auth_state: McpAuthState::Unsupported,
    };
    let err: StartupOutcomeError = anyhow::anyhow!("boom").into();

    let display = mcp_init_error_display(server_name, Some(&entry), &err);

    let expected = format!("MCP client for `{server_name}` failed to start: {err:#}");

    assert_eq!(expected, display);
}

#[test]
fn mcp_init_error_display_includes_startup_timeout_hint() {
    let server_name = "slow";
    for error in [
        "request timed out",
        "MCP client startup timed out after 30s",
    ] {
        let err: StartupOutcomeError = anyhow::anyhow!(error).into();

        let display = mcp_init_error_display(server_name, /*entry*/ None, &err);

        assert_eq!(
            "MCP client for `slow` timed out after 30 seconds. Add or adjust `startup_timeout_sec` in your config.toml:\n[mcp_servers.slow]\nstartup_timeout_sec = XX",
            display
        );
    }
}
