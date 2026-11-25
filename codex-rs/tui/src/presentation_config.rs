//! Refresh the selected local presentation layer without dropping its precedence.

use codex_config::ConfigLayerStack;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::Result;

pub(crate) async fn reload(
    stack: &ConfigLayerStack,
    path: &AbsolutePathBuf,
) -> Result<ConfigLayerStack> {
    let contents = tokio::fs::read_to_string(path.as_path()).await?;
    // The first save can fork a previously inherited layer. Preserve its slot
    // in the existing stack until the next full configuration reload.
    let layer = stack.get_user_config_file().unwrap_or(path);
    Ok(stack.with_user_config(layer, toml::from_str(&contents)?)?)
}
