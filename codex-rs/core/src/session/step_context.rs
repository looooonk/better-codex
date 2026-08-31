use std::collections::HashMap;
use std::sync::Arc;

use crate::agents_md::LoadedAgentsMd;
use crate::environment_selection::TurnEnvironmentSnapshot;
use crate::session::McpRuntimeSnapshot;
use crate::session::turn_context::TurnContext;
use codex_exec_server::ExecutorCapabilityDiscoverySnapshot;
use codex_exec_server::FileSystemSandboxContext;
use codex_exec_server::ResolvedSelectedCapabilityRoot;
use codex_extension_api::ExtensionData;
use codex_mcp::ToolInfo;
use tokio::sync::OnceCell;

/// Request-scoped state that may change between model sampling requests.
#[derive(Debug)]
pub(crate) struct StepContext {
    pub(crate) turn: Arc<TurnContext>,
    pub(crate) environments: TurnEnvironmentSnapshot,
    /// Capability roots bound to ready environments in this exact step.
    pub(crate) selected_capability_roots: Vec<ResolvedSelectedCapabilityRoot>,
    /// Executor-materialized capability files shared by MCP and skills in this exact step.
    pub(crate) executor_capability_discovery: Option<Arc<ExecutorCapabilityDiscoverySnapshot>>,
    /// The exact MCP config and manager used to advertise and execute tools for this step.
    pub(crate) mcp: Arc<McpRuntimeSnapshot>,
    /// The fixed MCP tool list used for this exact sampling request.
    mcp_tool_snapshot: OnceCell<Vec<ToolInfo>>,
    /// The canonical AGENTS.md value observed with this environment snapshot.
    pub(crate) loaded_agents_md: Option<Arc<LoadedAgentsMd>>,
    /// Extension-owned values captured for this exact sampling request.
    pub(crate) extension_data: ExtensionData,
}

impl StepContext {
    pub(crate) fn new(
        turn: Arc<TurnContext>,
        environments: TurnEnvironmentSnapshot,
        selected_capability_roots: Vec<ResolvedSelectedCapabilityRoot>,
        executor_capability_discovery: Option<Arc<ExecutorCapabilityDiscoverySnapshot>>,
        mcp: Arc<McpRuntimeSnapshot>,
        loaded_agents_md: Option<Arc<LoadedAgentsMd>>,
    ) -> Self {
        let extension_data = ExtensionData::new(turn.sub_id.clone());
        extension_data.insert(selected_capability_roots.clone());
        if let Some(discovery) = &executor_capability_discovery {
            extension_data.insert(discovery.as_ref().clone());
        }
        if !turn
            .config
            .permissions
            .file_system_sandbox_policy()
            .has_full_disk_read_access()
        {
            extension_data.insert(
                environments
                    .turn_environments
                    .iter()
                    .map(|environment| {
                        (
                            environment.environment_id.clone(),
                            turn.file_system_sandbox_context(
                                /*additional_permissions*/ None,
                                environment,
                            ),
                        )
                    })
                    .collect::<HashMap<String, FileSystemSandboxContext>>(),
            );
        }
        Self {
            turn,
            environments,
            selected_capability_roots,
            executor_capability_discovery,
            mcp,
            mcp_tool_snapshot: OnceCell::new(),
            loaded_agents_md,
            extension_data,
        }
    }

    pub(crate) async fn mcp_tools(&self) -> &[ToolInfo] {
        self.mcp_tool_snapshot
            .get_or_init(|| self.mcp.manager().list_all_tools())
            .await
    }
}
