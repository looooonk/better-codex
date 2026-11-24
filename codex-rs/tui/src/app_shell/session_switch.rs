use super::ShellState;
use super::backend::AppShellBackend;
use super::backend::app_shell_request_id;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use crate::app_server_session::AppServerStartedThread;
use crate::legacy_core::config::Config;
use crate::session_resume::effective_resume_cwd_mode;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::Project;
use codex_app_server_protocol::ProjectReadParams;
use codex_app_server_protocol::ProjectReadResponse;
use codex_config::types::ResumeCwdMode;
use codex_protocol::ThreadId;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::eyre;
use std::path::PathBuf;

impl ShellState {
    pub(super) async fn start_new_session<S>(
        &mut self,
        config: &Config,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        self.new_session_in(config, app_server, None).await
    }

    pub(super) async fn new_session_in<S: AppShellBackend>(
        &mut self,
        config: &Config,
        app_server: &mut S,
        project: Option<&Project>,
    ) -> Result<()> {
        if self.block_session_switch_if_busy() {
            return Ok(());
        }

        let mut session_config = self.current_session_config(config)?;
        if let Some(project) = project
            && let Some(root) = project.roots.first()
        {
            session_config.cwd = root.path.clone();
            session_config.workspace_roots = project.roots.iter().map(|r| r.path.clone()).collect();
        }
        let refresh_catalog = self.next_model_provider.is_some();
        if refresh_catalog {
            session_config.model = self.next_provider_model.clone();
            session_config.model_reasoning_effort = None;
            session_config.service_tier =
                Some(codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE.to_string());
        }

        self.finish_subscription_cleanup().await;
        let request = app_server.start_new_thread_in_background(
            &session_config,
            self.next_model_provider
                .as_deref()
                .or(Some(self.model_provider_id.as_str())),
            project,
        );
        self.start_backend_action(ActionGroup::SessionSwitch, "starting session", async move {
            BackendActionResult::SessionStart {
                result: request.await,
            }
        });
        Ok(())
    }

    pub(super) fn current_session_config(&self, config: &Config) -> Result<Config> {
        let mut session_config = config.clone();
        session_config.tui_auto_recap = self.automatic_recap.enabled;
        session_config.daybreak_enabled = self.daybreak_enabled;
        session_config.model = Some(self.model.clone());
        session_config.model_provider_id = self.model_provider_id.clone();
        session_config.model_reasoning_effort = self.reasoning_effort.clone();
        session_config.service_tier = self.service_tier.clone();
        session_config.personality = self.personality;
        session_config.approvals_reviewer = self.approvals_reviewer;
        session_config.cwd = AbsolutePathBuf::from_absolute_path(PathBuf::from(&self.cwd))
            .wrap_err("current session cwd is not absolute")?;
        session_config.workspace_roots = self.runtime_workspace_roots.clone();
        session_config.workspace_roots_explicit = true;
        session_config
            .permissions
            .approval_policy
            .set(self.approval_policy.to_core())
            .wrap_err("current approval policy is not allowed for a new session")?;
        Ok(session_config)
    }

    pub(super) async fn complete_session_switch<S>(
        &mut self,
        started: AppServerStartedThread,
        app_server: &S,
    ) where
        S: AppShellBackend,
    {
        self.finish_subscription_cleanup().await;
        self.cancel_agent_history().await;
        let provider_changed = self.model_provider_id != started.session.model_provider_id;
        let previous_thread_ids = self.tracked_thread_ids();
        self.projects.error = None;
        self.projects
            .failed_resumes
            .remove(&started.session.thread_id);
        self.replace_started_session(started);
        self.prepare_replaced_session_cleanup(app_server, previous_thread_ids);
        self.start_replaced_session_hydration(app_server);
        self.request_queue_hydration(app_server);
        self.start_session_list_refresh(app_server);
        if provider_changed || self.available_models.is_empty() {
            self.refresh_session_models(app_server);
        }
    }

    pub(super) fn refresh_session_models<S: AppShellBackend>(&mut self, app_server: &S) {
        self.available_models.clear();
        app_server.replace_available_models(Vec::new());
        let thread_id = self.thread_id;
        let request = app_server.workspace_request_in_background(
            thread_id,
            super::workspace_requests::WorkspaceRequest::Account {
                command: super::workspace_requests::AccountCommand::Refresh,
                provider: self.model_provider_id.clone(),
            },
        );
        self.backend_actions.invalidate([ActionGroup::ModelCatalog]);
        self.start_backend_action(ActionGroup::ModelCatalog, "refreshing models", async move {
            BackendActionResult::Workspace {
                thread_id,
                result: request.await,
            }
        });
    }

