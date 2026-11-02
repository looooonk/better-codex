use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_app_server_protocol::ConfigRequirementsReadResponse;
use codex_protocol::openai_models::ModelServiceTier;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;

fn model() -> ModelPreset {
    ModelPreset {
        id: "model".to_string(),
        model: "model".to_string(),
        display_name: "Model".to_string(),
        description: String::new(),
        model_specialty: None,
        default_reasoning_effort: ReasoningEffort::Medium,
        supported_reasoning_efforts: Vec::new(),
        supports_personality: false,
        additional_speed_tiers: Vec::new(),
        service_tiers: ["priority", "ultrafast", "flex"].map(|id| ModelServiceTier {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
        }).into(),
        default_service_tier: Some("ultrafast".to_string()),
        available_access_programs: None,
        is_default: true,
        upgrade: None,
        show_in_picker: true,
        multi_agent_version: None,
        availability_nux: None,
        supported_in_api: true,
        input_modalities: Vec::new(),
    }
}

#[test]
fn remote_speed_requirements_preserve_independent_modes_and_clear_disabled_defaults() {
    for (independent, expected_tiers, expected_default) in [
        (Some(true), vec!["ultrafast", "flex"], Some("ultrafast")),
        (None, vec!["flex"], None),
    ] {
        let requirements: ConfigRequirementsReadResponse = serde_json::from_value(serde_json::json!({
            "supportsIndependentSpeedModes": independent,
            "requirements": {"featureRequirements": {"fast_mode": false}}
        })).expect("requirements");
        let mut actual = [model()];
        let mut expected = model();
        expected.service_tiers.retain(|tier| expected_tiers.contains(&tier.id.as_str()));
        expected.default_service_tier = expected_default.map(str::to_string);
        constrain_server_service_tiers(&mut actual, &requirements);
        assert_eq!(actual, [expected]);
    }
}

#[tokio::test]
async fn flex_and_ultrafast_can_run_with_fast_mode_disabled() {
    let home = tempfile::tempdir().expect("tempdir");
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .loader_overrides(codex_config::LoaderOverrides::without_managed_config_for_tests())
        .build().await.expect("config");
    config.features.disable(Feature::FastMode).expect("disable Fast mode");
    config.features.enable(Feature::UltrafastMode).expect("enable Ultra Fast mode");
    let models = [model()];
    assert_eq!(effective_service_tier(&config, "model", &models), Some("ultrafast".to_string()));
    config.service_tier = Some("flex".to_string());
    config.features.disable(Feature::UltrafastMode).expect("disable Ultra Fast mode");
    assert_eq!(service_tier_update_for_core(&config, "model", &models), Some(Some("flex".to_string())));
    config.service_tier = Some("priority".to_string());
    assert_eq!(effective_service_tier(&config, "model", &models), None);
}
