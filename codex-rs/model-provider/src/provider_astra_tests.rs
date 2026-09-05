use super::*;
use codex_model_provider_info::AMAZON_BEDROCK_GPT_6_ASTRA_MODEL_ID;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn configured_astra_catalog_uses_the_same_bedrock_capabilities_as_the_bundled_entry() {
    let home =
        std::env::temp_dir().join(format!("codex-astra-bedrock-test-{}", std::process::id()));
    let provider = create_model_provider(
        ModelProviderInfo::create_amazon_bedrock_provider(/*aws*/ None),
        /*auth_manager*/ None,
    );
    let bundled = provider.models_manager(home.clone(), /*config_model_catalog*/ None);
    let expected = bundled
        .get_model_info(AMAZON_BEDROCK_GPT_6_ASTRA_MODEL_ID, &Default::default())
        .await;
    let mut source = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .find(|model| model.slug == "gpt-6-astra")
        .unwrap();
    assert!(source.use_responses_lite);
    assert!(
        source
            .supported_reasoning_levels
            .iter()
            .any(|level| level.effort == ReasoningEffort::Ultra)
    );
    source.slug = AMAZON_BEDROCK_GPT_6_ASTRA_MODEL_ID.to_string();
    source.priority = expected.priority;
    source.availability_nux = expected.availability_nux.clone();
    source.upgrade = expected.upgrade.clone();
    let configured = provider.models_manager(
        home,
        Some(ModelsResponse {
            models: vec![source],
        }),
    );

    assert_eq!(
        configured
            .get_model_info(AMAZON_BEDROCK_GPT_6_ASTRA_MODEL_ID, &Default::default())
            .await,
        expected
    );
    assert_eq!(
        (
            expected.use_responses_lite,
            expected.tool_mode,
            expected
                .supported_reasoning_levels
                .iter()
                .any(|level| level.effort == ReasoningEffort::Ultra)
        ),
        (false, None, false),
    );
}
