use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_config::LoaderOverrides;
use pretty_assertions::assert_eq;

#[test]
fn preset_arguments_preserve_multiline_input_and_validate_slots() {
    assert_eq!(
        PresetCommand::parse("save 2 Explain this:\nsecond line").unwrap(),
        PresetCommand::Save(2, QuickDial { input: "Explain this:\nsecond line".into(), label: None }),
    );
    assert_eq!(PresetCommand::parse("run 2").unwrap(), PresetCommand::Load(2));
    for invalid in ["save 0 text", "save 10 text", "save 2", "save 2 !rm -rf .", "delete 2 extra"] {
        assert!(PresetCommand::parse(invalid).is_err(), "{invalid}");
    }
}

#[tokio::test]
async fn native_presets_round_trip_without_changing_other_settings() -> Result<()> {
    let home = tempfile::tempdir()?;
    let path = home.path().join("config.toml");
    tokio::fs::write(&path, "model = \"gpt-6-luna\"\n").await?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build().await?;
    let server = crate::start_embedded_app_server_for_picker(&config).await?;
    let cwd = home.path().display().to_string();
    execute(server.request_handle(), cwd.clone(), PresetCommand::parse("save 3 Review the current changes")?).await?;
    let loaded = execute(server.request_handle(), cwd.clone(), PresetCommand::Load(3)).await?;
    let WorkspaceResponse::Preset(input) = loaded else { panic!("expected loaded preset") };
    assert_eq!(input, "Review the current changes");
    let saved: toml::Value = toml::from_str(&tokio::fs::read_to_string(&path).await?)?;
    assert_eq!(saved["model"].as_str(), Some("gpt-6-luna"));
    assert_eq!(saved["tui"]["presets"]["3"]["input"].as_str(), Some(input.as_str()));
    execute(server.request_handle(), cwd.clone(), PresetCommand::Delete(3)).await?;
    assert!(execute(server.request_handle(), cwd, PresetCommand::Load(3)).await.is_err());
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn project_presets_cannot_be_silently_overwritten() -> Result<()> {
    let home = tempfile::tempdir()?;
    let workspace = tempfile::tempdir()?;
    tokio::fs::create_dir(workspace.path().join(".codex")).await?;
    tokio::fs::write(workspace.path().join(".codex/config.toml"), "[tui.presets.2]\ninput = \"project preset\"\n").await?;
    let path = home.path().join("config.toml");
    let original = format!("model = \"gpt-6-luna\"\n[projects.{}]\ntrust_level = \"trusted\"\n", serde_json::to_string(&workspace.path().display().to_string())?);
    tokio::fs::write(&path, &original).await?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(workspace.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build().await?;
    let server = crate::start_embedded_app_server_for_picker(&config).await?;
    let error = execute(server.request_handle(), workspace.path().display().to_string(), PresetCommand::parse("save 2 replacement")?).await.unwrap_err();
    assert!(error.to_string().contains("controlled by"), "{error:#}");
    assert_eq!(tokio::fs::read_to_string(&path).await?, original);
    server.shutdown().await?;
    Ok(())
}
