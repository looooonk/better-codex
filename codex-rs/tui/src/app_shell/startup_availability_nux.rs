use crate::config_update;
use crate::legacy_core::config::Config;
use codex_app_server_client::AppServerRequestHandle;
use codex_protocol::openai_models::ModelAvailabilityNux;
use codex_protocol::openai_models::ModelPreset;

const MAX_SHOW_COUNT: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
struct AvailabilityNux {
    model_slug: String,
    message: String,
}

pub(super) async fn prepare(
    request_handle: AppServerRequestHandle,
    config: &mut Config,
    available_models: &[ModelPreset],
) -> Option<String> {
    if !config.show_tooltips {
        return None;
    }

    let nux = select(available_models, config)?;
    let mut shown_count = config.model_availability_nux.shown_count.clone();
    let count = shown_count
        .get(&nux.model_slug)
        .copied()
        .unwrap_or_default()
        .saturating_add(1);
    shown_count.insert(nux.model_slug.clone(), count);

    if let Err(err) = config_update::write_config_batch(
        request_handle,
        config_update::build_model_availability_nux_count_edits(&shown_count),
    )
    .await
    {
        tracing::error!(
            error = %err,
            model = %nux.model_slug,
            "failed to persist model availability nux count"
        );
        return Some(nux.message);
    }

    config.model_availability_nux.shown_count = shown_count;
    Some(nux.message)
}

fn select(available_models: &[ModelPreset], config: &Config) -> Option<AvailabilityNux> {
    let preset = available_models
        .iter()
        .find(|preset| preset.availability_nux.is_some())?;
    let ModelAvailabilityNux { message } = preset.availability_nux.as_ref()?;
    let shown_count = config
        .model_availability_nux
        .shown_count
        .get(&preset.model)
        .copied()
        .unwrap_or_default();

    (shown_count < MAX_SHOW_COUNT).then(|| AvailabilityNux {
        model_slug: preset.model.clone(),
        message: message.clone(),
    })
}

#[cfg(test)]
#[path = "startup_availability_nux_tests.rs"]
mod tests;
