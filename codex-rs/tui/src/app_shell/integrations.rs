use super::dashboard::dashboard_value;
use super::dashboard::format_usize;
use codex_app_server_protocol::ListMcpServerStatusResponse;
use codex_app_server_protocol::McpAuthStatus;
use codex_app_server_protocol::PluginListResponse;
use ratatui::style::Stylize;
use ratatui::text::Line;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct McpInventorySummary {
    loaded: bool,
    servers: usize,
    tools: usize,
    oauth: usize,
    bearer_token: usize,
    not_logged_in: usize,
    unsupported_auth: usize,
    error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PluginInventorySummary {
    loaded: bool,
    marketplaces: usize,
    plugins: usize,
    installed: usize,
    enabled: usize,
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
                McpAuthStatus::OAuth => summary.oauth += 1,
                McpAuthStatus::BearerToken => summary.bearer_token += 1,
                McpAuthStatus::NotLoggedIn => summary.not_logged_in += 1,
                McpAuthStatus::Unsupported => summary.unsupported_auth += 1,
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

    pub(super) fn has_details(&self) -> bool {
        self.loaded && self.error.is_none()
    }

    pub(super) fn lines(&self, width: usize) -> Vec<Line<'static>> {
        if let Some(error) = &self.error {
            return vec![Line::from(
                dashboard_value(error, width, /*prefix_width*/ 0).red(),
            )];
        }
        if !self.loaded {
            return vec![Line::from("not loaded".dim())];
        }
        vec![
            Line::from(format!("servers {}", format_usize(self.servers))),
            Line::from(format!("tools {}", format_usize(self.tools))),
            Line::from(format!(
                "auth oauth {} bearer {}",
                format_usize(self.oauth),
                format_usize(self.bearer_token)
            )),
            Line::from(format!(
                "needs login {} unsupported {}",
                format_usize(self.not_logged_in),
                format_usize(self.unsupported_auth)
            )),
        ]
    }
}

impl PluginInventorySummary {
    pub(super) fn from_response(response: &PluginListResponse) -> Self {
        let mut summary = Self {
            loaded: true,
            marketplaces: response.marketplaces.len(),
            errors: response.marketplace_load_errors.len(),
            ..Self::default()
        };
        for marketplace in &response.marketplaces {
            summary.plugins += marketplace.plugins.len();
            for plugin in &marketplace.plugins {
                if plugin.installed {
                    summary.installed += 1;
                }
                if plugin.enabled {
                    summary.enabled += 1;
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

    pub(super) fn has_details(&self) -> bool {
        self.loaded && self.error.is_none()
    }

    pub(super) fn lines(&self, width: usize) -> Vec<Line<'static>> {
        if let Some(error) = &self.error {
            return vec![Line::from(
                dashboard_value(error, width, /*prefix_width*/ 0).red(),
            )];
        }
        if !self.loaded {
            return vec![Line::from("not loaded".dim())];
        }
        vec![
            Line::from(format!("marketplaces {}", format_usize(self.marketplaces))),
            Line::from(format!("plugins {}", format_usize(self.plugins))),
            Line::from(format!(
                "installed {} enabled {}",
                format_usize(self.installed),
                format_usize(self.enabled)
            )),
            Line::from(format!("catalog errors {}", format_usize(self.errors))),
        ]
    }
}
