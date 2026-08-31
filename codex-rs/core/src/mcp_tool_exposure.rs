use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

use codex_connectors::AppToolPolicyEvaluator;
use codex_connectors::AppToolPolicyInput;
use codex_mcp::CODEX_APPS_MCP_SERVER_NAME;
use codex_mcp::McpBinding;
use codex_mcp::ToolInfo as McpToolInfo;
use codex_mcp::tool_is_model_visible;
use codex_tools::ToolExposure;
use codex_tools::ToolName;
use tracing::instrument;
use tracing::warn;

use crate::config::Config;
use crate::connectors;
use crate::tools::handlers::McpHandler;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::override_tool_exposure;

#[derive(Default)]
pub(crate) struct McpHandlerCache {
    cached: Mutex<Option<CachedMcpHandlers>>,
}

struct CachedMcpHandlers {
    binding: Weak<McpBinding>,
    handlers: HashMap<ToolName, Arc<McpHandler>>,
}

impl McpHandlerCache {
    fn get_or_build(
        &self,
        binding: &Arc<McpBinding>,
        tool: McpToolInfo,
    ) -> Result<Arc<McpHandler>, serde_json::Error> {
        let tool_name = tool.canonical_tool_name();
        let mut cached = self
            .cached
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !cached
            .as_ref()
            .and_then(|cached| cached.binding.upgrade())
            .is_some_and(|cached_binding| Arc::ptr_eq(&cached_binding, binding))
        {
            *cached = None;
        }

        let cached = cached.get_or_insert_with(|| CachedMcpHandlers {
            binding: Arc::downgrade(binding),
            handlers: HashMap::new(),
        });
        if let Some(handler) = cached.handlers.get(&tool_name) {
            return Ok(Arc::clone(handler));
        }
        let handler = Arc::new(McpHandler::new_bound(tool, Arc::clone(binding))?);
        cached.handlers.insert(tool_name, Arc::clone(&handler));
        Ok(handler)
    }
}

#[instrument(level = "trace", skip_all)]
pub(crate) fn build_mcp_tool_runtimes(
    all_mcp_tools: &[McpToolInfo],
    connectors: Option<&[connectors::AppInfo]>,
    config: &Config,
    search_tool_enabled: bool,
) -> Vec<Arc<dyn CoreToolRuntime>> {
    let exposed_tools = exposed_mcp_tools(all_mcp_tools, connectors, config);
    let exposure = if search_tool_enabled {
        ToolExposure::Deferred
    } else {
        ToolExposure::Direct
    };
    exposed_tools
        .into_iter()
        .filter_map(|tool| {
            let tool_name = tool.canonical_tool_name();
            match McpHandler::new(tool) {
                Ok(handler) => {
                    let handler: Arc<dyn CoreToolRuntime> = Arc::new(handler);
                    Some(override_tool_exposure(handler, exposure))
                }
                Err(err) => {
                    warn!("Skipping MCP tool `{tool_name}`: failed to build tool spec: {err}");
                    None
                }
            }
        })
        .collect()
}

#[instrument(level = "trace", skip_all)]
pub(crate) fn build_bound_mcp_tool_runtimes(
    binding: Arc<McpBinding>,
    connectors: Option<&[connectors::AppInfo]>,
    config: &Config,
    search_tool_enabled: bool,
    cache: &McpHandlerCache,
) -> Vec<Arc<dyn CoreToolRuntime>> {
    let exposed_tools = exposed_mcp_tools(binding.tools(), connectors, config);
    let exposure = if search_tool_enabled {
        ToolExposure::Deferred
    } else {
        ToolExposure::Direct
    };

    exposed_tools
        .into_iter()
        .filter_map(|tool| {
            let tool_name = tool.canonical_tool_name();
            match cache.get_or_build(&binding, tool) {
                Ok(handler) => {
                    let handler: Arc<dyn CoreToolRuntime> = handler;
                    Some(override_tool_exposure(handler, exposure))
                }
                Err(err) => {
                    warn!("Skipping MCP tool `{tool_name}`: failed to build tool spec: {err}");
                    None
                }
            }
        })
        .collect()
}

fn exposed_mcp_tools(
    all_mcp_tools: &[McpToolInfo],
    connectors: Option<&[connectors::AppInfo]>,
    config: &Config,
) -> Vec<McpToolInfo> {
    let mut tools = filter_non_codex_apps_mcp_tools_only(all_mcp_tools);
    if let Some(connectors) = connectors {
        tools.extend(filter_codex_apps_mcp_tools(
            all_mcp_tools,
            connectors,
            config,
        ));
    }
    tools
}

fn filter_non_codex_apps_mcp_tools_only(mcp_tools: &[McpToolInfo]) -> Vec<McpToolInfo> {
    mcp_tools
        .iter()
        .filter(|tool| {
            tool.server_name != CODEX_APPS_MCP_SERVER_NAME && tool_is_model_visible(tool)
        })
        .cloned()
        .collect()
}

fn filter_codex_apps_mcp_tools(
    mcp_tools: &[McpToolInfo],
    connectors: &[connectors::AppInfo],
    config: &Config,
) -> Vec<McpToolInfo> {
    let allowed: HashSet<&str> = connectors
        .iter()
        .map(|connector| connector.id.as_str())
        .collect();
    let app_tool_policy = AppToolPolicyEvaluator::new(&config.config_layer_stack);

    mcp_tools
        .iter()
        .filter(|tool| {
            if tool.server_name != CODEX_APPS_MCP_SERVER_NAME {
                return false;
            }
            if !tool_is_model_visible(tool) {
                return false;
            }
            let Some(connector_id) = tool.connector_id.as_deref() else {
                return false;
            };
            let annotations = tool.tool.annotations.as_ref();
            allowed.contains(connector_id)
                && app_tool_policy
                    .policy(AppToolPolicyInput {
                        connector_id: Some(connector_id),
                        tool_name: &tool.tool.name,
                        tool_title: tool.tool.title.as_deref(),
                        destructive_hint: annotations
                            .and_then(|annotations| annotations.destructive_hint),
                        open_world_hint: annotations
                            .and_then(|annotations| annotations.open_world_hint),
                    })
                    .enabled
        })
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "mcp_tool_exposure_test.rs"]
mod tests;
