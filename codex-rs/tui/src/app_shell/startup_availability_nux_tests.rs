use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_config::types::ModelAvailabilityNuxConfig;
use codex_protocol::openai_models::ModelAvailabilityNux;
use codex_protocol::openai_models::ReasoningEffort;
use std::collections::HashMap;

#[tokio::test]
async fn selects_only_the_highest_priority_availability_nux() {
    let mut models = test_models();
    models[0].availability_nux = Some(ModelAvailabilityNux {
        message: "newest".to_string(),
    });
    models[1].availability_nux = Some(ModelAvailabilityNux {
        message: "older".to_string(),
    });
    let config = test_config(HashMap::new()).await;

    assert_eq!(
        select(&models, &config),
        Some(AvailabilityNux {
            model_slug: models[0].model.clone(),
            message: "newest".to_string(),
        })
    );
}

#[tokio::test]
async fn exhausted_newest_nux_does_not_fall_back() {
    let mut models = test_models();
    models[0].availability_nux = Some(ModelAvailabilityNux {
        message: "newest".to_string(),
    });
    models[1].availability_nux = Some(ModelAvailabilityNux {
        message: "older".to_string(),
    });
    let config = test_config(HashMap::from([(models[0].model.clone(), MAX_SHOW_COUNT)])).await;

    assert_eq!(select(&models, &config), None);
}

fn test_models() -> Vec<ModelPreset> {
    ["newest-model", "older-model"]
        .into_iter()
        .map(|slug| ModelPreset {
            id: slug.to_string(),
            model: slug.to_string(),
            display_name: slug.to_string(),
            description: format!("{slug} description"),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: Vec::new(),
            supports_personality: false,
            additional_speed_tiers: Vec::new(),
            service_tiers: Vec::new(),
            default_service_tier: None,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            multi_agent_version: None,
            availability_nux: None,
            supported_in_api: true,
            input_modalities: Vec::new(),
        })
        .collect()
}

async fn test_config(shown_count: HashMap<String, u32>) -> Config {
    let codex_home = tempfile::tempdir().expect("tempdir").keep();
    let mut config = ConfigBuilder::default()
        .codex_home(codex_home)
        .build()
        .await
        .expect("config");
    config.model_availability_nux = ModelAvailabilityNuxConfig { shown_count };
    config
}
