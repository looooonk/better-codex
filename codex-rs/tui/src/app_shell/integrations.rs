use super::dashboard::format_usize;
use codex_app_server_protocol::ListMcpServerStatusResponse;
use codex_app_server_protocol::PluginListResponse;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct McpInventorySummary {
    loaded: bool,
    servers: usize,
    error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PluginInventorySummary {
    loaded: bool,
    installed: usize,
    error: Option<String>,
}

impl McpInventorySummary {
    pub(super) fn from_response(response: &ListMcpServerStatusResponse) -> Self {
        Self { loaded: true, servers: response.data.len(), ..Self::default() }
    }

    pub(super) fn from_error(error: impl Into<String>) -> Self {
        Self {
            loaded: true,
            error: Some(error.into()),
            ..Self::default()
        }
    }

    pub(super) fn label(&self) -> String {
        if let Some(error) = &self.error {
            return format!("error: {error}");
        }
        if !self.loaded {
            return "not loaded".to_string();
        }
        let noun = if self.servers == 1 { "server" } else { "servers" };
        format!("{} {noun}", format_usize(self.servers))
    }
}

impl PluginInventorySummary {
    pub(super) fn from_response(response: &PluginListResponse) -> Self {
        Self {
            loaded: true,
            installed: response.marketplaces.iter().flat_map(|m| &m.plugins).filter(|p| p.installed).count(),
            ..Self::default()
        }
    }

    pub(super) fn from_error(error: impl Into<String>) -> Self {
        Self {
            loaded: true,
            error: Some(error.into()),
            ..Self::default()
        }
    }

    pub(super) fn label(&self) -> String {
        if let Some(error) = &self.error {
            return format!("error: {error}");
        }
        if !self.loaded {
            return "not loaded".to_string();
        }
        format!("{} installed", format_usize(self.installed))
    }
}
