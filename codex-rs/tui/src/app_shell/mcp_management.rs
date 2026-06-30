use super::ShellState;
use super::backend::AppShellBackend;
use codex_app_server_protocol::ListMcpServerStatusResponse;
use codex_app_server_protocol::McpAuthStatus;
use codex_app_server_protocol::McpServerOauthLoginParams;
use codex_app_server_protocol::McpServerStatus;
use codex_app_server_protocol::MergeStrategy;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::style::Stylize;
use ratatui::text::Line;

const VISIBLE_MCP_ROWS: usize = 8;

impl ShellState {
    pub(in crate::app_shell) fn open_mcp_management(&mut self) {
        let Some(response) = self.mcp_catalog.clone() else {
            self.settings
                .set_error("refresh mcp before opening manager");
            return;
        };
        let state = McpManagementState::from_response(response);
        if state.is_empty() {
            self.settings.set_info("no mcp servers found");
            return;
        }
        self.pending_mcp_management = Some(state);
        self.push_status("mcp manager opened");
    }

    pub(in crate::app_shell) async fn handle_mcp_management_key<S>(
        &mut self,
        key: KeyEvent,
        app_server: &mut S,
    ) -> Result<bool>
    where
        S: AppShellBackend,
    {
        if self
            .pending_mcp_management
            .as_ref()
            .is_some_and(McpManagementState::editing)
        {
            return self.handle_mcp_edit_key(key, app_server).await;
        }
        match key.code {
            KeyCode::Esc => self.pending_mcp_management = None,
            KeyCode::Up => self.with_mcp_state(McpManagementState::move_up),
            KeyCode::Down => self.with_mcp_state(McpManagementState::move_down),
            KeyCode::Char('r') => {
                self.reload_mcp_servers(app_server).await?;
            }
            KeyCode::Enter | KeyCode::Char('l') => {
                self.login_selected_mcp_server(app_server).await?;
            }
            KeyCode::Char('d') => {
                self.disable_selected_mcp_server(app_server).await?;
            }
            KeyCode::Char('x') | KeyCode::Char('u') => {
                self.remove_selected_mcp_server(app_server).await?;
            }
            KeyCode::Char('a') => self.with_mcp_state(McpManagementState::start_add),
            KeyCode::Char('e') => self.with_mcp_state(McpManagementState::start_edit),
            _ => {}
        }
        Ok(true)
    }

    async fn handle_mcp_edit_key<S>(&mut self, key: KeyEvent, app_server: &mut S) -> Result<bool>
    where
        S: AppShellBackend,
    {
        match key.code {
            KeyCode::Esc => self.with_mcp_state(McpManagementState::cancel_edit),
            KeyCode::Enter => self.apply_mcp_edit(app_server).await?,
            KeyCode::Backspace => self.with_mcp_state(McpManagementState::pop_draft),
            KeyCode::Tab | KeyCode::BackTab => {
                self.with_mcp_state(|state| state.push_draft("    "))
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.with_mcp_state(|state| state.push_draft(&ch.to_string()));
            }
            _ => {}
        }
        Ok(true)
    }

    fn with_mcp_state(&mut self, update: impl FnOnce(&mut McpManagementState)) {
        if let Some(state) = &mut self.pending_mcp_management {
            update(state);
        }
    }

    fn rebuild_mcp_management_from_catalog(&mut self) {
        let previous = self
            .pending_mcp_management
            .as_ref()
            .map(McpManagementState::selected_server_name);
        if let Some(response) = self.mcp_catalog.clone() {
            let mut state = McpManagementState::from_response(response);
            if let Some(name) = previous {
                state.select_server_name(&name);
            }
            self.pending_mcp_management = (!state.is_empty()).then_some(state);
        } else {
            self.pending_mcp_management = None;
        }
    }

