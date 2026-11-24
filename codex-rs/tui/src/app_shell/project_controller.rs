use super::DashboardRoute;
use super::ShellState;
use super::backend::AppShellBackend;
use super::backend::app_shell_request_id;
use super::project_hub::Action;
use super::project_hub::Direction;
use super::project_hub::HubView;
use crate::legacy_core::config::Config;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadMetadataUpdateParams;
use codex_app_server_protocol::ThreadMetadataUpdateResponse;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadSettingsUpdateResponse;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;

impl ShellState {
    pub(super) fn refresh_projects<S: AppShellBackend>(&mut self, backend: &S) {
        if !self.dashboard_visible || self.dashboard_route != DashboardRoute::Sessions {
            return;
        }
        let Some(handle) = backend.app_server_request_handle() else {
            return;
        };
        let mut pages: std::collections::HashMap<_, _> = self
            .projects
            .groups
            .iter()
            .map(|g| (g.project.id.clone(), g.pages.clone()))
            .collect();
        if let Some((id, stack)) = &self.projects.requested_page {
            pages.insert(id.clone(), stack.clone());
        }
        let params = self.session_list.list_params_with_cursor(None);
        if let Some(task) = self.projects.task.take() {
            task.abort();
        }
        self.projects.task = Some(tokio::spawn(super::project_catalog::load(
            handle, params, pages,
        )));
    }

    pub(super) async fn poll_projects(&mut self) -> bool {
        if !self
            .projects
            .task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
        {
            return false;
        }
        let Some(task) = self.projects.task.take() else {
            return false;
        };
        self.projects.requested_page = None;
        match task.await {
            Ok(Ok(groups)) => {
                let selected = self.projects.selected.and_then(|i| {
                    self.projects
                        .rows(self.projects.width.get())
                        .get(i)?
                        .action
                        .clone()
                });
                self.projects.groups = groups;
                self.projects.load_error = None;
                if let Some(action) = selected {
                    self.projects.selected = self
                        .projects
                        .rows(self.projects.width.get())
                        .iter()
                        .position(|row| row.action.as_ref() == Some(&action));
                }
                if let HubView::Details(id) = &self.projects.view
                    && self.projects.group(id).is_none()
                {
                    self.projects.view = HubView::Browse;
                }
            }
            Ok(Err(err)) => {
                self.projects.load_error = Some(format!("Could not load projects · {err}"))
            }
            Err(err) => self.projects.load_error = Some(err.to_string()),
        }
        true
    }

    pub(super) fn project_menu(&mut self, view: HubView) {
        self.projects.view = view;
        self.projects.selected = None;
        self.projects.error = None;
        self.projects.step(Direction::Next);
        self.session_list.focused = true;
        self.dashboard_scroll.set(0);
    }

    pub(super) async fn activate_project<S: AppShellBackend>(
        &mut self,
        action: Action,
        config: &Config,
        backend: &mut S,
    ) {
        self.projects.error = None;
        let result: Result<()> = async {
            match action {
                Action::Back => self.project_menu(HubView::Browse),
                Action::Details(id) => self.project_menu(HubView::Details(id)),
                Action::Move(thread) => self.project_menu(HubView::Move(thread)),
                Action::Toggle(id) => {
                    if !self.projects.expanded.remove(&id) {
                        self.projects.expanded.insert(id);
                    }
                }
                Action::Page(id, forward) => {
                    if self.projects.page(&id, forward) {
                        self.refresh_projects(backend);
                    }
                }
                Action::Open(id, thread) => {
                    let Some(group) = self.projects.group(&id) else {
                        return Ok(());
                    };
                    let cwd = group
                        .chats
                        .iter()
                        .find(|c| c.id == thread)
                        .map(|c| c.cwd.clone());
                    self.resume_workspace_session(
                        config,
                        backend,
                        thread,
                        cwd,
                        Some(group.project.clone()),
                    );
                }
                Action::New(id) => {
                    let project = self.projects.group(&id).map(|g| g.project.clone());
                    if let Some(project) = project {
                        self.new_session_in(config, backend, Some(&project)).await?;
                    }
                }
                Action::Assign(thread, id) => {
                    if thread == self.thread_id && self.block_session_switch_if_busy() {
                        return Ok(());
                    }
                    let project = id
                        .as_ref()
                        .and_then(|id| self.projects.group(id))
                        .map(|g| g.project.clone());
                    let handle = backend.app_server_request_handle().ok_or_else(|| {
                        color_eyre::eyre::eyre!("project actions are unavailable")
                    })?;
                    let _: ThreadMetadataUpdateResponse = handle
                        .request_typed(ClientRequest::ThreadMetadataUpdate {
                            request_id: app_shell_request_id("move-chat"),
                            params: ThreadMetadataUpdateParams {
                                thread_id: thread.to_string(),
                                project_id: Some(id.unwrap_or_default()),
                                git_info: None,
                                daybreak_enabled: None,
                            },
                        })
                        .await?;
                    self.project_menu(HubView::Browse);
                    self.start_session_list_refresh(backend);
                    if thread == self.thread_id
                        && let Some(project) = &project
                        && let Some(first) = project.roots.first()
                    {
                        let _: ThreadSettingsUpdateResponse = handle
                            .request_typed(ClientRequest::ThreadSettingsUpdate {
                                request_id: app_shell_request_id("project-cwd"),
                                params: ThreadSettingsUpdateParams {
                                    thread_id: thread.to_string(),
                                    cwd: Some(first.path.to_path_buf()),
                                    ..Default::default()
                                },
                            })
                            .await
                            .map_err(|e| {
                                color_eyre::eyre::eyre!(
                                    "Project saved. Could not switch working directory · {e}"
                                )
                            })?;
                        self.cwd = first.path.display().to_string();
                        self.runtime_workspace_roots =
                            project.roots.iter().map(|r| r.path.clone()).collect();
                        self.diff_store.set_display_root(first.path.as_path());
                        self.reset_workspace_git_status();
                        self.mark_workspace_status_refresh_due();
                    }
                    self.push_status(project.map_or_else(
                        || "Removed chat from project".into(),
                        |p| format!("Moved chat to {}", p.name),
                    ));
                }
            }
            Ok(())
        }
        .await;
        if let Err(err) = result {
            self.project_action_error(&err);
        }
    }

