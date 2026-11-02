use super::bootstrap_request_error;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::Model as ApiModel;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::RequestId;
use codex_protocol::openai_models::ModelAvailabilityNux;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelServiceTier;
use codex_protocol::openai_models::ModelUpgrade;
use codex_protocol::openai_models::ReasoningEffortPreset;
use color_eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::WrapErr;
use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;
use uuid::Uuid;

pub(crate) async fn load_models(handle: AppServerRequestHandle) -> Result<Vec<ModelPreset>> {
    read_catalog(|params| {
        handle.request_typed(ClientRequest::ModelList {
            request_id: RequestId::String(Uuid::new_v4().to_string()),
            params,
        })
    })
    .await
}

async fn read_catalog<F, Fut>(mut read_page: F) -> Result<Vec<ModelPreset>>
where
    F: FnMut(ModelListParams) -> Fut,
    Fut: Future<Output = Result<ModelListResponse, TypedRequestError>>,
{
    let mut models = Vec::new();
    let mut cursor = None;
    let mut cursors = HashSet::new();
    for _ in 0..100 {
        let page = tokio::time::timeout(
            Duration::from_secs(/*secs*/ 30),
            read_page(ModelListParams {
                cursor,
                limit: Some(100),
                include_hidden: Some(true),
            }),
        )
        .await
        .wrap_err("model catalog request timed out")?
        .map_err(|err| bootstrap_request_error("model/list failed during TUI bootstrap", err))?;
        if page.data.len() > 100 {
            bail!("model catalog page exceeded its requested size");
        }
        models.extend(page.data.into_iter().map(model_preset_from_api_model));
        cursor = page.next_cursor;
        let Some(next) = &cursor else {
            return Ok(models);
        };
        if !cursors.insert(next.clone()) {
            bail!("server returned a repeated model catalog cursor");
        }
    }
    bail!("model catalog exceeds the 10,000-model limit");
}

fn model_preset_from_api_model(model: ApiModel) -> ModelPreset {
    let upgrade = model.upgrade.map(|upgrade_id| {
        let upgrade_info = model.upgrade_info.clone();
        ModelUpgrade {
            id: upgrade_id,
            migration_config_key: model.model.clone(),
            retirement_at: model
                .upgrade_info
                .as_ref()
                .and_then(|info| info.retirement_at)
                .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0)),
            model_link: upgrade_info
                .as_ref()
                .and_then(|info| info.model_link.clone()),
            upgrade_copy: upgrade_info
                .as_ref()
                .and_then(|info| info.upgrade_copy.clone()),
            migration_markdown: upgrade_info.and_then(|info| info.migration_markdown),
        }
    });

    ModelPreset {
        id: model.id,
        model: model.model,
        display_name: model.display_name,
        description: model.description,
        model_specialty: model.model_specialty,
        available_access_programs: model.available_access_programs.map(Into::into),
        default_reasoning_effort: model.default_reasoning_effort,
        supported_reasoning_efforts: model
            .supported_reasoning_efforts
            .into_iter()
            .map(|effort| ReasoningEffortPreset {
                effort: effort.reasoning_effort,
                description: effort.description,
            })
            .collect(),
        supports_personality: model.supports_personality,
        additional_speed_tiers: model.additional_speed_tiers,
        service_tiers: model
            .service_tiers
            .into_iter()
            .map(|service_tier| ModelServiceTier {
                id: service_tier.id,
                name: service_tier.name,
                description: service_tier.description,
            })
            .collect(),
        default_service_tier: model.default_service_tier,
        is_default: model.is_default,
        upgrade,
        show_in_picker: !model.hidden,
        availability_nux: model.availability_nux.map(|nux| ModelAvailabilityNux {
            message: nux.message,
        }),
        // `model/list` already returns models filtered for the active client/auth context.
        supported_in_api: true,
        input_modalities: model.input_modalities,
        multi_agent_version: model
            .multi_agent_version
            .map(codex_app_server_protocol::MultiAgentVersion::to_core),
    }
}


#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
