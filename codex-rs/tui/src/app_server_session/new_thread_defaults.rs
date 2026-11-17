use crate::legacy_core::config::Config;
use crate::legacy_core::config::ConfigOverrides;
use codex_app_server_protocol::NewThreadModelDefaults;
use codex_config::ConfigLayerSource;
use codex_protocol::config_types::ServiceTier;

fn has_launch_setting(config: &Config, key: &str) -> bool {
    config
        .config_layer_stack
        .layers_high_to_low()
        .find(|layer| layer.config.get(key).is_some())
        .is_some_and(|layer| {
            matches!(
                layer.name,
                ConfigLayerSource::SessionFlags
                    | ConfigLayerSource::User {
                        profile: Some(_),
                        ..
                    }
            )
        })
}

/// A fresh workspace uses the connected server's settings unless the launch explicitly overrides them.
pub(super) async fn apply_server_startup_settings(
    session: &super::AppServerSession,
    config: &mut Config,
) -> color_eyre::Result<()> {
    use codex_app_server_protocol::ClientRequest;
    use codex_app_server_protocol::ConfigReadParams;
    use codex_app_server_protocol::ConfigReadResponse;
    use codex_app_server_protocol::RequestId;

    let response: ConfigReadResponse = match session
        .request_handle()
        .request_typed(ClientRequest::ConfigRead {
            request_id: RequestId::String(format!("startup-settings-{}", uuid::Uuid::new_v4())),
            params: ConfigReadParams {
                include_layers: false,
                cwd: super::thread_cwd_from_config(
                    config,
                    session.thread_params_mode(),
                    session.remote_cwd_override(),
                ),
            },
        })
        .await
    {
        Ok(response) => response,
        Err(codex_app_server_client::TypedRequestError::Server { source, .. })
            if source.code == -32601 =>
        {
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    if session.launch_overrides.model.is_none() && !has_launch_setting(config, "model") {
        config.model = response.config.model;
    }
    if !has_launch_setting(config, "model_reasoning_effort") {
        config.model_reasoning_effort = response.config.model_reasoning_effort;
    }
    if session.launch_overrides.service_tier.is_none()
        && !has_launch_setting(config, "service_tier")
    {
        config.service_tier = response.config.service_tier;
    }
    if session.launch_overrides.model_provider.is_none()
        && !has_launch_setting(config, "model_provider")
    {
        config.model_provider_id = response
            .config
            .model_provider
            .unwrap_or_else(|| "openai".to_string());
    }
    Ok(())
}

pub(super) fn apply_managed_new_thread_defaults(
    config: &mut Config,
    defaults: Option<&NewThreadModelDefaults>,
    launch_overrides: &ConfigOverrides,
    provider_override: Option<&str>,
) {
    let Some(defaults) = defaults else {
        return;
    };
    let selected_provider = provider_override
        .or(launch_overrides.model_provider.as_deref())
        .unwrap_or(&config.model_provider_id);
    // A custom provider may intentionally clear both the model and service tier.
    if selected_provider != "openai" {
        return;
    }
    // A model and its effort must stay together.
    let explicit_model = launch_overrides.model.is_some()
        || has_launch_setting(config, "model")
        || has_launch_setting(config, "model_reasoning_effort");
    let explicit_tier =
        launch_overrides.service_tier.is_some() || has_launch_setting(config, "service_tier");
    if !explicit_model {
        if let Some(model) = &defaults.model {
            config.model = Some(model.clone());
        }
        if let Some(effort) = &defaults.model_reasoning_effort {
            config.model_reasoning_effort = Some(effort.clone());
        }
    }
    if !explicit_tier && let Some(tier) = &defaults.service_tier {
        config.service_tier = Some(
            ServiceTier::from_request_value(tier)
                .map(|tier| tier.request_value().to_string())
                .unwrap_or_else(|| tier.clone()),
        );
    }
}

#[cfg(test)]
#[path = "new_thread_defaults_tests.rs"]
mod tests;
