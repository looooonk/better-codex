//! Resolve the managed destination before daemon selection, login, or telemetry.

use super::*;
use crate::AppServerTarget;
use crate::Cli;
use codex_arg0::Arg0DispatchPaths;
use codex_exec_server::EnvironmentManager;
use codex_exec_server::ExecServerRuntimeOptions;
use codex_utils_absolute_path::AbsolutePathBuf;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare_startup(
    cli: &mut Cli,
    mut source: Config,
    overrides: &mut ConfigOverrides,
    cli_overrides: Vec<(String, toml::Value)>,
    loader_overrides: LoaderOverrides,
    strict_config: bool,
    target: &AppServerTarget,
    arg0_paths: &Arg0DispatchPaths,
    mut bundle: CloudConfigBundleLoader,
    network: &codex_app_server_client::EmbeddedNetworkPolicy,
) -> Result<(PreparedWorktree, CloudConfigBundleLoader)> {
    if target.uses_remote_workspace() {
        return Err(eyre!("`--worktree` is only supported for local sessions"));
    }
    let environment = if crate::should_load_configured_environments(&loader_overrides, target) {
        EnvironmentManager::prepare_from_codex_home(&source.codex_home).await
    } else {
        EnvironmentManager::prepare_from_env().await
    }
    .map_err(std::io::Error::other)?;
    if environment.default_environment_is_remote() {
        return Err(eyre!("`--worktree` is only supported for local sessions"));
    }
    if let Some(id_or_name) = cli.fork_session_id.as_deref() {
        let state =
            crate::init_state_db_for_app_server_target(&source, &AppServerTarget::Embedded).await?;
        let environment = environment.build(
            Some(ExecServerRuntimeOptions::from_optional_paths(
                arg0_paths.codex_self_exe.clone(),
                arg0_paths.codex_linux_sandbox_exe.clone(),
            )?),
            network.bind(source.http_client_factory()),
        )?;
        let mut lookup_config = source.clone();
        lookup_config.analytics_enabled = Some(false);
        let client = crate::start_embedded_app_server(
            arg0_paths.clone(),
            lookup_config,
            cli_overrides.clone(),
            loader_overrides.clone(),
            strict_config,
            bundle.clone(),
            codex_feedback::CodexFeedback::new(),
            /*log_db*/ None,
            state,
            Arc::new(environment),
            network.clone(),
        )
        .await?;
        let mut lookup = crate::app_server_session::AppServerSession::new(
            codex_app_server_client::AppServerClient::InProcess(client),
            crate::app_server_session::ThreadParamsMode::Embedded,
        );
        let resolved = async {
            let target = crate::lookup_session_target_with_app_server(&mut lookup, id_or_name)
                .await?
                .ok_or_else(|| eyre!("Session not found: {id_or_name}"))?;
            lookup
                .thread_read(target.thread_id, /*include_turns*/ false)
                .await
        }
        .await;
        let shutdown = lookup.shutdown().await;
        let thread = resolved?;
        shutdown?;
        cli.fork_session_id = Some(thread.id);
        if cli.cwd.is_none() {
            let fallback = thread.cwd.into_path_buf();
            let latest = if let Some(path) = thread.path {
                tokio::task::spawn_blocking(move || {
                    let reader = codex_rollout::open_rollout_seekable_reader(&path).ok()?;
                    let mut scanner = codex_rollout::ReverseJsonlScanner::new(reader).ok()?;
                    while let Some(outcome) = scanner.scan_next_rollout_line().ok()? {
                        if let codex_rollout::ScanOutcome::Parsed(codex_rollout::RolloutLine {
                            item: codex_rollout::RolloutItem::TurnContext(item),
                            ..
                        }) = outcome
                        {
                            return Some(item.cwd.into_path_buf());
                        }
                    }
                    None
                })
                .await
                .ok()
                .flatten()
            } else {
                None
            };
            overrides.cwd = Some(latest.unwrap_or(fallback));
            let cwd = AbsolutePathBuf::from_absolute_path(overrides.cwd.as_ref().unwrap())?;
            let bootstrap = load_config_toml_with_layer_stack(
                &source.codex_home,
                Some(&cwd),
                cli_overrides.clone(),
                ConfigLoadOptions {
                    loader_overrides: loader_overrides.clone(),
                    strict_config,
                    cloud_config_bundle: CloudConfigBundleLoader::default(),
                },
            )
            .await?;
            bundle = target
                .cloud_config_bundle_loader(|| {
                    crate::bootstrap_auth_config(&source.codex_home, &bootstrap)
                        .map(|auth| network.bind_bootstrap_auth(auth))
                })
                .await?;
            source = ConfigBuilder::default()
                .codex_home(source.codex_home.to_path_buf())
                .cli_overrides(cli_overrides.clone())
                .harness_overrides(overrides.clone())
                .loader_overrides(loader_overrides.clone())
                .cloud_config_bundle(bundle.clone())
                .strict_config(strict_config)
                .build()
                .await?;
        }
    }
    let invocation_cwd = std::env::current_dir()?;
    for path in &mut overrides.additional_writable_roots {
        if path.is_relative() {
            *path = invocation_cwd.join(&*path);
        }
    }
    let mut loader = WorktreeConfigLoader {
        cli_overrides,
        overrides: overrides.clone(),
        loader_overrides,
        cloud_config_bundle: bundle,
        strict_config,
    };
    let mut prepared = loader.prepare(&source, source.cwd.to_path_buf()).await?;
    let destination = AbsolutePathBuf::from_absolute_path(&prepared.checkout.cwd)?;
    let bootstrap = load_config_toml_with_layer_stack(
        &source.codex_home,
        Some(&destination),
        loader.cli_overrides.clone(),
        ConfigLoadOptions {
            loader_overrides: loader.loader_overrides.clone(),
            strict_config,
            cloud_config_bundle: CloudConfigBundleLoader::default(),
        },
    )
    .await?;
    bundle = target
        .cloud_config_bundle_loader(|| {
            crate::bootstrap_auth_config(&source.codex_home, &bootstrap)
                .map(|auth| network.bind_bootstrap_auth(auth))
        })
        .await?;
    loader.cloud_config_bundle = bundle.clone();
    let mut source_loader = loader.clone();
    source_loader.loader_overrides.ignore_project_config = true;
    let source = source_loader
        .load(&source, prepared.checkout.source_cwd.clone())
        .await?;
    if source.active_project.is_untrusted() {
        return Err(
            prepared.retained_error("Cannot create a worktree from an explicitly untrusted source")
        );
    }
    prepared.config = loader.load(&source, prepared.checkout.cwd.clone()).await?;
    if prepared.config.active_project.is_untrusted() {
        return Err(prepared.retained_error("The new worktree is explicitly untrusted"));
    }
    overrides.cwd = Some(prepared.checkout.cwd.clone());
    cli.cwd = Some(prepared.checkout.cwd.clone());
    Ok((prepared, bundle))
}
