use super::WorkspaceResponse;
use super::workspace_request_id;
use crate::config_update::replace_config_value;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigBatchWriteParams;
use codex_app_server_protocol::ConfigLayerSource;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::ConfigWriteResponse;
use codex_app_server_protocol::WriteStatus;
use codex_config::types::QuickDial;
use color_eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use color_eyre::eyre::WrapErr;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app_shell) enum PresetCommand {
    List,
    Load(u8),
    Save(u8, QuickDial),
    Delete(u8),
}

impl PresetCommand {
    pub(in crate::app_shell) fn parse(args: &str) -> Result<Self> {
        let mut words = args.splitn(2, char::is_whitespace);
        let action = words.next().unwrap_or("");
        let rest = words.next().unwrap_or("").trim();
        match action {
            "" | "list" if rest.is_empty() => Ok(Self::List),
            "save" => {
                let mut words = rest.splitn(2, char::is_whitespace);
                let slot = parse_slot(words.next().unwrap_or(""))?;
                let dial = QuickDial {
                    input: words.next().unwrap_or("").trim().to_string(),
                    label: None,
                };
                dial.validate().map_err(|error| eyre!(error))?;
                Ok(Self::Save(slot, dial))
            }
            "delete" => Ok(Self::Delete(parse_slot(rest)?)),
            "load" | "run" => Ok(Self::Load(parse_slot(rest)?)),
            slot if rest.is_empty() => Ok(Self::Load(parse_slot(slot)?)),
            _ => bail!("usage: /presets [1-9|save <1-9> <prompt>|delete <1-9>]"),
        }
    }
}

fn parse_slot(value: &str) -> Result<u8> {
    value.parse::<u8>().ok().filter(|slot| (1..=9).contains(slot))
        .ok_or_else(|| eyre!("preset slot must be 1 through 9"))
}

pub(super) async fn execute(
    client: AppServerRequestHandle,
    cwd: String,
    command: PresetCommand,
) -> Result<WorkspaceResponse> {
    let response: ConfigReadResponse = client.request_typed(ClientRequest::ConfigRead {
        request_id: workspace_request_id("presets-read"),
        params: ConfigReadParams { include_layers: true, cwd: Some(cwd.clone()) },
    }).await?;
    let presets: BTreeMap<String, QuickDial> = response.config.additional.get("tui")
        .and_then(|tui| tui.get("presets"))
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?.unwrap_or_default();
    let (slot, dial) = match command {
        PresetCommand::List => {
            let mut lines = vec!["Presets: /presets <1-9> to load, /presets save <1-9> <prompt>, /presets delete <1-9>".to_string()];
            for slot in 1..=9 {
                if let Some(dial) = presets.get(&slot.to_string()) {
                    let preview = dial.label.as_deref().unwrap_or(&dial.input)
                        .chars().take(160).collect::<String>().replace('\n', " ");
                    lines.push(format!("{slot}. {preview}"));
                }
            }
            if lines.len() == 1 { lines.push("No saved presets.".into()); }
            return Ok(WorkspaceResponse::Notice(lines.join("\n")));
        }
        PresetCommand::Load(slot) => {
            let dial = presets.get(&slot.to_string()).ok_or_else(|| eyre!("preset {slot} is empty"))?;
            dial.validate().map_err(|error| eyre!(error))?;
            return Ok(WorkspaceResponse::Preset(dial.input.clone()));
        }
        PresetCommand::Save(slot, dial) => (slot, Some(dial)),
        PresetCommand::Delete(slot) => (slot, None),
    };
    let layers = response.layers.ok_or_else(|| eyre!("config/read omitted requested layers"))?;
    let target = layers.iter().filter(|layer| {
        layer.disabled_reason.is_none() && matches!(layer.name, ConfigLayerSource::User { .. })
    }).max_by_key(|layer| layer.name.precedence());
    let precedence = target.map_or(20, |layer| layer.name.precedence());
    let prefix = format!("tui.presets.{slot}");
    if response.origins.iter().any(|(key, origin)| {
        (key == "tui" || key == "tui.presets" || key == &prefix || key.starts_with(&format!("{prefix}.")))
            && origin.name.precedence() > precedence
    }) {
        bail!("preset {slot} is controlled by project, command-line, or managed configuration");
    }
    if dial.is_none() && target.is_none_or(|layer| layer.config["tui"]["presets"].get(slot.to_string()).is_none()) {
        if !presets.contains_key(&slot.to_string()) {
            return Ok(WorkspaceResponse::Notice(format!("Preset {slot} is already empty.")));
        }
        let origin = response.origins.get(&format!("{prefix}.input"))
            .or_else(|| response.origins.get(&prefix));
        let source = match origin.map(|origin| &origin.name) {
            Some(ConfigLayerSource::User { file, .. }
                | ConfigLayerSource::System { file }
                | ConfigLayerSource::PackagedDefaults { file }
                | ConfigLayerSource::LegacyManagedConfigTomlFromFile { file }) => file.display().to_string(),
            _ => "the lower-priority source configuration".to_string(),
        };
        bail!("preset {slot} is inherited from {source}; remove it there to delete it");
    }
    let value = dial.as_ref().map_or(Value::Null, |dial| {
        let mut value = serde_json::json!({"input": dial.input});
        if let Some(label) = &dial.label { value["label"] = Value::String(label.clone()); }
        value
    });
    let result: ConfigWriteResponse = client.request_typed(ClientRequest::ConfigBatchWrite {
        request_id: workspace_request_id("presets-write"),
        params: ConfigBatchWriteParams {
            edits: vec![replace_config_value(format!("tui.presets.\"{slot}\""), value)],
            file_path: target.and_then(|layer| match &layer.name {
                ConfigLayerSource::User { file, .. } => Some(file.display().to_string()),
                _ => None,
            }),
            expected_version: target.map(|layer| layer.version.clone()),
            reload_user_config: true,
        },
    }).await?;
    if dial.is_none() {
        let effective: ConfigReadResponse = client.request_typed(ClientRequest::ConfigRead {
            request_id: workspace_request_id("presets-delete-readback"),
            params: ConfigReadParams { include_layers: false, cwd: Some(cwd) },
        }).await.wrap_err("preset override was removed, but its effective configuration could not be checked")?;
        if effective.config.additional.get("tui")
            .and_then(|tui| tui.get("presets"))
            .and_then(|presets| presets.get(slot.to_string()))
            .is_some()
        {
            return Ok(WorkspaceResponse::Notice(format!("Removed preset {slot} from the active configuration. An inherited preset remains available; use /presets {slot} to load it.")));
        }
    }
    let action = if dial.is_some() { "Saved" } else { "Deleted" };
    let suffix = if result.status == WriteStatus::OkOverridden {
        " A higher-priority setting still overrides it."
    } else { "" };
    Ok(WorkspaceResponse::Notice(format!("{action} preset {slot}.{suffix}")))
}

#[cfg(test)]
#[path = "presets_tests.rs"]
mod tests;
