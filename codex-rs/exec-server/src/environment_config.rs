use codex_config::CONFIG_TOML_FILE;
use codex_config::format_config_layer_source;
use codex_config::host_name;
use codex_config::loader::LocalTomlLayerStack;
use codex_config::loader::load_local_config_layers;
use codex_exec_server_protocol::EnvironmentConfigLayer;
use codex_exec_server_protocol::EnvironmentConfigLayerStack;
use codex_exec_server_protocol::EnvironmentConfigReadParams;
use codex_exec_server_protocol::EnvironmentConfigReadResponse;
use codex_file_system::ExecutorFileSystem;
use codex_utils_home_dir::find_codex_home;
use codex_utils_path_uri::PathUri;

#[cfg(test)]
#[path = "environment_config_tests.rs"]
mod tests;

const MAX_ENVIRONMENT_CONFIG_SELECTORS: usize = 64;
const MAX_ENVIRONMENT_CONFIG_SELECTOR_COMPONENTS: usize = 32;
const MAX_ENVIRONMENT_CONFIG_SELECTOR_BYTES: usize = 64 * 1024;
const MAX_ENVIRONMENT_CONFIG_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ReadEnvironmentConfigError {
    #[error("{0}")]
    InvalidParams(String),
    #[error("{0}")]
    Internal(String),
}

pub(crate) async fn read_environment_config(
    file_system: &dyn ExecutorFileSystem,
    params: EnvironmentConfigReadParams,
) -> Result<EnvironmentConfigReadResponse, ReadEnvironmentConfigError> {
    validate_paths(&params)?;
    let cwd = params
        .cwd
        .to_abs_path()
        .map_err(|error| ReadEnvironmentConfigError::InvalidParams(error.to_string()))?;
    let codex_home = find_codex_home().map_err(|error| {
        ReadEnvironmentConfigError::Internal(format!("failed to find Codex home: {error}"))
    })?;
    let layers = load_local_config_layers(file_system, codex_home.as_path(), &cwd)
        .await
        .map_err(|error| {
            ReadEnvironmentConfigError::Internal(format!(
                "failed to load executor-local config: {error}"
            ))
        })?
        .project(&params.config_paths, &params.requirements_paths);
    let response = EnvironmentConfigReadResponse {
        user_home_dir: dirs::home_dir()
            .and_then(|home_dir| PathUri::from_host_native_path(home_dir).ok()),
        codex_home_dir: PathUri::from_abs_path(&codex_home),
        hostname: host_name(),
        config: serialize_layer_stack(layers.config, |source| {
            format_config_layer_source(source, CONFIG_TOML_FILE)
        })?,
        requirements: serialize_layer_stack(layers.requirements, ToString::to_string)?,
    };
    validate_response_size(&response)?;
    Ok(response)
}

fn validate_paths(params: &EnvironmentConfigReadParams) -> Result<(), ReadEnvironmentConfigError> {
    let selector_count = params
        .config_paths
        .len()
        .checked_add(params.requirements_paths.len())
        .ok_or_else(|| invalid_params("too many TOML selectors"))?;
    if selector_count == 0 {
        return Err(invalid_params(
            "at least one config or requirements path is required",
        ));
    }
    if selector_count > MAX_ENVIRONMENT_CONFIG_SELECTORS {
        return Err(invalid_params(format!(
            "at most {MAX_ENVIRONMENT_CONFIG_SELECTORS} TOML selectors are allowed"
        )));
    }

    let mut selector_bytes = 0usize;
    for path in params.config_paths.iter().chain(&params.requirements_paths) {
        if path.is_empty() {
            return Err(invalid_params(
                "TOML paths must contain at least one key segment",
            ));
        }
        if path.len() > MAX_ENVIRONMENT_CONFIG_SELECTOR_COMPONENTS {
            return Err(invalid_params(format!(
                "TOML paths may contain at most {MAX_ENVIRONMENT_CONFIG_SELECTOR_COMPONENTS} key segments"
            )));
        }
        for segment in path {
            selector_bytes = selector_bytes
                .checked_add(segment.len())
                .ok_or_else(|| invalid_params("TOML selector input is too large"))?;
            if selector_bytes > MAX_ENVIRONMENT_CONFIG_SELECTOR_BYTES {
                return Err(invalid_params(format!(
                    "TOML selector input may contain at most {MAX_ENVIRONMENT_CONFIG_SELECTOR_BYTES} bytes"
                )));
            }
        }
    }
    Ok(())
}

fn serialize_layer_stack<S>(
    stack: LocalTomlLayerStack<S>,
    source_name: impl Fn(&S) -> String,
) -> Result<EnvironmentConfigLayerStack, ReadEnvironmentConfigError> {
    let layers = stack
        .layers
        .into_iter()
        .map(|layer| {
            let toml = toml::to_string(&layer.toml).map_err(|error| {
                ReadEnvironmentConfigError::Internal(format!(
                    "failed to serialize executor-local config: {error}"
                ))
            })?;
            Ok(EnvironmentConfigLayer {
                source: source_name(&layer.source),
                base_dir: PathUri::from_abs_path(&layer.base_dir),
                toml,
            })
        })
        .collect::<Result<Vec<_>, ReadEnvironmentConfigError>>()?;
    Ok(EnvironmentConfigLayerStack {
        layers,
        cloud_insertion_index: stack.cloud_insertion_index,
    })
}

fn validate_response_size(
    response: &EnvironmentConfigReadResponse,
) -> Result<(), ReadEnvironmentConfigError> {
    let response_bytes = serde_json::to_vec(response).map_err(|error| {
        ReadEnvironmentConfigError::Internal(format!(
            "failed to serialize executor-local config response: {error}"
        ))
    })?;
    if response_bytes.len() > MAX_ENVIRONMENT_CONFIG_RESPONSE_BYTES {
        return Err(invalid_params(format!(
            "environment config response exceeds the {MAX_ENVIRONMENT_CONFIG_RESPONSE_BYTES}-byte limit"
        )));
    }
    Ok(())
}

fn invalid_params(message: impl Into<String>) -> ReadEnvironmentConfigError {
    ReadEnvironmentConfigError::InvalidParams(message.into())
}
