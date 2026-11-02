use super::super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn resume_does_not_replace_saved_provider_with_current_default() {
    let home = tempfile::tempdir().expect("tempdir");
    std::fs::write(home.path().join("config.toml"), "model_provider = 'ollama'").expect("config");
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build().await.expect("config");
    let params = thread_resume_params_from_config(config, ThreadId::new(), ThreadParamsMode::Embedded,
        /*remote_cwd_override*/ None,
        ResumeModelSettings::OverrideFromCurrentConfig,
        ResumePermissions::CURRENT_CONFIG,
    );
    assert_eq!(params.model_provider, None);
}

#[tokio::test]
async fn explicit_remote_provider_reaches_start_resume_and_fork() {
    let home = tempfile::tempdir().expect("tempdir");
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .cli_overrides(vec![("model_provider".to_string(), "ollama".into())])
        .build().await.expect("config");
    let thread_id = ThreadId::new();
    let start = thread_start_params_from_config(&config, ThreadParamsMode::Remote,
        /*remote_cwd_override*/ None, /*session_start_source*/ None);
    let resume = thread_resume_params_from_config(config.clone(), thread_id, ThreadParamsMode::Remote,
        /*remote_cwd_override*/ None,
        ResumeModelSettings::OverrideFromCurrentConfig,
        ResumePermissions::CURRENT_CONFIG,
    );
    let fork = thread_fork_params_from_config(config, thread_id, ThreadParamsMode::Remote,
        /*remote_cwd_override*/ None);
    assert_eq!((start.model_provider, resume.model_provider, fork.model_provider),
        (Some("ollama".to_string()), Some("ollama".to_string()), Some("ollama".to_string())));
}

#[tokio::test]
async fn runtime_provider_choice_reaches_remote_thread_creation() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(codex_config::LoaderOverrides::without_managed_config_for_tests())
        .build().await?;
    config.model = Some("local-model".to_string());
    config.service_tier = Some(SERVICE_TIER_DEFAULT_REQUEST_VALUE.to_string());
    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    server.thread_params_mode = ThreadParamsMode::Remote;
    server.managed_new_thread_defaults = Some(NewThreadModelDefaults {
        model: Some("managed-openai-model".to_string()),
        model_reasoning_effort: Some(ReasoningEffort::High),
        service_tier: Some("fast".to_string()),
    });
    let started = server.start_thread_with_session_start_source(
        &config, /*session_start_source*/ None, Some("ollama"),
    ).await?;
    server.shutdown().await?;
    assert_eq!((started.session.model_provider_id, started.session.model, started.session.service_tier),
        ("ollama".to_string(), "local-model".to_string(), None));
    Ok(())
}
