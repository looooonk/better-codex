use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_config::LoaderOverrides;
use pretty_assertions::assert_eq;

#[test]
fn memory_arguments_distinguish_session_and_global_settings() {
    assert_eq!(
        MemoryCommand::parse("off").unwrap(),
        MemoryCommand::Session(ThreadMemoryMode::Disabled)
    );
    assert_eq!(
        MemoryCommand::parse("use off").unwrap(),
        MemoryCommand::Use(false)
    );
    assert_eq!(
        MemoryCommand::parse("generate on").unwrap(),
        MemoryCommand::Generate(true)
    );
    assert!(MemoryCommand::parse("generate maybe").is_err());
}

#[tokio::test]
async fn disabling_memory_use_preserves_generation_preference() -> Result<()> {
    let home = tempfile::tempdir()?;
    let path = home.path().join("config.toml");
    tokio::fs::write(
        &path,
        "[memories]\nuse_memories = true\ngenerate_memories = true\n",
    )
    .await?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    let server = crate::start_embedded_app_server_for_picker(&config).await?;
    let response = execute(
        server.request_handle(),
        codex_protocol::ThreadId::new().to_string(),
        home.path().display().to_string(),
        MemoryCommand::Use(false),
    )
    .await?;
    let WorkspaceResponse::Notice(message) = response else {
        panic!("expected memory status")
    };
    insta::assert_snapshot!("memory_preferences", message);
    let saved: toml::Value = toml::from_str(&tokio::fs::read_to_string(path).await?)?;
    assert_eq!(
        saved["memories"]
            .clone()
            .try_into::<std::collections::BTreeMap<String, bool>>()?,
        std::collections::BTreeMap::from([
            ("use_memories".into(), false),
            ("generate_memories".into(), true)
        ])
    );
    server.shutdown().await?;
    Ok(())
}
