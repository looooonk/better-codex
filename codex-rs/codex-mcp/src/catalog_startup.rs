use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::future::join_all;
use tracing::trace;

use super::McpConnectionManager;
use crate::mcp::CODEX_APPS_MCP_SERVER_NAME;

const OPTIONAL_MCP_STARTUP_GRACE: Duration = Duration::from_secs(1);

pub(super) async fn wait_for_catalog_startup(manager: &McpConnectionManager) {
    let default_deadline = *manager
        .optional_startup_deadline
        .get_or_init(|| tokio::time::Instant::now() + OPTIONAL_MCP_STARTUP_GRACE);
    join_all(manager.clients.iter().map(|(server_name, client)| async move {
        if client.startup_complete.load(Ordering::Acquire) {
            return;
        }

        let has_cached_tools = client.has_cached_tools();
        let must_wait_for_startup = manager.required_servers.binary_search(server_name).is_ok()
            || manager.is_selected_plugin_mcp_server(server_name)
            || (server_name == CODEX_APPS_MCP_SERVER_NAME && !has_cached_tools);
        if !must_wait_for_startup && has_cached_tools {
            return;
        }
        if !must_wait_for_startup {
            let startup_deadline = client
                .tool_catalog_cache_context
                .as_ref()
                .map(|cache| cache.optional_startup_deadline(default_deadline))
                .unwrap_or(default_deadline);
            if tokio::time::timeout_at(startup_deadline, client.client())
                .await
                .is_err()
            {
                trace!(server_name = %server_name, "omitting pending optional MCP server");
            }
            return;
        }
        let _ = client.client().await;
    }))
    .await;
}
