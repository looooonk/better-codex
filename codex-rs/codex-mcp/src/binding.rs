//! Revision-bound MCP catalogs used by one or more model sampling steps.

use std::future::Future;
use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
use codex_protocol::mcp::CallToolResult;
use tracing::warn;

use crate::McpConnectionManager;
use crate::ToolInfo;

const MAX_BINDING_CAPTURE_ATTEMPTS: usize = 3;
const UNSTABLE_CATALOG_REVISION: u64 = u64::MAX;

pub struct McpBinding {
    manager: Arc<McpConnectionManager>,
    tools: Arc<[ToolInfo]>,
    catalog_revision: u64,
}

impl McpBinding {
    pub async fn capture(manager: Arc<McpConnectionManager>) -> Arc<Self> {
        for _attempt in 0..MAX_BINDING_CAPTURE_ATTEMPTS {
            let catalog_revision = manager.catalog_revision().await;
            let tools = manager.list_all_tools().await;
            if manager.catalog_revision().await == catalog_revision {
                return Arc::new(Self {
                    manager,
                    tools: tools.into(),
                    catalog_revision,
                });
            }
        }

        warn!("MCP catalog kept changing while capturing a binding; exposing no tools");
        Arc::new(Self {
            catalog_revision: UNSTABLE_CATALOG_REVISION,
            manager,
            tools: Vec::<ToolInfo>::new().into(),
        })
    }

    pub fn tools(&self) -> &[ToolInfo] {
        &self.tools
    }

    pub fn catalog_revision(&self) -> u64 {
        self.catalog_revision
    }

    pub async fn is_current(&self) -> bool {
        self.manager.catalog_revision().await == self.catalog_revision
    }

    pub fn tool(&self, server: &str, tool: &str) -> Option<&ToolInfo> {
        self.tools.iter().find(|info| {
            info.server_name == server
                && info.tool.name == tool
                && crate::tool_is_model_visible(info)
        })
    }

    pub async fn wait_for_server_startup(&self, server: &str) -> bool {
        self.manager.wait_for_server_startup(server).await
    }

    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: Option<serde_json::Value>,
        meta: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        self.call_tool_with_preparation(server, tool, || async move { Ok((arguments, meta)) })
            .await
    }

    #[expect(
        clippy::await_holding_invalid_type,
        reason = "exact target materialization must stay serialized with visibility revalidation"
    )]
    pub async fn call_tool_with_preparation<F, Fut>(
        &self,
        server: &str,
        tool: &str,
        prepare: F,
    ) -> Result<CallToolResult>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(Option<serde_json::Value>, Option<serde_json::Value>)>>,
    {
        let authority = self
            .manager
            .lock_catalog_revision(self.catalog_revision)
            .await?;
        let captured = self
            .tool(server, tool)
            .ok_or_else(|| anyhow!("MCP tool `{server}/{tool}` was not advertised by this step"))?;
        let current = self
            .manager
            .model_visible_tool_info(server, tool)
            .await
            .ok_or_else(|| anyhow!("MCP tool `{server}/{tool}` is no longer model-visible"))?;
        if current.canonical_tool_name() != captured.canonical_tool_name() {
            return Err(anyhow!(
                "MCP tool `{server}/{tool}` changed identity after it was advertised"
            ));
        }
        let call = self.manager.prepare_tool_call(server, tool).await?;
        drop(authority);
        let (arguments, meta) = prepare().await?;
        call.call(arguments, meta).await
    }
}

impl std::fmt::Debug for McpBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpBinding")
            .field("catalog_revision", &self.catalog_revision)
            .field("tool_count", &self.tools.len())
            .finish_non_exhaustive()
    }
}
