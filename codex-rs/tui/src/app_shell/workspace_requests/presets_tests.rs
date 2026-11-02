use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_config::LoaderOverrides;
use pretty_assertions::assert_eq;

#[test]
fn preset_arguments_preserve_multiline_input_and_validate_slots() {
    assert_eq!(
        PresetCommand::parse("save 2 Explain this:\nsecond line").unwrap(),
        PresetCommand::Save(
            2,
            QuickDial {
                input: "Explain this:\nsecond line".into(),
                label: None
            }
        ),
    );
    assert_eq!(
        PresetCommand::parse("run 2").unwrap(),
        PresetCommand::Load(2)
    );
    for invalid in [
        "save 0 text",
        "save 10 text",
        "save 2",
        "save 2 !rm -rf .",
        "delete 2 extra",
    ] {
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
        .build()
        .await?;
    let server = crate::start_embedded_app_server_for_picker(&config).await?;
    let cwd = home.path().display().to_string();
    execute(
        server.request_handle(),
        cwd.clone(),
        PresetCommand::parse("save 3 Review the current changes")?,
    )
    .await?;
    let loaded = execute(server.request_handle(), cwd.clone(), PresetCommand::Load(3)).await?;
    let WorkspaceResponse::Preset(input) = loaded else {
        panic!("expected loaded preset")
    };
    assert_eq!(input, "Review the current changes");
    let saved: toml::Value = toml::from_str(&tokio::fs::read_to_string(&path).await?)?;
    assert_eq!(saved["model"].as_str(), Some("gpt-6-luna"));
    assert_eq!(
        saved["tui"]["presets"]["3"]["input"].as_str(),
        Some(input.as_str())
    );
    execute(
        server.request_handle(),
        cwd.clone(),
        PresetCommand::Delete(3),
    )
    .await?;
    assert!(
        execute(server.request_handle(), cwd, PresetCommand::Load(3))
            .await
            .is_err()
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn project_presets_cannot_be_silently_overwritten() -> Result<()> {
    let home = tempfile::tempdir()?;
    let workspace = tempfile::tempdir()?;
    tokio::fs::create_dir(workspace.path().join(".codex")).await?;
    tokio::fs::write(
        workspace.path().join(".codex/config.toml"),
        "[tui.presets.2]\ninput = \"project preset\"\n",
    )
    .await?;
    let path = home.path().join("config.toml");
    let original = format!(
        "model = \"gpt-6-luna\"\n[projects.{}]\ntrust_level = \"trusted\"\n",
        serde_json::to_string(&workspace.path().display().to_string())?
    );
    tokio::fs::write(&path, &original).await?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(workspace.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    let server = crate::start_embedded_app_server_for_picker(&config).await?;
    let error = execute(
        server.request_handle(),
        workspace.path().display().to_string(),
        PresetCommand::parse("save 2 replacement")?,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("controlled by"), "{error:#}");
    assert_eq!(tokio::fs::read_to_string(&path).await?, original);
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn profile_preset_deletion_preserves_and_reports_inherited_values() -> Result<()> {
    let home = tempfile::tempdir()?;
    let base_path = home.path().join("config.toml");
    let base = "[tui.presets.2]\ninput = 'base second preset'\n[tui.presets.3]\ninput = 'base third preset'\n";
    tokio::fs::write(&base_path, base).await?;
    let profile_path = codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
        home.path().join("work.config.toml"),
    )?;
    let profile = "[tui.presets.3]\ninput = 'profile third preset'\n";
    tokio::fs::write(&profile_path, profile).await?;
    let loader = LoaderOverrides {
        user_config_path: Some(profile_path.clone()),
        user_config_profile: Some("work".parse()?),
        ..LoaderOverrides::without_managed_config_for_tests()
    };
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(loader.clone())
        .build()
        .await?;
    let client = crate::start_embedded_app_server(
        Default::default(),
        config,
        Vec::new(),
        loader,
        /*strict_config*/ false,
        crate::CloudConfigBundleLoader::default(),
        codex_feedback::CodexFeedback::new(),
        /*log_db*/ None,
        /*state_db*/ None,
        std::sync::Arc::new(crate::EnvironmentManager::default_for_tests()),
        Default::default(),
    )
    .await?;
    let server = crate::app_server_session::AppServerSession::new(
        codex_app_server_client::AppServerClient::InProcess(client),
        crate::app_server_session::ThreadParamsMode::Embedded,
    );
    let cwd = home.path().display().to_string();
    let error = execute(
        server.request_handle(),
        cwd.clone(),
        PresetCommand::Delete(2),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "preset 2 is inherited from {}; remove it there to delete it",
            base_path.display()
        )
    );
    assert_eq!(tokio::fs::read_to_string(&profile_path).await?, profile);

    let result = execute(
        server.request_handle(),
        cwd.clone(),
        PresetCommand::Delete(3),
    )
    .await?;
    let WorkspaceResponse::Notice(message) = result else {
        panic!("expected deletion notice")
    };
    insta::assert_snapshot!(message, @"Removed preset 3 from the active configuration. An inherited preset remains available; use /presets 3 to load it.");
    let loaded = execute(server.request_handle(), cwd, PresetCommand::Load(3)).await?;
    let WorkspaceResponse::Preset(input) = loaded else {
        panic!("expected inherited preset")
    };
    assert_eq!(input, "base third preset");
    assert_eq!(tokio::fs::read_to_string(&base_path).await?, base);
    let saved: toml::Value = toml::from_str(&tokio::fs::read_to_string(&profile_path).await?)?;
    assert!(
        saved
            .get("tui")
            .and_then(|tui| tui.get("presets"))
            .and_then(|presets| presets.get("3"))
            .is_none()
    );
    server.shutdown().await?;
    Ok(())
}
