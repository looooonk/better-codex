use crate::legacy_core::config::Config;
use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::ConfigOverrides;
use crate::legacy_core::config::load_config_toml_with_layer_stack;
use codex_config::CloudConfigBundleLoader;
use codex_config::ConfigLoadOptions;
use codex_config::LoaderOverrides;
use codex_features::Feature;
use codex_protocol::ThreadId;
use codex_worktree::CreateWorktree;
use codex_worktree::ManagedWorktree;
use codex_worktree::WorktreeManager;
use codex_worktree::WorktreeSettings;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::eyre;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

#[derive(Clone)]
pub(crate) struct WorktreeConfigLoader {
    pub(crate) cli_overrides: Vec<(String, toml::Value)>,
    pub(crate) overrides: ConfigOverrides,
    pub(crate) loader_overrides: LoaderOverrides,
    pub(crate) cloud_config_bundle: CloudConfigBundleLoader,
    pub(crate) strict_config: bool,
}

pub(crate) struct WorktreeRuntime {
    pub(crate) loader: WorktreeConfigLoader,
    pub(crate) startup: Option<PreparedWorktree>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorktreeMode {
    New,
    Fork,
}

pub(crate) struct PreparedWorktree {
    pub(crate) config: Config,
    pub(crate) checkout: ManagedWorktree,
    manager: WorktreeManager,
    recovery: Arc<WorktreeRecovery>,
}

struct WorktreeRecovery {
    root: PathBuf,
    reported_or_bound: AtomicBool,
}

impl WorktreeConfigLoader {
    pub(crate) async fn load(&self, source: &Config, cwd: PathBuf) -> Result<Config> {
        let mut overrides = self.overrides.clone();
        overrides.cwd = Some(cwd);
        let mut config = ConfigBuilder::default()
            .codex_home(source.codex_home.to_path_buf())
            .cli_overrides(self.cli_overrides.clone())
            .harness_overrides(overrides)
            .loader_overrides(self.loader_overrides.clone())
            .cloud_config_bundle(self.cloud_config_bundle.clone())
            .strict_config(self.strict_config)
            .build()
            .await?;
        config.application_network_policy = source.application_network_policy.clone();
        Ok(config)
    }

    pub(crate) async fn prepare(
        &self,
        source: &Config,
        source_cwd: PathBuf,
    ) -> Result<PreparedWorktree> {
        let source = self.load(source, source_cwd).await?;
        if !source.features.enabled(Feature::Worktrees) {
            return Err(eyre!(
                "Enable worktrees with --enable worktrees before creating a managed checkout"
            ));
        }
        if source.active_project.is_untrusted() {
            return Err(eyre!(
                "Cannot create a worktree from an explicitly untrusted source"
            ));
        }
        if source.ephemeral {
            return Err(eyre!("Managed worktrees require a saved session"));
        }
        let host = load_config_toml_with_layer_stack(
            source.codex_home.as_path(),
            /*cwd*/ None,
            Vec::new(),
            ConfigLoadOptions {
                loader_overrides: self.loader_overrides.clone(),
                ..Default::default()
            },
        )
        .await?;
        let settings = WorktreeSettings::for_cli(
            source.codex_home.as_path(),
            host.config_toml.desktop.as_ref(),
        )
        .map_err(|error| eyre!(error.to_string()))?;
        let manager = WorktreeManager::new(settings);
        let create_cwd = source.cwd.to_path_buf();
        let (manager, checkout, recovery) = tokio::task::spawn_blocking(move || {
            manager
                .create(&CreateWorktree {
                    source_cwd: create_cwd,
                    base: None,
                })
                .map(|checkout| {
                    let recovery = Arc::new(WorktreeRecovery {
                        root: checkout.root.clone(),
                        reported_or_bound: AtomicBool::new(false),
                    });
                    (manager, checkout, recovery)
                })
        })
        .await?
        .map_err(|error| eyre!(error.to_string()))?;
        let mut prepared = PreparedWorktree {
            config: source,
            checkout,
            manager,
            recovery,
        };
        let source = self
            .load(&prepared.config, prepared.checkout.source_cwd.clone())
            .await
            .map_err(|error| prepared.retained_error(error))?;
        if source.active_project.is_untrusted() {
            return Err(prepared.retained_error("Source trust changed while creating the worktree"));
        }
        let destination = self
            .load(&source, prepared.checkout.cwd.clone())
            .await
            .map_err(|error| prepared.retained_error(error))?;
        if destination.active_project.is_untrusted() {
            return Err(prepared.retained_error("The new worktree is explicitly untrusted"));
        }
        prepared.config = destination;
        Ok(prepared)
    }
}

impl PreparedWorktree {
    pub(crate) fn bind(&self, thread_id: ThreadId) -> Result<()> {
        self.manager
            .bind_thread(&self.checkout.root, &thread_id.to_string())
            .map_err(|error| self.retained_error(error))
            .wrap_err("Could not bind the managed worktree to its new session")?;
        self.recovery
            .reported_or_bound
            .store(true, Ordering::Relaxed);
        Ok(())
    }

    pub(crate) fn retained_error(&self, reason: impl std::fmt::Display) -> color_eyre::Report {
        self.recovery
            .reported_or_bound
            .store(true, Ordering::Relaxed);
        eyre!(
            "{reason}. The checkout was kept at {:?}. Resume there with Better Codex, or remove it with `git worktree remove <checkout-path>` from the source repository after checking that no session uses it. Do not use --force.",
            self.checkout.root
        )
    }
}

impl Drop for WorktreeRecovery {
    fn drop(&mut self) {
        if !self.reported_or_bound.load(Ordering::Relaxed) {
            #[allow(clippy::print_stderr)]
            {
                eprintln!(
                    "Worktree startup did not finish binding a session. The checkout was kept at {:?}. Resume there with Better Codex, or remove it with `git worktree remove <checkout-path>` from the source repository after checking that no session uses it. Do not use --force.",
                    self.root
                );
            }
        }
    }
}

#[path = "managed_worktree_startup.rs"]
mod startup;
pub(crate) use startup::prepare_startup;

#[cfg(test)]
#[path = "managed_worktree_tests.rs"]
mod tests;
