use super::ConfiguredPermissions;
use super::ConfiguredSandbox;
use super::PermissionMode;
use crate::app_server_session::AppServerSession;
use crate::app_shell::DashboardRoute;
use crate::app_shell::ShellState;
use crate::app_shell::backend::app_shell_request_id;
use crate::app_shell::composer_toolbar::ComposerControl;
use crate::app_shell::selector::SelectorState;
use crate::legacy_core::config::Config;
use crate::managed_worktree::WorktreeConfigLoader;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::SandboxMode;
use codex_protocol::protocol::SandboxPolicy;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use std::collections::HashMap;
use tokio::task::JoinHandle;

/// Refresh on entry to a permission picker or a different workspace. Thread
/// settings notifications change the active choice, never the saved default.
#[derive(Default)]
pub(in crate::app_shell) struct WorkspacePermissionDefaults {
    cwd: String,
    menu_open: bool,
    settings_open: bool,
    remote_defaults: HashMap<String, Option<ConfiguredPermissions>>,
    pending: Option<JoinHandle<Result<PickerDefaults>>>,
}

struct PickerDefaults {
    modes: Vec<PermissionMode>,
    configured: Option<ConfiguredPermissions>,
}

impl PickerDefaults {
    fn from_config(config: &Config) -> Self {
        Self {
            modes: PermissionMode::ALL
                .into_iter()
                .filter(|mode| mode.available(config))
                .collect(),
            configured: ConfiguredPermissions::from_config(config),
        }
    }
}

impl ShellState {
    pub(in crate::app_shell) fn configure_permission_picker(&mut self, config: &Config) {
        self.apply_permission_defaults(PickerDefaults::from_config(config));
    }

    fn apply_permission_defaults(&mut self, defaults: PickerDefaults) {
        self.permission_modes = defaults.modes;
        self.set_configured_permissions(defaults.configured);
        if self
            .selector
            .as_ref()
            .and_then(SelectorState::composer_control)
            == Some(ComposerControl::Permissions)
        {
            let updated = SelectorState::permissions(self);
            if let Some(selector) = &mut self.selector {
                selector.refresh_permission_options(updated);
            }
        }
    }
}

impl WorkspacePermissionDefaults {
    pub(in crate::app_shell) async fn refresh(
        &mut self,
        shell: &mut ShellState,
        base: &Config,
        loader: &WorktreeConfigLoader,
        server: &AppServerSession,
    ) -> bool {
        let menu_open = shell
            .selector
            .as_ref()
            .and_then(SelectorState::composer_control)
            == Some(ComposerControl::Permissions);
        let settings_open =
            shell.dashboard_visible && shell.dashboard_route == DashboardRoute::Settings;
        let workspace_changed = self.cwd != shell.cwd;
        let entered = (menu_open && !self.menu_open) || (settings_open && !self.settings_open);
        self.menu_open = menu_open;
        self.settings_open = settings_open;
        if workspace_changed || entered {
            if let Some(task) = self.pending.take() {
                task.abort();
            }
            self.cwd.clone_from(&shell.cwd);
            let cwd = shell.cwd.clone();
            let remote = server.uses_remote_workspace()
                || shell
                    .resume_cwd_runtime
                    .uses_remote_workspace_or_environment;
            if remote {
                let initial = self
                    .remote_defaults
                    .entry(cwd.clone())
                    .or_insert_with(|| shell.active_permissions())
                    .clone();
                let mut defaults = PickerDefaults::from_config(base);
                if workspace_changed {
                    shell.set_configured_permissions(initial.clone());
                    shell.permission_modes.clone_from(&defaults.modes);
                }
                let handle = server.request_handle();
                self.pending = Some(tokio::spawn(async move {
                    defaults.configured =
                        Some(remote_permissions(handle, &cwd, initial.as_ref()).await?);
                    Ok(defaults)
                }));
            } else {
                if workspace_changed {
                    shell.set_configured_permissions(None);
                }
                let mut loader = loader.clone();
                // Retain the selected config file and project layers. One-off
                // permission flags (e.g. --yolo) belong to the active thread.
                loader.overrides = Default::default();
                loader.cli_overrides.retain(|(key, _)| {
                    !matches!(
                        key.split('.').next(),
                        Some(
                            "approval_policy"
                                | "approvals_reviewer"
                                | "sandbox_mode"
                                | "sandbox_workspace_write"
                                | "default_permissions"
                                | "permissions"
                        )
                    )
                });
                let base = base.clone();
                self.pending = Some(tokio::spawn(async move {
                    let config = loader.load(&base, cwd.into()).await?;
                    Ok(PickerDefaults::from_config(&config))
                }));
            }
        }
        if self
            .pending
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
            && let Some(task) = self.pending.take()
        {
            match task
                .await
                .map_err(color_eyre::Report::from)
                .and_then(|result| result)
            {
                Ok(defaults) => {
                    shell.apply_permission_defaults(defaults);
                    return true;
                }
                Err(error) => tracing::warn!(%error, "could not read configured permissions"),
            }
        }
        false
    }
}

