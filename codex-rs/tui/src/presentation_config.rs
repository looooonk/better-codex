//! Refresh the selected local presentation layer without dropping its precedence.

use codex_config::ConfigLayerStack;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::Result;

pub(crate) async fn reload(
    stack: &ConfigLayerStack,
    path: &AbsolutePathBuf,
) -> Result<ConfigLayerStack> {
    let contents = tokio::fs::read_to_string(path.as_path()).await?;
    Ok(stack.with_user_config(path, toml::from_str(&contents)?)?)
}
