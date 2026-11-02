use super::WorkspaceResponse;
use super::workspace_request_id;
use crate::config_update::app_scoped_key_path;
use crate::config_update::replace_config_value;
use crate::config_update::write_config_batch;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::AppsListParams;
use codex_app_server_protocol::AppsListResponse;
use codex_app_server_protocol::AppsReadParams;
use codex_app_server_protocol::AppsReadResponse;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::WriteStatus;
use color_eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app_shell) enum AppsCommand {
    List(String),
    Access(String),
    Tools(String),
    Configure {
        app_id: String,
        key: String,
        value: Value,
    },
}

impl AppsCommand {
    pub(in crate::app_shell) fn parse(args: &str) -> Result<Self> {
        let words = shlex::split(args).ok_or_else(|| eyre!("unclosed quote in app command"))?;
        let parts = words.iter().map(String::as_str).collect::<Vec<_>>();
        let configure = |id: &str, key: String, value: Value| Self::Configure {
            app_id: id.to_string(),
            key,
            value,
        };
        match parts.as_slice() {
            [] | ["list"] => Ok(Self::List(String::new())),
            ["list", query @ ..] => Ok(Self::List(query.join(" "))),
            ["access", id] => Ok(Self::Access((*id).into())),
            ["tools", id] => Ok(Self::Tools((*id).into())),
            [action @ ("enable" | "disable"), id] => Ok(configure(
                id,
                "enabled".into(),
                Value::Bool(*action == "enable"),
            )),
            [
                "approval",
                id,
                mode @ ("auto" | "prompt" | "writes" | "approve"),
            ] => Ok(configure(
                id,
                "default_tools_approval_mode".into(),
                Value::String((*mode).into()),
            )),
            ["tool", id, tool, action @ ("enable" | "disable")] => Ok(configure(
                id,
                format!("tools.{}.enabled", serde_json::to_string(tool)?),
                Value::Bool(*action == "enable"),
            )),
            [
                "tool",
                id,
                tool,
                "approval",
                mode @ ("auto" | "prompt" | "writes" | "approve"),
            ] => Ok(configure(
                id,
                format!("tools.{}.approval_mode", serde_json::to_string(tool)?),
                Value::String((*mode).into()),
            )),
            _ => bail!(
                "usage: /apps [list [search]|access <id>|tools <id>|enable <id>|disable <id>|approval <id> auto|prompt|writes|approve|tool <id> <tool> enable|disable|approval <mode>]"
            ),
        }
    }
}

pub(super) async fn execute(
    client: AppServerRequestHandle,
    thread_id: String,
    command: AppsCommand,
) -> Result<WorkspaceResponse> {
    if let AppsCommand::List(query) = command {
        let mut cursor = None;
        let mut seen = HashSet::new();
        let mut lines = vec![
            "Apps: /apps access <id> to connect; /apps tools <id> for permissions.".to_string(),
        ];
        let query = query.to_lowercase();
        for _ in 0..100 {
            let response: AppsListResponse = client
                .request_typed(ClientRequest::AppsList {
                    request_id: workspace_request_id("apps-list"),
                    params: AppsListParams {
                        cursor,
                        limit: Some(100),
                        thread_id: Some(thread_id.clone()),
                        force_refetch: false,
                    },
                })
                .await?;
            if response.data.len() > 100 {
                bail!("app catalog exceeded its requested page size");
            }
            for app in response.data {
                if !query.is_empty()
                    && !format!("{} {}", app.id, app.name)
                        .to_lowercase()
                        .contains(&query)
                {
                    continue;
                }
                let access = if app.is_accessible {
                    "connected"
                } else {
                    "connect to use"
                };
                let enabled = if app.is_enabled {
                    "enabled"
                } else {
                    "disabled"
                };
                lines.push(format!("{} [{}] - {access}, {enabled}", app.name, app.id));
            }
            cursor = response.next_cursor;
            let Some(next) = &cursor else {
                if lines.len() == 1 {
                    lines.push("No matching apps.".into());
                }
                return Ok(WorkspaceResponse::Notice(lines.join("\n")));
            };
            if !seen.insert(next.clone()) {
                bail!("app catalog returned a repeated cursor");
            }
        }
        bail!("app catalog exceeds the 10,000-app limit");
    }
    let app_id = match &command {
        AppsCommand::Access(id) | AppsCommand::Tools(id) => id,
        AppsCommand::Configure { app_id, .. } => app_id,
        AppsCommand::List(_) => unreachable!(),
    };
    let response: AppsReadResponse = client
        .request_typed(ClientRequest::AppsRead {
            request_id: workspace_request_id("apps-read"),
            params: AppsReadParams {
                app_ids: vec![app_id.clone()],
                thread_id: Some(thread_id),
                include_tools: true,
            },
        })
        .await?;
    let app = response
        .apps
        .into_iter()
        .find(|app| &app.id == app_id)
        .ok_or_else(|| eyre!("unknown app {app_id}; use /apps list to find its ID"))?;
    let message = match command {
        AppsCommand::Access(_) => match app.install_url {
            Some(url) => format!("Connect or manage {} in your browser:\n{url}", app.name),
            None => format!(
                "{} has no connection link. Manage its access in ChatGPT Settings > Apps.",
                app.name
            ),
        },
        AppsCommand::Tools(_) => {
            let mut lines = vec![format!(
                "{} tools (use /apps tool {} <tool> enable|disable|approval <mode>):",
                app.name, app.id
            )];
            for tool in app.tool_summaries.unwrap_or_default() {
                let mode = if tool.is_enabled {
                    "enabled"
                } else {
                    "disabled"
                };
                let access = if tool.is_read_only { "read" } else { "write" };
                let reason = tool
                    .disabled_reason
                    .map(|reason| format!(" - {reason}"))
                    .unwrap_or_default();
                lines.push(format!("{} - {mode}, {access}{reason}", tool.name));
            }
            lines.join("\n")
        }
        AppsCommand::Configure { app_id, key, value } => {
            let response = write_config_batch(
                client,
                vec![replace_config_value(
                    app_scoped_key_path(&app_id, &key),
                    value,
                )],
            )
            .await?;
            if response.status == WriteStatus::OkOverridden {
                format!(
                    "Saved {} permissions; a higher-priority configuration overrides the change.",
                    app.name
                )
            } else {
                format!(
                    "Updated {} permissions. Applies to subsequent turns.",
                    app.name
                )
            }
        }
        AppsCommand::List(_) => unreachable!(),
    };
    Ok(WorkspaceResponse::Notice(message))
}

#[cfg(test)]
#[path = "apps_tests.rs"]
mod tests;
