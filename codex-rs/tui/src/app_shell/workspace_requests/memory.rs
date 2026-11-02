use super::WorkspaceResponse;
use super::workspace_request_id;
use crate::config_update::replace_config_value;
use crate::config_update::write_config_batch;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::MemoryStatusParams;
use codex_app_server_protocol::MemoryStatusResponse;
use codex_app_server_protocol::ThreadMemoryMode;
use codex_app_server_protocol::ThreadMemoryModeSetParams;
use codex_app_server_protocol::ThreadMemoryModeSetResponse;
use codex_app_server_protocol::ThreadMemoryReadParams;
use codex_app_server_protocol::ThreadMemoryReadResponse;
use codex_app_server_protocol::WriteStatus;
use color_eyre::Result;
use color_eyre::eyre::bail;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app_shell) enum MemoryCommand {
    Status,
    Read,
    Session(ThreadMemoryMode),
    Use(bool),
    Generate(bool),
}

impl MemoryCommand {
    pub(in crate::app_shell) fn parse(args: &str) -> Result<Self> {
        match args.split_whitespace().collect::<Vec<_>>().as_slice() {
            [] | ["status"] => Ok(Self::Status),
            ["read"] => Ok(Self::Read),
            ["on"] => Ok(Self::Session(ThreadMemoryMode::Enabled)),
            ["off"] => Ok(Self::Session(ThreadMemoryMode::Disabled)),
            ["use", state @ ("on" | "off")] => Ok(Self::Use(*state == "on")),
            ["generate", state @ ("on" | "off")] => Ok(Self::Generate(*state == "on")),
            _ => bail!("usage: /memory [status|read|on|off|use on|off|generate on|off]"),
        }
    }
}

pub(super) async fn execute(client: AppServerRequestHandle, thread_id: String, cwd: String, command: MemoryCommand) -> Result<WorkspaceResponse> {
    if command == MemoryCommand::Read {
        let response: ThreadMemoryReadResponse = client.request_typed(ClientRequest::ThreadMemoryRead {
            request_id: workspace_request_id("memory-read"), params: ThreadMemoryReadParams { thread_id },
        }).await?;
        return Ok(WorkspaceResponse::Notice(response.summary.unwrap_or_else(|| "No saved memory summary for this session.".into())));
    }
    if let MemoryCommand::Session(mode) = command {
        let _: ThreadMemoryModeSetResponse = client.request_typed(ClientRequest::ThreadMemoryModeSet {
            request_id: workspace_request_id("memory-session"), params: ThreadMemoryModeSetParams { thread_id, mode },
        }).await?;
        return Ok(WorkspaceResponse::Notice(format!("Session memory generation: {mode:?}")));
    }
    let mut notice = String::new();
    if let MemoryCommand::Use(enabled) | MemoryCommand::Generate(enabled) = command {
        let key = if matches!(command, MemoryCommand::Use(_)) { "use_memories" } else { "generate_memories" };
        let response = write_config_batch(client.clone(), vec![replace_config_value(format!("memories.{key}"), enabled.into())]).await?;
        if response.status == WriteStatus::OkOverridden {
            notice.push_str("Saved, but a higher-priority configuration overrides the change. ");
        }
    }
    let config: ConfigReadResponse = client.request_typed(ClientRequest::ConfigRead {
        request_id: workspace_request_id("memory-config"), params: ConfigReadParams { include_layers: false, cwd: Some(cwd) },
    }).await?;
    let memories = config.config.additional.get("memories");
    let enabled = |key| memories.and_then(|value| value.get(key)).and_then(serde_json::Value::as_bool).unwrap_or(true);
    let use_memories = enabled("use_memories");
    let generate_memories = enabled("generate_memories");
    if matches!(command, MemoryCommand::Generate(_)) {
        let mode = if generate_memories { ThreadMemoryMode::Enabled } else { ThreadMemoryMode::Disabled };
        if let Err(error) = client.request_typed::<ThreadMemoryModeSetResponse>(ClientRequest::ThreadMemoryModeSet {
            request_id: workspace_request_id("memory-session"), params: ThreadMemoryModeSetParams { thread_id, mode },
        }).await {
            notice.push_str(&format!("Default saved; this session could not be updated: {error}. "));
        }
    }
    let status: MemoryStatusResponse = client.request_typed(ClientRequest::MemoryStatus {
        request_id: workspace_request_id("memory-status"), params: MemoryStatusParams { min_consolidated_threads: None },
    }).await?;
    let state = |value| if value { "on" } else { "off" };
    let ready = if status.v2_ready { "ready" } else { "building" };
    Ok(WorkspaceResponse::Notice(format!("{notice}Memory {ready}: {} consolidated sessions.\nUse memories: {}. Generate memories: {}.\n/memory use on|off; /memory generate on|off; /memory on|off for this session; /memory read for its summary.", status.v2_consolidated_threads, state(use_memories), state(generate_memories))))
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