    async fn reload_mcp_servers<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        app_server.mcp_server_refresh().await?;
        self.refresh_mcp_inventory(app_server).await;
        self.rebuild_mcp_management_from_catalog();
        self.push_status("mcp servers reloaded");
        Ok(())
    }

    async fn login_selected_mcp_server<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(server) = self.selected_mcp_server().cloned() else {
            return Ok(());
        };
        if !matches!(
            server.auth_status,
            McpAuthStatus::NotLoggedIn | McpAuthStatus::OAuth
        ) {
            self.push_status("selected mcp server does not support oauth login");
            return Ok(());
        }
        let response = app_server
            .mcp_server_oauth_login(McpServerOauthLoginParams {
                name: server.name.clone(),
                thread_id: Some(self.thread_id.to_string()),
                scopes: None,
                timeout_secs: None,
            })
            .await?;
        self.push_status(format!(
            "{} login started: {}",
            server.name, response.authorization_url
        ));
        Ok(())
    }

    async fn disable_selected_mcp_server<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(server) = self.selected_mcp_server().cloned() else {
            return Ok(());
        };
        app_server
            .mcp_server_write_config(
                server.name.clone(),
                serde_json::json!({ "enabled": false }),
                MergeStrategy::Upsert,
            )
            .await?;
        self.reload_mcp_servers(app_server).await?;
        self.push_status(format!("{} disabled", server.name));
        Ok(())
    }

    async fn remove_selected_mcp_server<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(server) = self.selected_mcp_server().cloned() else {
            return Ok(());
        };
        app_server
            .mcp_server_write_config(
                server.name.clone(),
                serde_json::Value::Null,
                MergeStrategy::Replace,
            )
            .await?;
        self.reload_mcp_servers(app_server).await?;
        self.push_status(format!("{} removed", server.name));
        Ok(())
    }

    async fn apply_mcp_edit<S>(&mut self, app_server: &mut S) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some((server_name, value)) = self
            .pending_mcp_management
            .as_ref()
            .and_then(McpManagementState::parsed_edit)
        else {
            self.push_error("mcp edit must be: name {\"url\":\"https://...\"}");
            return Ok(());
        };
        app_server
            .mcp_server_write_config(server_name.clone(), value, MergeStrategy::Replace)
            .await?;
        self.reload_mcp_servers(app_server).await?;
        if let Some(state) = &mut self.pending_mcp_management {
            state.select_server_name(&server_name);
        }
        self.push_status(format!("{server_name} saved"));
        Ok(())
    }

    fn selected_mcp_server(&self) -> Option<&McpServerStatus> {
        self.pending_mcp_management
            .as_ref()
            .and_then(McpManagementState::selected)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct McpManagementState {
    servers: Vec<McpServerStatus>,
    selected: usize,
    edit: Option<McpEditState>,
}

impl McpManagementState {
    pub(super) fn from_response(response: ListMcpServerStatusResponse) -> Self {
        let mut servers = response.data;
        servers.sort_by_key(|server| server.name.to_lowercase());
        Self {
            servers,
            selected: 0,
            edit: None,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    pub(super) fn lines(&self) -> Vec<Line<'static>> {
        if let Some(edit) = &self.edit {
            return edit.lines();
        }
        let mut lines = vec![
            vec![
                format!("{}/{}", self.selected + 1, self.servers.len())
                    .cyan()
                    .bold(),
                " mcp servers".into(),
                "  Enter/l login  d disable  x remove  a add  e edit  r reload".dim(),
            ]
            .into(),
        ];
        let start = visible_start(self.selected, self.servers.len());
        for (index, server) in self
            .servers
            .iter()
            .enumerate()
            .skip(start)
            .take(VISIBLE_MCP_ROWS)
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
                    format!("{:<12}", auth_label(server.auth_status)).into(),
                    " ".dim(),
                    server.name.clone().bold(),
                    " ".into(),
                    format!("{} tools", server.tools.len()).dim(),
                ]
                .into(),
            );
        }
        if let Some(server) = self.selected() {
            lines.push("".into());
            lines.push(vec!["Action: ".dim(), action_hint(server).into()].into());
        }
        lines
    }

    fn selected(&self) -> Option<&McpServerStatus> {
        self.servers.get(self.selected)
    }

    fn selected_server_name(&self) -> String {
        self.selected()
            .map_or_else(String::new, |server| server.name.clone())
    }

    fn select_server_name(&mut self, name: &str) {
        if let Some(index) = self.servers.iter().position(|server| server.name == name) {
            self.selected = index;
        }
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn move_down(&mut self) {
        self.selected = (self.selected + 1).min(self.servers.len().saturating_sub(1));
    }

    fn editing(&self) -> bool {
        self.edit.is_some()
    }

    fn start_add(&mut self) {
        self.edit = Some(McpEditState {
            title: "Add MCP server".to_string(),
            draft: String::new(),
        });
    }

    fn start_edit(&mut self) {
        if let Some(server) = self.selected() {
            self.edit = Some(McpEditState {
                title: format!("Edit {}", server.name),
                draft: format!("{} {{}}", server.name),
            });
        }
    }

    fn cancel_edit(&mut self) {
        self.edit = None;
    }

    fn push_draft(&mut self, text: &str) {
        if let Some(edit) = &mut self.edit {
            edit.draft.push_str(text);
        }
    }

    fn pop_draft(&mut self) {
        if let Some(edit) = &mut self.edit {
            edit.draft.pop();
        }
    }

    fn parsed_edit(&self) -> Option<(String, serde_json::Value)> {
        let draft = self.edit.as_ref()?.draft.trim();
        let split = draft.find(char::is_whitespace)?;
        let name = draft[..split].trim();
        let value = draft[split..].trim();
        if name.is_empty() {
            return None;
        }
        serde_json::from_str::<serde_json::Value>(value)
            .ok()
            .filter(serde_json::Value::is_object)
            .map(|value| (name.to_string(), value))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct McpEditState {
    title: String,
    draft: String,
}

impl McpEditState {
    fn lines(&self) -> Vec<Line<'static>> {
        vec![
            self.title.clone().bold().into(),
            "Format: name {\"url\":\"https://...\"} or name {\"command\":\"cmd\",\"args\":[]}"
                .dim()
                .into(),
            "".into(),
            self.draft.clone().into(),
            "".into(),
            "Enter save  Esc cancel".dim().into(),
        ]
    }
}

fn auth_label(status: McpAuthStatus) -> &'static str {
    match status {
        McpAuthStatus::Unsupported => "unsupported",
        McpAuthStatus::NotLoggedIn => "login",
        McpAuthStatus::BearerToken => "bearer",
        McpAuthStatus::OAuth => "oauth",
    }
}

fn action_hint(server: &McpServerStatus) -> &'static str {
    match server.auth_status {
        McpAuthStatus::NotLoggedIn => "press Enter or l to start oauth login",
        McpAuthStatus::OAuth => "press l to refresh oauth login",
        McpAuthStatus::BearerToken | McpAuthStatus::Unsupported => {
            "press d to disable, x to remove, e to replace config"
        }
    }
}

fn visible_start(selected: usize, total: usize) -> usize {
    if total <= VISIBLE_MCP_ROWS {
        0
    } else {
        selected
            .saturating_sub(VISIBLE_MCP_ROWS / 2)
            .min(total.saturating_sub(VISIBLE_MCP_ROWS))
    }
}