    pub(super) async fn resume_session<S>(
        &mut self,
        config: &Config,
        app_server: &S,
        thread_id: ThreadId,
    ) where
        S: AppShellBackend,
    {
        let cwd = self.session_list.cwd_for_thread(thread_id).cloned();
        let project = if let Some(id) = self.session_list.project_for_thread(thread_id) {
            if let Some(group) = self.projects.group(id) {
                Some(group.project.clone())
            } else {
                // Recents may finish loading before the project catalog.
                let Some(handle) = app_server.app_server_request_handle() else {
                    self.report_resume_error(
                        thread_id,
                        &eyre!("Could not load the selected chat’s project"),
                    );
                    return;
                };
                let request = ClientRequest::ProjectRead {
                    request_id: app_shell_request_id("resume-project"),
                    params: ProjectReadParams {
                        project_id: id.to_owned(),
                    },
                };
                if self.block_session_switch_if_busy() {
                    return;
                }
                let config = Box::new(config.clone());
                self.start_backend_action(
                    ActionGroup::SessionSwitch,
                    "loading session project",
                    async move {
                        BackendActionResult::SessionProject {
                            thread_id,
                            cwd,
                            config,
                            result: handle
                                .request_typed::<ProjectReadResponse>(request)
                                .await
                                .map(|r| r.project)
                                .map_err(Into::into),
                        }
                    },
                );
                return;
            }
        } else {
            None
        };
        self.resume_workspace_session(config, app_server, thread_id, cwd, project);
    }

    pub(super) fn resume_workspace_session<S: AppShellBackend>(
        &mut self,
        config: &Config,
        app_server: &S,
        thread_id: ThreadId,
        cwd: Option<PathBuf>,
        project: Option<Project>,
    ) {
        if self.block_session_switch_if_busy() {
            return;
        }
        if thread_id == self.thread_id && self.session_unavailable_reason.is_none() {
            self.projects.error = None;
            return;
        }
        let session_config = if let Some(project) = &project
            && let Some(root) = project.roots.first()
        {
            let mut session_config = config.clone();
            session_config.cwd = cwd
                .filter(|dir| project.roots.iter().any(|r| dir.starts_with(&r.path)))
                .and_then(|dir| AbsolutePathBuf::from_absolute_path(dir).ok())
                .unwrap_or_else(|| root.path.clone());
            session_config.workspace_roots = project.roots.iter().map(|r| r.path.clone()).collect();
            session_config
        } else {
            match self.session_switch_config(
                config,
                cwd.as_deref(),
                app_server.uses_remote_workspace(),
            ) {
                Ok(config) => config,
                Err(err) => {
                    self.report_resume_error(thread_id, &err);
                    return;
                }
            }
        };
        let request = app_server.resume_thread_in_background(session_config, thread_id, project);
        self.start_backend_action(ActionGroup::SessionSwitch, "resuming session", async move {
            BackendActionResult::SessionResume {
                thread_id,
                result: request.await,
            }
        });
    }

    pub(super) fn session_switch_config(
        &self,
        config: &Config,
        session_cwd: Option<&std::path::Path>,
        uses_remote_workspace: bool,
    ) -> Result<Config> {
        let mode = effective_resume_cwd_mode(
            config.tui_resume_cwd,
            self.resume_cwd_runtime.explicit_cwd.as_deref(),
        );
        if self.resume_cwd_runtime.uses_remote_workspace_or_environment
            && self.resume_cwd_runtime.explicit_cwd.is_none()
            && matches!(mode, Some(ResumeCwdMode::Current))
        {
            return Err(eyre!(
                "`tui.resume_cwd = \"current\"` requires `--cd` when using a remote workspace"
            ));
        }
        if uses_remote_workspace || mode.is_none() {
            return Ok(config.clone());
        }

        let cwd = match mode {
            Some(ResumeCwdMode::Current)
                if self.resume_cwd_runtime.uses_remote_workspace_or_environment =>
            {
                self.resume_cwd_runtime
                    .explicit_cwd
                    .as_deref()
                    .expect("remote current mode validated above")
            }
            Some(ResumeCwdMode::Current) => self.resume_cwd_runtime.launch_cwd.as_path(),
            Some(ResumeCwdMode::Session) => session_cwd.ok_or_else(|| {
                eyre!("failed to determine the working directory recorded for the selected session")
            })?,
            None => unreachable!("unset modes return before choosing a working directory"),
        };
        let cwd = AbsolutePathBuf::from_absolute_path(cwd)
            .wrap_err("selected working directory is not absolute")?;

        let mut session_config = config.clone();
        session_config.cwd = cwd.clone();
        if !session_config.workspace_roots_explicit {
            session_config.workspace_roots = vec![cwd];
        }
        Ok(session_config)
    }
}
