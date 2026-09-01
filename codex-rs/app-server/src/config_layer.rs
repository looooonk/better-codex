use codex_app_server_protocol::ConfigLayer as ApiConfigLayer;
use codex_app_server_protocol::ConfigLayerMetadata as ApiConfigLayerMetadata;
use codex_app_server_protocol::ConfigLayerSource as ApiConfigLayerSource;
use codex_config::ConfigLayer;
use codex_config::ConfigLayerMetadata;
use codex_config::ConfigLayerSource;

/// Converts a config-layer source into the public app-server wire type.
///
/// Packaged defaults are internal and are omitted from public responses.
pub(crate) fn config_layer_source_to_api(
    source: ConfigLayerSource,
) -> Option<ApiConfigLayerSource> {
    Some(match source {
        ConfigLayerSource::PackagedDefaults { .. } => return None,
        ConfigLayerSource::Mdm { domain, key } => ApiConfigLayerSource::Mdm { domain, key },
        ConfigLayerSource::System { file } => ApiConfigLayerSource::System { file },
        ConfigLayerSource::EnterpriseManaged { id, name } => {
            ApiConfigLayerSource::EnterpriseManaged { id, name }
        }
        ConfigLayerSource::User { file, profile } => ApiConfigLayerSource::User { file, profile },
        ConfigLayerSource::Project { dot_codex_folder } => {
            ApiConfigLayerSource::Project { dot_codex_folder }
        }
        ConfigLayerSource::SessionFlags => ApiConfigLayerSource::SessionFlags,
        ConfigLayerSource::LegacyManagedConfigTomlFromFile { file } => {
            ApiConfigLayerSource::LegacyManagedConfigTomlFromFile { file }
        }
        ConfigLayerSource::LegacyManagedConfigTomlFromMdm => {
            ApiConfigLayerSource::LegacyManagedConfigTomlFromMdm
        }
    })
}

/// Converts public config-layer metadata, omitting internal packaged defaults.
pub(crate) fn config_layer_metadata_to_api(
    metadata: ConfigLayerMetadata,
) -> Option<ApiConfigLayerMetadata> {
    Some(ApiConfigLayerMetadata {
        name: config_layer_source_to_api(metadata.name)?,
        version: metadata.version,
    })
}

/// Converts a public config layer, omitting internal packaged defaults.
pub(crate) fn config_layer_to_api(layer: ConfigLayer) -> Option<ApiConfigLayer> {
    Some(ApiConfigLayer {
        name: config_layer_source_to_api(layer.name)?,
        version: layer.version,
        config: layer.config,
        disabled_reason: layer.disabled_reason,
    })
}
