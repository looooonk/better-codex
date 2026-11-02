use super::super::ResumePermissions;
use super::super::ThreadParamsMode;
use super::super::thread_resume_params_from_config;
use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_config::LoaderOverrides;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn global_defaults_do_not_override_saved_model_effort_or_tier() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    std::fs::write(
        home.path().join("config.toml"),
        "model = 'different-model'\nmodel_provider = 'ollama'\nmodel_reasoning_effort = 'high'\nservice_tier = 'flex'\n",
    )?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    assert!(!explicitly_selected(
        &config,
        &[
            "model",
            "model_provider",
            "model_reasoning_effort",
            "service_tier"
        ]
    ));
    let params = thread_resume_params_from_config(
        config,
        ThreadId::new(),
        ThreadParamsMode::Embedded,
        /*remote_cwd_override*/ None,
        ResumeModelSettings::RestoreFromThread,
        ResumePermissions::default(),
    );
    assert_eq!(
        (
            params.model,
            params.model_provider,
            params.service_tier,
            params
                .config
                .as_ref()
                .and_then(|config| config.get("model_reasoning_effort")),
            params.approval_policy,
            params.approvals_reviewer,
            params.sandbox,
            params.permissions,
            params.runtime_workspace_roots
        ),
        (None, None, None, None, None, None, None, None, None)
    );
    Ok(())
}

#[tokio::test]
async fn explicit_model_choices_are_forwarded_on_resume() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .cli_overrides(vec![
            ("model".to_string(), "selected-model".into()),
            ("model_provider".to_string(), "ollama".into()),
            ("model_reasoning_effort".to_string(), "high".into()),
            ("service_tier".to_string(), "flex".into()),
        ])
        .build()
        .await?;
    assert!(explicitly_selected(
        &config,
        &["model", "model_provider", "model_reasoning_effort"]
    ));
    let params = thread_resume_params_from_config(
        config,
        ThreadId::new(),
        ThreadParamsMode::Remote,
        /*remote_cwd_override*/ None,
        ResumeModelSettings::OverrideFromCurrentConfig,
        ResumePermissions::CURRENT_CONFIG,
    );
    assert_eq!(
        (
            params.model,
            params.model_provider,
            params.service_tier,
            params
                .config
                .as_ref()
                .and_then(|config| config.get("model_reasoning_effort"))
                .cloned()
        ),
        (
            Some("selected-model".to_string()),
            Some("ollama".to_string()),
            Some(Some("flex".to_string())),
            Some(serde_json::json!("high"))
        )
    );
    Ok(())
}

#[tokio::test]
async fn resume_keeps_saved_settings_when_current_defaults_change() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    config.model = Some("gpt-5.4".to_string());
    config.model_reasoning_effort = Some(ReasoningEffort::Medium);
    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    let started = server.start_thread(&config).await?;
    server
        .thread_inject_items(
            started.session.thread_id,
            vec![serde_json::from_value(serde_json::json!({
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Saved context."}],
            }))?],
        )
        .await?;
    let metadata = server
        .thread_read(started.session.thread_id, /*include_turns*/ false)
        .await?;
    assert_eq!(
        metadata.history_mode,
        codex_app_server_protocol::ThreadHistoryMode::Paginated
    );
    server.managed_new_thread_defaults = Some(codex_app_server_protocol::NewThreadModelDefaults {
        model: Some("managed-new-thread-model".to_string()),
        model_reasoning_effort: Some(ReasoningEffort::High),
        service_tier: Some("flex".to_string()),
    });
    config.model = Some("different-model".to_string());
    config.model_provider_id = "ollama".to_string();
    config.model_reasoning_effort = Some(ReasoningEffort::High);
    config.service_tier = Some("flex".to_string());
    config
        .permissions
        .approval_policy
        .set(codex_protocol::protocol::AskForApproval::Never)?;
    config.workspace_roots = vec![
        codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
            home.path().join("other-project"),
        )?,
    ];
    let restored = server
        .resume_thread(config, started.session.thread_id)
        .await?;
    server.shutdown().await?;
    assert_eq!(
        (
            restored.session.model,
            restored.session.model_provider_id,
            restored.session.reasoning_effort,
            restored.session.service_tier,
            restored.session.approval_policy,
            restored.session.runtime_workspace_roots
        ),
        (
            started.session.model,
            started.session.model_provider_id,
            started.session.reasoning_effort,
            started.session.service_tier,
            started.session.approval_policy,
            started.session.runtime_workspace_roots
        )
    );
    Ok(())
}

#[tokio::test]
async fn explicit_harness_model_and_provider_remain_resume_overrides() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    let server = crate::start_embedded_app_server_for_picker(&config)
        .await?
        .with_launch_overrides(&ConfigOverrides {
            model: Some("selected-model".to_string()),
            model_provider: Some("ollama".to_string()),
            ..Default::default()
        });
    let settings = server.resume_settings(&config);
    server.shutdown().await?;
    assert_eq!(
        (settings.model, settings.provider),
        (
            ResumeModelSettings::OverrideFromCurrentConfig,
            Some("ollama".to_string())
        )
    );
    Ok(())
}
