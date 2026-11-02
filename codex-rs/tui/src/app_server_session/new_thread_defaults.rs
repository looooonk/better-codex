use crate::legacy_core::config::Config;
use crate::legacy_core::config::ConfigOverrides;
use codex_app_server_protocol::NewThreadModelDefaults;
use codex_config::ConfigLayerSource;
use codex_protocol::config_types::ServiceTier;

pub(super) fn apply_managed_new_thread_defaults(
    config: &mut Config,
    defaults: Option<&NewThreadModelDefaults>,
    launch_overrides: &ConfigOverrides,
    provider_override: Option<&str>,
) {
    let Some(defaults) = defaults else {
        return;
    };
    let has_launch_setting = |key: &str| {
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
    };
    let selected_provider = provider_override
        .or(launch_overrides.model_provider.as_deref())
        .or_else(|| {
            has_launch_setting("model_provider").then_some(config.model_provider_id.as_str())
        });
    // A custom provider may intentionally clear both the model and service tier.
    if selected_provider.is_some_and(|provider| provider != "openai") {
        return;
    }
    // A model and its effort must stay together.
    let explicit_model = launch_overrides.model.is_some()
        || has_launch_setting("model")
        || has_launch_setting("model_reasoning_effort");
    let explicit_tier =
        launch_overrides.service_tier.is_some() || has_launch_setting("service_tier");
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
