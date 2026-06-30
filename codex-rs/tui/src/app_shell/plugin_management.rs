use super::ShellState;
use super::backend::AppShellBackend;
use codex_app_server_protocol::PluginAvailability;
use codex_app_server_protocol::PluginInstallParams;
use codex_app_server_protocol::PluginInstallPolicy;
use codex_app_server_protocol::PluginInstallResponse;
use codex_app_server_protocol::PluginListResponse;
use codex_app_server_protocol::PluginSource;
use codex_app_server_protocol::PluginSummary;
use codex_app_server_protocol::PluginUninstallParams;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::style::Stylize;
use ratatui::text::Line;

const VISIBLE_PLUGIN_ROWS: usize = 8;

impl ShellState {
    pub(in crate::app_shell) fn open_plugin_management(&mut self) {
        let Some(response) = self.plugin_catalog.clone() else {
            self.settings
                .set_error("refresh plugins before opening catalog");
            return;
        };
        let state = PluginManagementState::from_response(response);
        if state.is_empty() {
            self.settings.set_info("no plugins found");
            return;
        }
        self.pending_plugin_management = Some(state);
        self.push_status("plugin manager opened");
    }

    pub(in crate::app_shell) async fn handle_plugin_management_key<S>(
        &mut self,
        key: KeyEvent,
        app_server: &mut S,
    ) -> Result<bool>
    where
        S: AppShellBackend,
    {
        match key.code {
            KeyCode::Esc => self.pending_plugin_management = None,
            KeyCode::Up => {
                if let Some(state) = &mut self.pending_plugin_management {
                    state.move_up();
                }
            }
            KeyCode::Down => {
                if let Some(state) = &mut self.pending_plugin_management {
                    state.move_down();
                }
            }
            KeyCode::Char('r') => {
                self.refresh_plugin_inventory(app_server).await;
                self.rebuild_plugin_management_from_catalog();
            }
            KeyCode::Enter | KeyCode::Char('i') => {
                self.install_or_update_selected_plugin(app_server).await?;
            }
            KeyCode::Char('e') => {
                self.toggle_selected_plugin_enabled(app_server).await?;
            }
            KeyCode::Char('u') => {
                self.uninstall_selected_plugin(app_server).await?;
            }
            _ => {}
        }
        Ok(true)
    }

    fn rebuild_plugin_management_from_catalog(&mut self) {
        let previous = self
            .pending_plugin_management
            .as_ref()
            .map(PluginManagementState::selected_plugin_id);
        if let Some(response) = self.plugin_catalog.clone() {
            let mut state = PluginManagementState::from_response(response);
            if let Some(plugin_id) = previous {
                state.select_plugin_id(&plugin_id);
            }
            self.pending_plugin_management = (!state.is_empty()).then_some(state);
        } else {
            self.pending_plugin_management = None;
        }
    }

