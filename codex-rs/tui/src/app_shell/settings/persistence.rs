use crate::config_update::replace_config_value;
use crate::config_update::write_config_batch;
use crate::config_update::write_config_batch_at_version;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigLayerSource;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::MergeStrategy;
use codex_app_server_protocol::RequestId;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use serde_json::Map;
use serde_json::Value as JsonValue;
use std::future::Future;
use uuid::Uuid;

struct UserConfigSnapshot {
    version: Option<String>,
    config: Map<String, JsonValue>,
}

pub(in crate::app_shell) async fn persist_settings_update<F>(
    request_handle: AppServerRequestHandle,
    edits: Vec<ConfigEdit>,
    thread_update: Option<F>,
) -> Result<()>
where
    F: Future<Output = Result<()>> + Send,
{
    let Some(thread_update) = thread_update else {
        write_config_batch(request_handle, edits).await?;
        return Ok(());
    };

    let snapshot = read_user_config_snapshot(request_handle.clone()).await?;
    let rollback_edits = rollback_edits(&snapshot.config, &edits)?;
    let write =
        write_config_batch_at_version(request_handle.clone(), edits, snapshot.version).await?;

    if let Err(thread_error) = thread_update.await {
        return match write_config_batch_at_version(
            request_handle,
            rollback_edits,
            Some(write.version),
        )
        .await
        {
            Ok(_) => Err(thread_error)
                .wrap_err("thread settings update failed; global config changes were rolled back"),
            Err(rollback_error) => Err(eyre!(
                "thread settings update failed: {thread_error:#}; global config rollback also failed: {rollback_error:#}"
            )),
        };
    }

    Ok(())
}

async fn read_user_config_snapshot(
    request_handle: AppServerRequestHandle,
) -> Result<UserConfigSnapshot> {
    let response: ConfigReadResponse = request_handle
        .request_typed(ClientRequest::ConfigRead {
            request_id: RequestId::String(format!("tui-settings-config-read-{}", Uuid::new_v4())),
            params: ConfigReadParams {
                include_layers: true,
                cwd: None,
            },
        })
        .await
        .wrap_err("config/read failed before saving TUI settings")?;
    let layers = response
        .layers
        .ok_or_else(|| eyre!("config/read omitted requested layers"))?;
    let Some(layer) = layers
        .into_iter()
        .find(|layer| matches!(&layer.name, ConfigLayerSource::User { profile: None, .. }))
    else {
        return Ok(UserConfigSnapshot {
            version: None,
            config: Map::new(),
        });
    };
    let JsonValue::Object(config) = layer.config else {
        bail!("base user config layer was not an object");
    };
    Ok(UserConfigSnapshot {
        version: Some(layer.version),
        config,
    })
}

fn rollback_edits(
    config: &Map<String, JsonValue>,
    edits: &[ConfigEdit],
) -> Result<Vec<ConfigEdit>> {
    edits
        .iter()
        .map(|edit| {
            if edit.merge_strategy != MergeStrategy::Replace || edit.key_path.contains(['.', '"']) {
                bail!(
                    "cannot safely roll back settings config key {}",
                    edit.key_path
                );
            }
            Ok(replace_config_value(
                &edit.key_path,
                config
                    .get(&edit.key_path)
                    .cloned()
                    .unwrap_or(JsonValue::Null),
            ))
        })
        .collect()
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;