    pub(super) fn report_resume_error(
        &mut self,
        thread_id: codex_protocol::ThreadId,
        err: &color_eyre::Report,
    ) {
        if format!("{err:#}").contains("active writer") {
            // A catalog refresh will recheck this hint. A click always retries the actual resume.
            self.session_list.mark_external_writer(thread_id);
            for chat in self
                .projects
                .groups
                .iter_mut()
                .flat_map(|group| &mut group.chats)
                .filter(|chat| chat.id == thread_id)
            {
                chat.external_writer = true;
            }
        } else {
            self.projects.failed_resumes.insert(thread_id);
        }
        self.status = "ready".into();
        self.project_action_error(err);
    }

    pub(super) fn project_action_error(&mut self, err: &color_eyre::Report) {
        let detail = format!("{err:#}");
        let message = if detail.contains("active writer") {
            "This chat is open in another Codex session. Close it there and retry.".into()
        } else {
            detail
        };
        if self.dashboard_visible && self.dashboard_route == DashboardRoute::Sessions {
            self.projects.error = Some(message);
            self.dashboard_scroll.set(0);
        }
    }

    pub(super) async fn handle_project_key<S: AppShellBackend>(
        &mut self,
        key: KeyEvent,
        config: &Config,
        backend: &mut S,
    ) -> bool {
        if !super::is_unmodified_action_key(key) {
            return false;
        }
        if self.projects.selected.is_none() {
            match key.code {
                KeyCode::Up | KeyCode::Char('k')
                    if self.session_list.at_top() && !self.projects.groups.is_empty() =>
                {
                    self.projects.step(Direction::Previous);
                    return true;
                }
                KeyCode::Char('p') => {
                    self.project_menu(HubView::Browse);
                    return true;
                }
                KeyCode::Char('m') if !self.session_list.show_archived() => {
                    if let Some(id) = self.session_list.selected_thread_id() {
                        self.project_menu(HubView::Move(id));
                    }
                    return true;
                }
                _ => return false,
            }
        }
        let rows = self.projects.rows(self.projects.width.get().max(1));
        let row = self.projects.selected.and_then(|i| rows.get(i));
        let action = match key.code {
            KeyCode::Enter | KeyCode::Char('r') => row.and_then(|r| r.action.clone()),
            KeyCode::Char('m' | '.') => row.and_then(|r| r.more.clone()),
            KeyCode::Down | KeyCode::Char('j') => {
                self.projects.step(Direction::Next);
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.projects.step(Direction::Previous);
                None
            }
            KeyCode::Esc if !matches!(self.projects.view, HubView::Browse) => Some(Action::Back),
            KeyCode::Char('/' | 'v') => {
                self.projects.selected = None;
                self.projects.reveal.set(true);
                return false;
            }
            KeyCode::Esc | KeyCode::Char('p') => {
                self.projects.selected = None;
                self.projects.reveal.set(true);
                None
            }
            _ => return true,
        };
        if let Some(action) = action {
            self.activate_project(action, config, backend).await;
        }
        true
    }
}
