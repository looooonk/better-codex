use super::WorkspaceResponse;
use super::workspace_request_id;
use crate::config_update::replace_config_value;
use crate::config_update::write_config_batch;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::CustomGatewaySettingsParams;
use codex_app_server_protocol::ModelProviderConfigWriteParams;
use codex_app_server_protocol::ModelProviderConfigWriteResponse;
use codex_app_server_protocol::ModelProviderListParams;
use codex_app_server_protocol::ModelProviderListResponse;
use codex_app_server_protocol::ProviderSettings;
use codex_app_server_protocol::ProviderSettingsPatch;
use codex_app_server_protocol::WriteStatus;
use color_eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app_shell) enum ProviderCommand {
    List,
    Use { id: String, model: Option<String> },
    Add(CustomGatewaySettingsParams),
    Configure { id: String, fields: Vec<(String, Option<String>)> },
}

impl ProviderCommand {
    pub(in crate::app_shell) fn parse(args: &str) -> Result<Self> {
        let words = shlex::split(args).ok_or_else(|| eyre!("unclosed quote in provider command"))?;
        let parts = words.iter().map(String::as_str).collect::<Vec<_>>();
        match parts.as_slice() {
            [] | ["list"] => Ok(Self::List),
            ["use", id] => Ok(Self::Use { id: (*id).into(), model: None }),
            ["use", id, model] => Ok(Self::Use { id: (*id).into(), model: Some((*model).into()) }),
            ["add", name, url, env @ ..] if env.len() <= 1 => Ok(Self::Add(CustomGatewaySettingsParams {
                provider_id: None, name: Some((*name).into()), base_url: Some(Some((*url).into())),
                env_key: env.first().map(|key| Some((*key).into())),
            })),
            ["configure", id, fields @ ..] if !fields.is_empty() => {
                let fields = fields.iter().map(|field| {
                    let (key, value) = field.split_once('=').ok_or_else(|| eyre!("provider fields use name=value; an empty value clears the field"))?;
                    Ok((key.to_string(), (!value.is_empty()).then(|| value.to_string())))
                }).collect::<Result<Vec<_>>>()?;
                Ok(Self::Configure { id: (*id).into(), fields })
            }
            _ => bail!("usage: /provider [list|use <id> [model]|add <name> <url> [key-environment-variable]|configure <id> field=value ...]"),
        }
    }
}

pub(super) async fn execute(client: AppServerRequestHandle, command: ProviderCommand) -> Result<WorkspaceResponse> {
    let catalog: ModelProviderListResponse = client.request_typed(ClientRequest::ModelProviderList {
        request_id: workspace_request_id("providers-list"), params: ModelProviderListParams {},
    }).await?;
    let settings = match command {
        ProviderCommand::List => {
            let mut lines = vec!["Providers: /provider use <id> [model] selects the default for new sessions.".to_string()];
            for provider in catalog.data {
                let selected = if provider.id == catalog.default_provider_id { " (default)" } else { "" };
                let fields = match provider.settings {
                    ProviderSettings::OpenAi => "",
                    ProviderSettings::CustomGateway { .. } => "name, base_url, env_key",
                    ProviderSettings::Bedrock { .. } => "profile, region",
                    ProviderSettings::Gcp { .. } => "project_id, location, credential_file",
                };
                lines.push(format!("{} [{}]{selected}", provider.name, provider.id));
                if provider.can_edit && !fields.is_empty() { lines.push(format!("  Configure fields: {fields}")); }
                if let Some(url) = provider.base_url { lines.push(format!("  {url}")); }
            }
            lines.push("/provider configure <id> field=value ...; /provider add <name> <url> [key-environment-variable]".into());
            return Ok(WorkspaceResponse::Notice(lines.join("\n")));
        }
        ProviderCommand::Use { id, model } => {
            if !catalog.can_change_default { bail!("the default provider is controlled by managed or command-line configuration"); }
            if !catalog.data.iter().any(|provider| provider.id == id) { bail!("unknown provider {id}; use /provider list"); }
            let response = write_config_batch(client, vec![
                replace_config_value("model_provider", Value::String(id.clone())),
                replace_config_value("model", model.clone().map_or(Value::Null, Value::String)),
            ]).await?;
            if response.status == WriteStatus::OkOverridden { bail!("provider saved, but a higher-priority configuration prevents applying it"); }
            return Ok(WorkspaceResponse::ProviderSelected { id, model });
        }
        ProviderCommand::Add(settings) => ProviderSettingsPatch::CustomGateway(settings),
        ProviderCommand::Configure { id, fields } => {
            let provider = catalog.data.iter().find(|provider| provider.id == id).ok_or_else(|| eyre!("unknown provider {id}"))?;
            if !provider.can_edit { bail!("provider {id} is not editable in the active user configuration"); }
            let kind = match provider.settings {
                ProviderSettings::OpenAi => bail!("use /login to configure OpenAI access"),
                ProviderSettings::CustomGateway { .. } => "customGateway",
                ProviderSettings::Bedrock { .. } => "bedrock",
                ProviderSettings::Gcp { .. } => "gcp",
            };
            let mut value = serde_json::json!({"type":kind,"providerId":id});
            for (key, field) in fields {
                let key = match key.as_str() {
                    "base_url" => "baseUrl", "env_key" => "envKey", "project_id" => "projectId",
                    "credential_file" => "credentialFile", "name" => "name", "profile" => "profile",
                    "region" => "region", "location" => "location",
                    _ => bail!("unknown provider field {key}"),
                };
                value[key] = field.map_or(Value::Null, Value::String);
            }
            serde_json::from_value(value)?
        }
    };
    let response: ModelProviderConfigWriteResponse = client.request_typed(ClientRequest::ModelProviderConfigWrite {
        request_id: workspace_request_id("provider-write"), params: ModelProviderConfigWriteParams { settings },
    }).await?;
    Ok(WorkspaceResponse::Notice(format!("Saved provider {} [{}]. Select it with /provider use {} [model], then /new.", response.provider.name, response.provider.id, response.provider.id)))
}

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;
