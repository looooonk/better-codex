use super::AppServerSession;
use crate::legacy_core::config::Config;
use crate::legacy_core::config::ConfigOverrides;
use codex_config::ConfigLayerSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ResumeModelSettings {
    OverrideFromCurrentConfig,
    RestoreFromThread,
}

pub(super) struct ResumeSettings {
    pub(super) model: ResumeModelSettings,
    pub(super) provider: Option<String>,
    pub(super) permissions: super::ResumePermissions,
}

pub(super) fn explicitly_selected(config: &Config, keys: &[&str]) -> bool {
    config.config_layer_stack.layers_high_to_low().any(|layer| {
        matches!(
            layer.name,
            ConfigLayerSource::SessionFlags
                | ConfigLayerSource::User {
                    profile: Some(_),
                    ..
                }
        ) && keys.iter().any(|key| layer.config.get(*key).is_some())
    })
}

impl AppServerSession {
    pub(crate) fn with_launch_overrides(mut self, overrides: &ConfigOverrides) -> Self {
        self.launch_overrides = overrides.clone();
        self
    }

    pub(super) fn resume_settings(&self, config: &Config) -> ResumeSettings {
        let model = if self.launch_overrides.model.is_some()
            || self.launch_overrides.model_provider.is_some()
            || explicitly_selected(
                config,
                &["model", "model_provider", "model_reasoning_effort"],
            ) {
            ResumeModelSettings::OverrideFromCurrentConfig
        } else {
            ResumeModelSettings::RestoreFromThread
        };
        ResumeSettings {
            model,
            provider: self
                .launch_overrides
                .model_provider
                .as_ref()
                .map(|provider| {
                    config
                        .config_layer_stack
                        .required_model_provider()
                        .unwrap_or(provider)
                        .to_owned()
                }),
            permissions: super::ResumePermissions::from_overrides(config, &self.launch_overrides),
        }
    }
}

#[cfg(test)]
#[path = "resume_settings_tests.rs"]
mod tests;
