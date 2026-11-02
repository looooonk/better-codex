use super::WorkspaceResponse;
use super::workspace_request_id;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::AccountSessionsListParams;
use codex_app_server_protocol::AccountSessionsResponse;
use codex_app_server_protocol::AccountSessionsSwitchParams;
use codex_app_server_protocol::AccountSessionsSwitchResponse;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigRequirementsReadResponse;
use color_eyre::Result;
use color_eyre::eyre::bail;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app_shell) enum AccountCommand {
    List,
    Refresh,
    Switch { session_id: String, account_id: String },
}

impl AccountCommand {
    pub(in crate::app_shell) fn parse(args: &str) -> Result<Self> {
        match args.split_whitespace().collect::<Vec<_>>().as_slice() {
            [] | ["list"] => Ok(Self::List),
            ["refresh"] => Ok(Self::Refresh),
            ["switch", session_id, account_id] => Ok(Self::Switch { session_id: (*session_id).into(), account_id: (*account_id).into() }),
            _ => bail!("usage: /account [list|switch <session-id> <workspace-id>|refresh]; /login adds an account"),
        }
    }
}

pub(super) async fn execute(client: AppServerRequestHandle, command: AccountCommand) -> Result<WorkspaceResponse> {
    if command != AccountCommand::Refresh {
        let accounts: AccountSessionsResponse = client.request_typed(ClientRequest::AccountSessionsList {
            request_id: workspace_request_id("accounts-list"), params: AccountSessionsListParams { refresh_workspace_metadata: true },
        }).await?;
        match command {
            AccountCommand::List => {
                let mut lines = vec!["Saved accounts: /account switch <session-id> <workspace-id>; /login to add.".into()];
                for session in accounts.sessions {
                    let name = session.email.as_deref().or(session.display_name.as_deref()).unwrap_or("Account");
                    let selected = if session.is_active { " (active)" } else { "" };
                    lines.push(format!("{name} [{}]{selected}", session.session_id));
                    for workspace in session.workspaces {
                        let name = workspace.name.as_deref().unwrap_or("Workspace");
                        let selected = if session.selected_workspace_account_id.as_deref() == Some(&workspace.account_id) { " (selected)" } else { "" };
                        lines.push(format!("  {name} [{}]{selected}", workspace.account_id));
                    }
                }
                if lines.len() == 1 { lines.push("No saved ChatGPT accounts. Use /login.".into()); }
                return Ok(WorkspaceResponse::Notice(lines.join("\n")));
            }
            AccountCommand::Switch { session_id, account_id } => {
                if !accounts.sessions.iter().any(|session| session.session_id == session_id && session.workspaces.iter().any(|workspace| workspace.account_id == account_id)) {
                    bail!("unknown saved account or workspace; use /account list to refresh the IDs");
                }
                let response: AccountSessionsSwitchResponse = client.request_typed(ClientRequest::AccountSessionsSwitch {
                    request_id: workspace_request_id("accounts-switch"), params: AccountSessionsSwitchParams { session_id, account_id, allow_browser_login: true },
                }).await?;
                if let AccountSessionsSwitchResponse::BrowserLogin { login_id, auth_url } = response {
                    return Ok(WorkspaceResponse::AccountLogin { login_id, auth_url });
                }
            }
            AccountCommand::Refresh => unreachable!(),
        }
    }
    let models = async {
        let mut models = crate::app_server_session::load_models(client.clone()).await?;
        let requirements: ConfigRequirementsReadResponse = client.request_typed(ClientRequest::ConfigRequirementsRead {
            request_id: workspace_request_id("account-requirements"), params: None,
        }).await?;
        crate::service_tier_resolution::constrain_server_service_tiers(&mut models, &requirements);
        Ok::<_, color_eyre::Report>(models)
    }.await;
    Ok(WorkspaceResponse::AccountChanged {
        models: models.map_err(|error| error.to_string()),
    })
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
