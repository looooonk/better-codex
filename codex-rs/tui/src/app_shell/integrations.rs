use super::dashboard::format_usize;
use codex_app_server_protocol::ListMcpServerStatusResponse;
use codex_app_server_protocol::McpAuthStatus;
use codex_app_server_protocol::PluginListResponse;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct McpInventorySummary {
    loaded: bool,
    servers: usize,
    tools: usize,
    not_logged_in: usize,
    error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PluginInventorySummary {
    loaded: bool,
    plugins: usize,
    installed: usize,
    errors: usize,
    error: Option<String>,
}

impl McpInventorySummary {
    pub(super) fn from_response(response: &ListMcpServerStatusResponse) -> Self {
        let mut summary = Self {
            loaded: true,
            servers: response.data.len(),
            ..Self::default()
        };
        for server in &response.data {
            summary.tools += server.tools.len();
            match server.auth_status {
                McpAuthStatus::OAuth | McpAuthStatus::BearerToken | McpAuthStatus::Unsupported => {}
                McpAuthStatus::NotLoggedIn => summary.not_logged_in += 1,
            }
        }
        summary
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
        let mut parts = vec![
            format!("{} servers", format_usize(self.servers)),
            format!("{} tools", format_usize(self.tools)),
        ];
        if self.not_logged_in > 0 {
            parts.push(format!("{} login needed", format_usize(self.not_logged_in)));
        }
        parts.join(" / ")
    }
}

impl PluginInventorySummary {
    pub(super) fn from_response(response: &PluginListResponse) -> Self {
        let mut summary = Self {
            loaded: true,
            errors: response.marketplace_load_errors.len(),
            ..Self::default()
        };
        for marketplace in &response.marketplaces {
            summary.plugins += marketplace.plugins.len();
            for plugin in &marketplace.plugins {
                if plugin.installed {
                    summary.installed += 1;
                }
            }
        }
        summary
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
        let mut parts = vec![
            format!("{} installed", format_usize(self.installed)),
            format!("{} available", format_usize(self.plugins)),
        ];
        if self.errors > 0 {
            parts.push(format!("{} catalog errors", format_usize(self.errors)));
        }
        parts.join(" / ")
    }
}
