use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Barrier;

use codex_config::Constrained;
use codex_mcp::CODEX_APPS_MCP_SERVER_NAME;
use codex_mcp::McpBinding;
use codex_mcp::McpConnectionManager;
use codex_mcp::ToolInfo;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_tools::ToolExposure;
use codex_tools::ToolExecutor;
use codex_tools::ToolName;
use pretty_assertions::assert_eq;
use rmcp::model::JsonObject;
use rmcp::model::MetaObject;
use rmcp::model::Tool;

use super::*;
use crate::config::CONFIG_TOML_FILE;
use crate::config::ConfigBuilder;
use crate::config::test_config;
use crate::connectors::AppInfo;
use tempfile::tempdir;

fn make_connector(id: &str, name: &str) -> AppInfo {
    AppInfo {
        id: id.to_string(),
        name: name.to_string(),
        description: None,
        logo_url: None,
        logo_url_dark: None,
        icon_assets: None,
        icon_dark_assets: None,
        distribution_channel: None,
        branding: None,
        app_metadata: None,
        labels: None,
        install_url: None,
        is_accessible: true,
        is_enabled: true,
        plugin_display_names: Vec::new(),
    }
}

fn make_mcp_tool(
    server_name: &str,
    tool_name: &str,
    callable_namespace: &str,
    callable_name: &str,
    connector_id: Option<&str>,
    connector_name: Option<&str>,
) -> ToolInfo {
    ToolInfo {
        server_name: server_name.to_string(),
        supports_parallel_tool_calls: false,
        server_origin: None,
        callable_name: callable_name.to_string(),
        callable_namespace: callable_namespace.to_string(),
        namespace_description: None,
        tool: Tool::new(
            tool_name.to_string(),
            format!("Test tool: {tool_name}"),
            Arc::new(JsonObject::default()),
        ),
        openai_file_input_optional_fields: Default::default(),
        connector_id: connector_id.map(str::to_string),
        connector_name: connector_name.map(str::to_string),
        plugin_display_names: Vec::new(),
    }
}

fn numbered_mcp_tools(count: usize) -> Vec<ToolInfo> {
    (0..count)
        .map(|index| {
            let tool_name = format!("tool_{index}");
            make_mcp_tool(
                "rmcp",
                &tool_name,
                "mcp__rmcp",
                &tool_name,
                /*connector_id*/ None,
                /*connector_name*/ None,
            )
        })
        .collect()
}

fn expected_runtimes(
    tools: &[ToolInfo],
    exposure: ToolExposure,
) -> HashMap<ToolName, ToolExposure> {
    tools
        .iter()
        .map(|tool| (tool.canonical_tool_name(), exposure))
        .collect()
}

async fn empty_binding() -> Arc<McpBinding> {
    let manager = Arc::new(McpConnectionManager::new_uninitialized_with_permission_profile(
        &Constrained::allow_any(AskForApproval::OnRequest),
        &PermissionProfile::default(),
        /*prefix_mcp_tool_names*/ true,
    ));
    McpBinding::capture(manager).await
}

fn runtimes_by_name(runtimes: &[Arc<dyn CoreToolRuntime>]) -> HashMap<ToolName, ToolExposure> {
    runtimes
        .iter()
        .map(|runtime| (runtime.tool_name(), runtime.exposure()))
        .collect()
}

fn with_visibility(mut tool: ToolInfo, visibility: &[&str]) -> ToolInfo {
    tool.tool.meta = Some(MetaObject(
        serde_json::json!({ "ui": { "visibility": visibility } })
            .as_object()
            .expect("metadata object")
            .clone(),
    ));
    tool
}

#[tokio::test]
async fn directly_exposes_effective_tool_sets_when_search_is_unavailable() {
    let config = test_config().await;
    let mcp_tools = numbered_mcp_tools(/*count*/ 2);

    let runtimes = build_mcp_tool_runtimes(
        &mcp_tools, /*connectors*/ None, &config, /*search_tool_enabled*/ false,
    );

    assert_eq!(
        runtimes_by_name(&runtimes),
        expected_runtimes(&mcp_tools, ToolExposure::Direct)
    );
}

