use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use codex_config::McpServerConfig;
use codex_exec_server::EnvironmentManager;
use pretty_assertions::assert_eq;
use rmcp::model::ElicitationCapability;
use rmcp::model::JsonObject;
use rmcp::model::Tool;
use rmcp::model::ToolAnnotations;

use super::*;

fn config(value: serde_json::Value) -> McpServerConfig {
    serde_json::from_value(value).expect("MCP config")
}

fn tool(name: &str) -> ToolInfo {
    let mut tool = Tool::new(
        Cow::Owned(name.to_string()),
        Cow::Owned(format!("{name} description")),
        Arc::new(JsonObject::new()),
    );
    tool.annotations = Some(ToolAnnotations::new().read_only(true));
    ToolInfo {
        server_name: "docs".to_string(),
        supports_parallel_tool_calls: false,
        server_origin: None,
        callable_name: name.to_string(),
        callable_namespace: "mcp__docs".to_string(),
        namespace_description: Some("session instructions".to_string()),
        tool,
        openai_file_input_optional_fields: HashMap::new(),
        connector_id: None,
        connector_name: None,
        plugin_display_names: Vec::new(),
    }
}

fn context(
    cache: &McpToolCatalogCache,
    config: &McpServerConfig,
    runtime_context: &McpRuntimeContext,
) -> McpToolCatalogCacheContext {
    cache
        .context(
            "docs",
            config,
            runtime_context,
            /*resolved_environment*/ None,
            &ElicitationCapability::default(),
            /*supports_openai_form_elicitation*/ false,
        )
        .expect("cache context")
}

#[test]
fn cache_reuses_sanitized_newest_catalog_and_honors_server_opt_out() {
    let cache = McpToolCatalogCache::default();
    let runtime_context = McpRuntimeContext::new(
        Arc::new(EnvironmentManager::without_environments()),
        PathBuf::from("/workspace"),
    );
    let config = config(serde_json::json!({ "command": "docs-mcp" }));
    let first = context(&cache, &config, &runtime_context);
    let older = first.begin_fetch();
    let newer = first.begin_fetch();
    first.publish_if_newest(newer, &[tool("new")]);
    first.publish_if_newest(older, &[tool("old")]);
    let cached = context(&cache, &config, &runtime_context)
        .current_tools()
        .expect("cached tools");
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].callable_name, "new");
    assert_eq!(cached[0].namespace_description, None);
    assert_eq!(cached[0].tool.annotations, None);
    let other_runtime = McpRuntimeContext::new(
        Arc::new(EnvironmentManager::without_environments()),
        PathBuf::from("/other-workspace"),
    );
    assert!(!context(&cache, &config, &other_runtime).has_tools());
    first.disable();
    first.publish_if_newest(first.begin_fetch(), &[tool("ignored")]);
    assert_eq!(first.current_tools().map(|tools| tools.len()), None);
}

#[test]
fn cache_bypasses_http_and_remote_sourced_environment_variables() {
    let cache = McpToolCatalogCache::default();
    let runtime_context = McpRuntimeContext::new(
        Arc::new(EnvironmentManager::without_environments()),
        PathBuf::from("/workspace"),
    );
    let configs = [
        config(serde_json::json!({ "url": "https://example.com/mcp" })),
        config(serde_json::json!({
            "command": "docs-mcp",
            "env_vars": [{ "name": "DOCS_TOKEN", "source": "remote" }]
        })),
    ];
    assert!(configs.iter().all(|config| {
        cache
            .context(
                "docs",
                config,
                &runtime_context,
                /*resolved_environment*/ None,
                &ElicitationCapability::default(),
                /*supports_openai_form_elicitation*/ false,
            )
            .is_none()
    }));
}
