use crate::app_theme::TuiAppTheme;
use crate::app_theme::app_theme_edit;
use crate::legacy_core::config::Config;
use crate::legacy_core::config::edit::ConfigEditsBuilder;
use codex_config::CONFIG_TOML_FILE;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::Result;
use color_eyre::eyre::eyre;

pub(super) fn selected_config_path(config: &Config) -> AbsolutePathBuf {
    let path = config
        .config_layer_stack
        .get_user_config_file()
        .cloned()
        .unwrap_or_else(|| config.codex_home.join(CONFIG_TOML_FILE));
    AbsolutePathBuf::resolve_path_against_base(
        codex_config::user_config_file::write_path(&config.codex_home, path.as_path()),
        &config.codex_home,
    )
}

pub(super) async fn persist(config_path: AbsolutePathBuf, app_theme: TuiAppTheme) -> Result<()> {
    ConfigEditsBuilder::for_config_path(&config_path)
        .with_edits([app_theme_edit(app_theme)])
        .apply()
        .await
        .map_err(|err| eyre!("failed to persist local app theme: {err:#}"))
}

#[cfg(test)]
#[path = "local_app_theme_tests.rs"]
mod tests;