#[tokio::test]
async fn excludes_tools_hidden_from_model_exposure() {
    let config = test_config().await;
    let visible_tool = make_mcp_tool(
        "rmcp",
        "visible_tool",
        "mcp__rmcp",
        "visible_tool",
        /*connector_id*/ None,
        /*connector_name*/ None,
    );
    let hidden_tool = with_visibility(
        make_mcp_tool(
            "rmcp",
            "hidden_tool",
            "mcp__rmcp",
            "hidden_tool",
            /*connector_id*/ None,
            /*connector_name*/ None,
        ),
        &["app"],
    );
    let empty_visibility_tool = with_visibility(
        make_mcp_tool(
            "rmcp",
            "empty_visibility_tool",
            "mcp__rmcp",
            "empty_visibility_tool",
            /*connector_id*/ None,
            /*connector_name*/ None,
        ),
        &[],
    );
    let visible_app_tool = with_visibility(
        make_mcp_tool(
            CODEX_APPS_MCP_SERVER_NAME,
            "calendar_read",
            "mcp__codex_apps__calendar",
            "read",
            Some("calendar"),
            Some("Calendar"),
        ),
        &["app", "model"],
    );
    let hidden_app_tool = with_visibility(
        make_mcp_tool(
            CODEX_APPS_MCP_SERVER_NAME,
            "calendar_open",
            "mcp__codex_apps__calendar",
            "open",
            Some("calendar"),
            Some("Calendar"),
        ),
        &["app"],
    );
    let mcp_tools = vec![
        visible_tool.clone(),
        hidden_tool,
        empty_visibility_tool,
        visible_app_tool.clone(),
        hidden_app_tool,
    ];
    let connectors = vec![make_connector("calendar", "Calendar")];

    let runtimes = build_mcp_tool_runtimes(
        &mcp_tools,
        Some(connectors.as_slice()),
        &config,
        /*search_tool_enabled*/ false,
    );

    assert_eq!(
        runtimes_by_name(&runtimes),
        expected_runtimes(&[visible_tool, visible_app_tool], ToolExposure::Direct)
    );
}

#[tokio::test]
async fn applies_per_tool_app_policy_across_the_exposure_build() {
    let codex_home = tempdir().expect("tempdir should succeed");
    std::fs::write(
        codex_home.path().join(CONFIG_TOML_FILE),
        r#"
[apps.calendar]
default_tools_enabled = false

[apps.calendar.tools."events/create"]
enabled = true
"#,
    )
    .expect("write config");
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .build()
        .await
        .expect("config should build");
    let enabled_tool = make_mcp_tool(
        CODEX_APPS_MCP_SERVER_NAME,
        "events/create",
        "mcp__codex_apps__calendar",
        "create",
        Some("calendar"),
        Some("Calendar"),
    );
    let disabled_tool = make_mcp_tool(
        CODEX_APPS_MCP_SERVER_NAME,
        "events/list",
        "mcp__codex_apps__calendar",
        "list",
        Some("calendar"),
        Some("Calendar"),
    );
    let connectors = vec![make_connector("calendar", "Calendar")];

    let runtimes = build_mcp_tool_runtimes(
        &[enabled_tool.clone(), disabled_tool],
        Some(connectors.as_slice()),
        &config,
        /*search_tool_enabled*/ false,
    );

    assert_eq!(
        runtimes_by_name(&runtimes),
        expected_runtimes(&[enabled_tool], ToolExposure::Direct)
    );
}

#[tokio::test]
async fn defers_effective_tool_sets_when_search_is_available() {
    let config = test_config().await;
    let mcp_tools = numbered_mcp_tools(/*count*/ 2);

    let runtimes = build_mcp_tool_runtimes(
        &mcp_tools, /*connectors*/ None, &config, /*search_tool_enabled*/ true,
    );

    assert_eq!(
        runtimes_by_name(&runtimes),
        expected_runtimes(&mcp_tools, ToolExposure::Deferred)
    );
}