// config/read merges the server's trusted project, user and managed layers.
// For settings the server leaves implicit, use the workspace's initial values.
async fn remote_permissions(
    handle: AppServerRequestHandle,
    cwd: &str,
    initial: Option<&ConfiguredPermissions>,
) -> Result<ConfiguredPermissions> {
    let response: ConfigReadResponse = handle
        .request_typed(ClientRequest::ConfigRead {
            request_id: app_shell_request_id("permission-defaults"),
            params: ConfigReadParams {
                include_layers: true,
                cwd: Some(cwd.to_owned()),
            },
        })
        .await?;
    let config = response.config;
    let policy = config
        .approval_policy
        .or_else(|| initial.map(|setup| setup.approval))
        .unwrap_or(codex_app_server_protocol::AskForApproval::OnRequest);
    let reviewer = config
        .approvals_reviewer
        .map(codex_app_server_protocol::ApprovalsReviewer::to_core)
        .or_else(|| initial.map(|setup| setup.reviewer))
        .unwrap_or_default();
    let named = config
        .additional
        .get("default_permissions")
        .and_then(|value| value.as_str());
    // An explicit sandbox_mode in a higher layer supersedes an inherited
    // default_permissions. Within a layer the named profile takes precedence.
    let named = named.filter(|_| {
        config.sandbox_mode.is_none()
            || response
                .layers
                .iter()
                .flatten()
                .filter(|layer| layer.disabled_reason.is_none())
                .find_map(|layer| {
                    if layer.config.get("default_permissions").is_some() {
                        Some(true)
                    } else if layer.config.get("sandbox_mode").is_some() {
                        Some(false)
                    } else {
                        None
                    }
                })
                .unwrap_or(true)
    });
    let sandbox = if let Some(id) = named {
        ConfiguredSandbox::Named(id.to_owned())
    } else {
        let implicit_workspace = initial.and_then(|setup| setup.sandbox.profile_id())
            == Some(codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_WORKSPACE);
        let mode = config.sandbox_mode.or_else(|| {
            (implicit_workspace && config.sandbox_workspace_write.is_some())
                .then_some(SandboxMode::WorkspaceWrite)
        });
        match mode {
            Some(SandboxMode::DangerFullAccess) => {
                ConfiguredSandbox::Legacy(SandboxPolicy::DangerFullAccess)
            }
            Some(SandboxMode::ReadOnly) => {
                ConfiguredSandbox::Legacy(SandboxPolicy::new_read_only_policy())
            }
            Some(SandboxMode::WorkspaceWrite) => {
                let settings = config.sandbox_workspace_write.unwrap_or_default();
                ConfiguredSandbox::Legacy(SandboxPolicy::WorkspaceWrite {
                    writable_roots: settings
                        .writable_roots
                        .into_iter()
                        .map(codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path)
                        .collect::<std::io::Result<_>>()?,
                    network_access: settings.network_access,
                    exclude_tmpdir_env_var: settings.exclude_tmpdir_env_var,
                    exclude_slash_tmp: settings.exclude_slash_tmp,
                })
            }
            None => initial
                .ok_or_else(|| eyre!("server did not supply a default sandbox"))?
                .sandbox
                .clone(),
        }
    };
    Ok(ConfiguredPermissions {
        approval: policy,
        reviewer,
        sandbox,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_core::config::ConfigBuilder;
    use codex_app_server_protocol::AskForApproval;
    use codex_config::LoaderOverrides;
    use codex_protocol::config_types::ApprovalsReviewer;

    #[tokio::test]
    async fn remote_config_read_uses_saved_permissions_after_a_thread_override() -> Result<()> {
        let home = tempfile::tempdir()?;
        tokio::fs::write(home.path().join("config.toml"), "approval_policy = 'on-request'\napprovals_reviewer = 'user'\nsandbox_mode = 'danger-full-access'\n").await?;
        let config = ConfigBuilder::default()
            .codex_home(home.path().into())
            .fallback_cwd(Some(home.path().into()))
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .build()
            .await?;
        let server = crate::start_embedded_app_server_for_picker(&config).await?;
        let overridden = ConfiguredPermissions {
            approval: AskForApproval::OnRequest,
            reviewer: ApprovalsReviewer::AutoReview,
            sandbox: ConfiguredSandbox::Named(":workspace".into()),
        };
        let defaults = remote_permissions(
            server.request_handle(),
            &home.path().to_string_lossy(),
            Some(&overridden),
        )
        .await?;
        assert_eq!(Some(defaults), ConfiguredPermissions::from_config(&config));
        server.shutdown().await?;
        Ok(())
    }
}
