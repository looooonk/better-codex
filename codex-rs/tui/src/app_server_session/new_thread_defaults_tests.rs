use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_config::LoaderOverrides;
use codex_protocol::config_types::TrustLevel;
use codex_protocol::openai_models::ReasoningEffort;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

fn defaults() -> NewThreadModelDefaults {
    NewThreadModelDefaults {
        model: Some("managed-model".to_string()),
        model_reasoning_effort: Some(ReasoningEffort::High),
        service_tier: Some("fast".to_string()),
    }
}

#[tokio::test]
async fn managed_defaults_apply_without_replacing_explicit_choices() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    config.model = Some("configured-model".to_string());
    config.model_reasoning_effort = Some(ReasoningEffort::Low);
    config.service_tier = Some("flex".to_string());
    for (overrides, provider, expected_model, expected_effort, expected_tier) in [
        (
            ConfigOverrides::default(),
            None,
            "managed-model",
            ReasoningEffort::High,
            ServiceTier::Fast.request_value(),
        ),
        (
            ConfigOverrides {
                model: config.model.clone(),
                ..Default::default()
            },
            None,
            "configured-model",
            ReasoningEffort::Low,
            ServiceTier::Fast.request_value(),
        ),
        (
            ConfigOverrides {
                service_tier: Some(config.service_tier.clone()),
                ..Default::default()
            },
            None,
            "managed-model",
            ReasoningEffort::High,
            "flex",
        ),
        (
            ConfigOverrides::default(),
            Some("custom"),
            "configured-model",
            ReasoningEffort::Low,
            "flex",
        ),
        (
            ConfigOverrides {
                model_provider: Some("ollama".to_string()),
                ..Default::default()
            },
            None,
            "configured-model",
            ReasoningEffort::Low,
            "flex",
        ),
        (
            ConfigOverrides {
                model_provider: Some("ollama".to_string()),
                ..Default::default()
            },
            Some("openai"),
            "managed-model",
            ReasoningEffort::High,
            ServiceTier::Fast.request_value(),
        ),
    ] {
        let mut actual = config.clone();
        let mut expected = config.clone();
        expected.model = Some(expected_model.to_string());
        expected.model_reasoning_effort = Some(expected_effort);
        expected.service_tier = Some(expected_tier.to_string());
        apply_managed_new_thread_defaults(&mut actual, Some(&defaults()), &overrides, provider);
        assert_eq!(actual, expected);
    }
    Ok(())
}

#[tokio::test]
async fn custom_provider_keeps_cleared_model_and_tier() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    let builder = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests());
    for (cli_overrides, launch_overrides, provider_override) in [
        (vec![], ConfigOverrides::default(), Some("ollama")),
        (
            vec![],
            ConfigOverrides {
                model_provider: Some("ollama".to_string()),
                ..Default::default()
            },
            None,
        ),
        (
            vec![("model_provider".to_string(), "ollama".into())],
            ConfigOverrides::default(),
            None,
        ),
    ] {
        let mut config = builder.clone().cli_overrides(cli_overrides).build().await?;
        config.model = None;
        config.model_reasoning_effort = None;
        for service_tier in [
            None,
            Some(codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE.to_string()),
        ] {
            config.service_tier = service_tier;
            let mut actual = config.clone();
            apply_managed_new_thread_defaults(
                &mut actual,
                Some(&defaults()),
                &launch_overrides,
                provider_override,
            );
            assert_eq!(actual, config);
        }
    }
    Ok(())
}

#[tokio::test]
async fn explicit_effort_keeps_the_model_pair_and_tier_independent() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    let mut actual = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .cli_overrides(vec![("model_reasoning_effort".to_string(), "low".into())])
        .build()
        .await?;
    let mut expected = actual.clone();
    expected.service_tier = Some(ServiceTier::Fast.request_value().to_string());
    apply_managed_new_thread_defaults(
        &mut actual,
        Some(&defaults()),
        &ConfigOverrides::default(),
        /*provider_override*/ None,
    );
    assert_eq!(actual, expected);
    Ok(())
}

#[tokio::test]
async fn only_the_effective_selected_profile_exempts_managed_defaults() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    let project = tempfile::tempdir()?;
    let profile = home.path().join("work.config.toml");
    std::fs::write(&profile, "model = 'profile-model'\nservice_tier = 'flex'\n")?;
    let builder = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .harness_overrides(ConfigOverrides {
            cwd: Some(project.path().to_path_buf()),
            ..Default::default()
        })
        .loader_overrides(LoaderOverrides {
            user_config_path: Some(AbsolutePathBuf::try_from(profile)?),
            user_config_profile: Some("work".parse()?),
            ..LoaderOverrides::without_managed_config_for_tests()
        });
    let mut actual = builder.clone().build().await?;
    let expected = actual.clone();
    apply_managed_new_thread_defaults(
        &mut actual,
        Some(&defaults()),
        &ConfigOverrides::default(),
        /*provider_override*/ None,
    );
    assert_eq!(actual, expected);

    std::fs::create_dir(project.path().join(".codex"))?;
    std::fs::write(
        project.path().join(".codex/config.toml"),
        "model = 'project-model'\n",
    )?;
    crate::legacy_core::config::set_project_trust_level(
        home.path(),
        project.path(),
        TrustLevel::Trusted,
    )
    .map_err(color_eyre::eyre::Report::msg)?;
    let mut actual = builder.build().await?;
    let mut expected = actual.clone();
    expected.model = Some("managed-model".to_string());
    expected.model_reasoning_effort = Some(ReasoningEffort::High);
    apply_managed_new_thread_defaults(
        &mut actual,
        Some(&defaults()),
        &ConfigOverrides::default(),
        /*provider_override*/ None,
    );
    assert_eq!(actual, expected);
    Ok(())
}

#[tokio::test]
async fn new_thread_sends_managed_model_and_effort_to_app_server() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    server.managed_new_thread_defaults = Some(NewThreadModelDefaults {
        model: Some("gpt-5.4".to_string()),
        model_reasoning_effort: Some(ReasoningEffort::Medium),
        service_tier: None,
    });
    let started = server.start_thread(&config).await?;
    server.shutdown().await?;
    assert_eq!(
        (started.session.model, started.session.reasoning_effort),
        ("gpt-5.4".to_string(), Some(ReasoningEffort::Medium))
    );
    Ok(())
}