#[tokio::test]
async fn defers_apps_and_non_app_mcp_tools() {
    let config = test_config().await;
    let mcp_tools = vec![
        make_mcp_tool(
            "rmcp",
            "tool",
            "mcp__rmcp",
            "tool",
            /*connector_id*/ None,
            /*connector_name*/ None,
        ),
        make_mcp_tool(
            CODEX_APPS_MCP_SERVER_NAME,
            "calendar_create_event",
            "mcp__codex_apps__calendar",
            "_create_event",
            Some("calendar"),
            Some("Calendar"),
        ),
    ];
    let connectors = vec![make_connector("calendar", "Calendar")];

    let runtimes = build_mcp_tool_runtimes(
        &mcp_tools,
        Some(connectors.as_slice()),
        &config,
        /*search_tool_enabled*/ true,
    );

    assert_eq!(
        runtimes_by_name(&runtimes),
        expected_runtimes(&mcp_tools, ToolExposure::Deferred)
    );
}

#[tokio::test]
async fn handler_cache_reuses_only_the_current_binding() {
    let cache = McpHandlerCache::default();
    let first_binding = empty_binding().await;
    let tool = make_mcp_tool(
        "rmcp",
        "tool",
        "mcp__rmcp",
        "tool",
        /*connector_id*/ None,
        /*connector_name*/ None,
    );
    let tool_name = tool.canonical_tool_name();
    let first = cache
        .bind(&first_binding)
        .get_or_build(tool.clone())
        .expect("handler should build");
    let first_weak = Arc::downgrade(&first);
    drop(first);

    let reused = cache
        .bind(&first_binding)
        .get_or_build(tool.clone())
        .expect("handler should be reused");
    assert!(Arc::ptr_eq(
        &first_weak.upgrade().expect("cache should retain the handler"),
        &reused,
    ));
    drop(reused);

    let replacement_binding = empty_binding().await;
    let replacement = cache
        .bind(&replacement_binding)
        .get_or_build(tool)
        .expect("replacement handler should build");
    assert!(first_weak.upgrade().is_none());
    assert_eq!(replacement.tool_name(), tool_name);
}

#[tokio::test]
async fn empty_binding_clears_handlers_from_the_previous_catalog() {
    let cache = McpHandlerCache::default();
    let previous_binding = empty_binding().await;
    let previous = cache
        .bind(&previous_binding)
        .get_or_build(
            make_mcp_tool(
                "rmcp",
                "tool",
                "mcp__rmcp",
                "tool",
                /*connector_id*/ None,
                /*connector_name*/ None,
            ),
        )
        .expect("handler should build");
    let previous = Arc::downgrade(&previous);

    let empty_binding = empty_binding().await;
    assert!(
        build_bound_mcp_tool_runtimes(
            empty_binding,
            /*connectors*/ None,
            &test_config().await,
            /*search_tool_enabled*/ false,
            &cache,
        )
        .is_empty()
    );
    assert!(previous.upgrade().is_none());
}

#[tokio::test]
async fn concurrent_bindings_never_share_a_cached_handler() {
    let cache = Arc::new(McpHandlerCache::default());
    let first_binding = empty_binding().await;
    let second_binding = empty_binding().await;
    let tool = make_mcp_tool(
        "rmcp",
        "tool",
        "mcp__rmcp",
        "tool",
        /*connector_id*/ None,
        /*connector_name*/ None,
    );

    for _ in 0..32 {
        let start = Arc::new(Barrier::new(3));
        let first_start = Arc::clone(&start);
        let first_cache = Arc::clone(&cache);
        let first_binding = Arc::clone(&first_binding);
        let first_tool = tool.clone();
        let second_start = Arc::clone(&start);
        let second_cache = Arc::clone(&cache);
        let second_binding = Arc::clone(&second_binding);
        let second_tool = tool.clone();

        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(move || {
                first_start.wait();
                first_cache
                    .bind(&first_binding)
                    .get_or_build(first_tool)
                    .expect("first handler should build")
            });
            let second = scope.spawn(move || {
                second_start.wait();
                second_cache
                    .bind(&second_binding)
                    .get_or_build(second_tool)
                    .expect("second handler should build")
            });
            start.wait();
            (
                first.join().expect("first cache thread"),
                second.join().expect("second cache thread"),
            )
        });

        assert!(!Arc::ptr_eq(&first, &second));
    }
}
