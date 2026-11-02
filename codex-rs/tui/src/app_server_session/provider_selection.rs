use crate::legacy_core::config::Config;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::RequestId;
use codex_config::ConfigLayerSource;

pub(super) fn explicit_provider(config: &Config) -> Option<String> {
    config
        .config_layer_stack
        .layers_high_to_low()
        .find(|layer| layer.config.get("model_provider").is_some())
        .filter(|layer| {
            matches!(
                layer.name,
                ConfigLayerSource::SessionFlags
                    | ConfigLayerSource::User {
                        profile: Some(_),
                        ..
                    }
            )
        })
        .map(|_| config.model_provider_id.clone())
}

impl super::AppServerSession {
    pub(crate) async fn history_model_provider(
        &self,
        config: &Config,
    ) -> color_eyre::Result<Option<String>> {
        if let Some(provider) = self
            .resume_settings(config)
            .provider
            .or_else(|| explicit_provider(config))
        {
            return Ok(Some(provider));
        }
        let Some(cwd) = super::thread_cwd_from_config(
            config,
            self.thread_params_mode(),
            self.remote_cwd_override(),
        ) else {
            return Ok(None);
        };
        let response: ConfigReadResponse = match self
            .request_handle()
            .request_typed(ClientRequest::ConfigRead {
                request_id: RequestId::String(format!("history-provider-{}", uuid::Uuid::new_v4())),
                params: ConfigReadParams {
                    include_layers: false,
                    cwd: Some(cwd),
                },
            })
            .await
        {
            Ok(response) => response,
            Err(TypedRequestError::Server { source, .. }) if source.code == -32601 => {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Some(
            response
                .config
                .model_provider
                .unwrap_or_else(|| "openai".to_string()),
        ))
    }
}

#[cfg(test)]
#[path = "provider_selection_tests.rs"]
mod tests;
