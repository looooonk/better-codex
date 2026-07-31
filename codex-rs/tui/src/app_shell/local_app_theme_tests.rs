use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_config::LoaderOverrides;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn profile_v2_theme_persistence_only_updates_selected_local_config() {
    let codex_home = tempfile::tempdir().expect("create temp codex home");
    let base_config_path = codex_home.path().join(CONFIG_TOML_FILE);
    let selected_config_path = codex_home.path().join("profiles/work.toml");
    let default_profile_path = codex_home.path().join("work.config.toml");
    tokio::fs::create_dir_all(
        selected_config_path
            .parent()
            .expect("selected config should have a parent"),
    )
    .await
    .expect("create custom profile directory");

    let base_config = r#"[tui]
app_theme = "tokyo-night"
"#;
    let selected_config = r#"[tui]
animations = false
"#;
    tokio::fs::write(&base_config_path, base_config)
        .await
        .expect("write base config");
    tokio::fs::write(&selected_config_path, selected_config)
        .await
        .expect("write selected config");

    let selected_config_path = AbsolutePathBuf::from_absolute_path(selected_config_path)
        .expect("selected config path should be absolute");
    let loader_overrides = LoaderOverrides {
        user_config_path: Some(selected_config_path.clone()),
        user_config_profile: Some("work".parse().expect("profile-v2 name")),
        ..LoaderOverrides::without_managed_config_for_tests()
    };
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .loader_overrides(loader_overrides.clone())
        .build()
        .await
        .expect("load profile-v2 config");

    assert_eq!(config.tui_app_theme, TuiAppTheme::TokyoNight);
    assert_eq!(super::selected_config_path(&config), selected_config_path);

    super::persist(
        super::selected_config_path(&config),
        TuiAppTheme::CatppuccinMocha,
    )
    .await
    .expect("persist selected app theme");

    assert_eq!(
        tokio::fs::read_to_string(&base_config_path)
            .await
            .expect("read unchanged base config"),
        base_config
    );
    assert_eq!(
        tokio::fs::read_to_string(selected_config_path.as_path())
            .await
            .expect("read selected config"),
        r#"[tui]
animations = false
app_theme = "catppuccin-mocha"
"#
    );
    assert!(
        !default_profile_path.exists(),
        "persistence should not create the derived profile path"
    );

    let reloaded_config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .loader_overrides(loader_overrides)
        .build()
        .await
        .expect("reload profile-v2 config");
    assert_eq!(reloaded_config.tui_app_theme, TuiAppTheme::CatppuccinMocha);
}