    async fn install_or_update_selected_plugin<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(entry) = self
            .pending_plugin_management
            .as_ref()
            .and_then(PluginManagementState::selected)
        else {
            return Ok(());
        };
        let Some(params) = entry.install_params() else {
            self.push_status("selected plugin cannot be installed or updated");
            return Ok(());
        };
        let plugin_name = entry.display_name();
        let response = app_server.plugin_install(params).await?;
        self.report_plugin_install_result(&plugin_name, entry.plugin.installed, response);
        self.refresh_plugin_inventory(app_server).await;
        self.rebuild_plugin_management_from_catalog();
        Ok(())
    }

    async fn toggle_selected_plugin_enabled<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(entry) = self
            .pending_plugin_management
            .as_ref()
            .and_then(PluginManagementState::selected)
        else {
            return Ok(());
        };
        if !entry.can_toggle() {
            self.push_status("selected plugin cannot be enabled or disabled");
            return Ok(());
        }
        let display_name = entry.display_name();
        let enabled = !entry.plugin.enabled;
        let plugin_id = entry.plugin.id.clone();
        app_server.plugin_set_enabled(plugin_id, enabled).await?;
        self.push_status(format!(
            "{display_name} {}",
            if enabled { "enabled" } else { "disabled" }
        ));
        self.refresh_plugin_inventory(app_server).await;
        self.rebuild_plugin_management_from_catalog();
        Ok(())
    }

    async fn uninstall_selected_plugin<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(entry) = self
            .pending_plugin_management
            .as_ref()
            .and_then(PluginManagementState::selected)
        else {
            return Ok(());
        };
        let Some(plugin_id) = entry.uninstall_plugin_id() else {
            self.push_status("selected plugin cannot be uninstalled");
            return Ok(());
        };
        let display_name = entry.display_name();
        app_server
            .plugin_uninstall(PluginUninstallParams { plugin_id })
            .await?;
        self.push_status(format!("{display_name} uninstalled"));
        self.refresh_plugin_inventory(app_server).await;
        self.rebuild_plugin_management_from_catalog();
        Ok(())
    }

    fn report_plugin_install_result(
        &mut self,
        plugin_name: &str,
        was_installed: bool,
        response: PluginInstallResponse,
    ) {
        let action = if was_installed {
            "updated"
        } else {
            "installed"
        };
        if response.apps_needing_auth.is_empty() {
            self.push_status(format!("{plugin_name} {action}"));
        } else {
            let app_names = response
                .apps_needing_auth
                .iter()
                .map(|app| app.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            self.push_status(format!(
                "{plugin_name} {action}; auth required for {app_names}"
            ));
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PluginManagementState {
    entries: Vec<PluginEntry>,
    selected: usize,
}

impl PluginManagementState {
    pub(super) fn from_response(response: PluginListResponse) -> Self {
        let mut entries = response
            .marketplaces
            .into_iter()
            .flat_map(|marketplace| {
                let marketplace_name = marketplace.name.clone();
                let marketplace_path = marketplace.path.clone();
                marketplace
                    .plugins
                    .into_iter()
                    .map(move |plugin| PluginEntry {
                        marketplace_name: marketplace_name.clone(),
                        marketplace_path: marketplace_path.clone(),
                        plugin,
                    })
            })
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| {
            (!a.plugin.installed, a.display_name().to_lowercase())
                .cmp(&(!b.plugin.installed, b.display_name().to_lowercase()))
        });
        Self {
            entries,
            selected: 0,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn selected(&self) -> Option<&PluginEntry> {
        self.entries.get(self.selected)
    }

    fn selected_plugin_id(&self) -> String {
        self.selected()
            .map_or_else(String::new, |entry| entry.plugin.id.clone())
    }

    fn select_plugin_id(&mut self, plugin_id: &str) {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.plugin.id == plugin_id)
        {
            self.selected = index;
        }
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn move_down(&mut self) {
        self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1));
    }

    pub(super) fn lines(&self) -> Vec<Line<'static>> {
        if self.entries.is_empty() {
            return vec!["No plugins found.".into()];
        }

        let mut lines = vec![
            vec![
                format!("{}/{}", self.selected + 1, self.entries.len())
                    .cyan()
                    .bold(),
                " plugin catalog".into(),
                "  Enter action  i install/update  e enable/disable  u uninstall  r refresh".dim(),
            ]
            .into(),
        ];
        let start = visible_start(self.selected, self.entries.len());
        for (index, entry) in self
            .entries
            .iter()
            .enumerate()
            .skip(start)
            .take(VISIBLE_PLUGIN_ROWS)
        {
            let marker = if index == self.selected {
                ">".cyan().bold()
            } else {
                " ".into()
            };
            lines.push(
                vec![
                    marker,
                    " ".into(),
                    format!("{:<10}", entry.status_label()).into(),
                    " ".dim(),
                    entry.display_name().bold(),
                    " ".into(),
                    format!("({})", entry.marketplace_name).dim(),
                ]
                .into(),
            );
        }
        if let Some(entry) = self.selected() {
            lines.push("".into());
            lines.push(vec!["Action: ".dim(), entry.action_hint().into()].into());
        }
        lines
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PluginEntry {
    marketplace_name: String,
    marketplace_path: Option<AbsolutePathBuf>,
    plugin: PluginSummary,
}

impl PluginEntry {
    fn display_name(&self) -> String {
        self.plugin
            .interface
            .as_ref()
            .and_then(|interface| interface.display_name.clone())
            .unwrap_or_else(|| self.plugin.name.clone())
    }

    fn status_label(&self) -> &'static str {
        if self.plugin.availability == PluginAvailability::DisabledByAdmin {
            return "admin";
        }
        if self.plugin.installed && self.plugin.enabled {
            return "enabled";
        }
        if self.plugin.installed {
            return "disabled";
        }
        match self.plugin.install_policy {
            PluginInstallPolicy::Available => "available",
            PluginInstallPolicy::NotAvailable => "blocked",
            PluginInstallPolicy::InstalledByDefault => "admin",
        }
    }

    fn action_hint(&self) -> &'static str {
        if self.install_params().is_some() {
            if self.plugin.installed {
                "press i or Enter to update from the marketplace"
            } else {
                "press i or Enter to install"
            }
        } else if self.can_toggle() {
            if self.plugin.enabled {
                "press e to disable"
            } else {
                "press e to enable"
            }
        } else if self.uninstall_plugin_id().is_some() {
            "press u to uninstall"
        } else {
            "no action available"
        }
    }

    fn can_toggle(&self) -> bool {
        self.plugin.installed
            && self.plugin.install_policy != PluginInstallPolicy::InstalledByDefault
            && self.plugin.availability != PluginAvailability::DisabledByAdmin
    }

    fn install_params(&self) -> Option<PluginInstallParams> {
        if self.plugin.availability == PluginAvailability::DisabledByAdmin
            || self.plugin.install_policy != PluginInstallPolicy::Available
        {
            return None;
        }
        if let Some(marketplace_path) = self.marketplace_path.clone() {
            return Some(PluginInstallParams {
                marketplace_path: Some(marketplace_path),
                remote_marketplace_name: None,
                plugin_name: plugin_request_name(&self.plugin),
            });
        }
        plugin_remote_identity(&self.plugin).map(|_| PluginInstallParams {
            marketplace_path: None,
            remote_marketplace_name: Some(self.marketplace_name.clone()),
            plugin_name: plugin_request_name(&self.plugin),
        })
    }

    fn uninstall_plugin_id(&self) -> Option<String> {
        if !self.plugin.installed
            || self.plugin.install_policy == PluginInstallPolicy::InstalledByDefault
        {
            return None;
        }
        if matches!(&self.plugin.source, PluginSource::Remote) {
            return plugin_remote_identity(&self.plugin).map(str::to_string);
        }
        Some(self.plugin.id.clone())
    }
}

fn plugin_request_name(plugin: &PluginSummary) -> String {
    if matches!(&plugin.source, PluginSource::Remote)
        && let Some(remote_plugin_id) = plugin_remote_identity(plugin)
    {
        return remote_plugin_id.to_string();
    }
    plugin.name.clone()
}

fn plugin_remote_identity(plugin: &PluginSummary) -> Option<&str> {
    plugin
        .share_context
        .as_ref()
        .map(|context| context.remote_plugin_id.as_str())
        .or(plugin.remote_plugin_id.as_deref())
}

fn visible_start(selected: usize, total: usize) -> usize {
    if total <= VISIBLE_PLUGIN_ROWS {
        0
    } else {
        selected
            .saturating_sub(VISIBLE_PLUGIN_ROWS / 2)
            .min(total.saturating_sub(VISIBLE_PLUGIN_ROWS))
    }
}
