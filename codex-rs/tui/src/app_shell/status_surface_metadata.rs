use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use super::status_surface_items::StatusLineItem;
use super::status_surface_items::TerminalTitleItem;
use crate::branch_summary::StatusLineGitSummary;
use crate::branch_summary::current_branch_name;
use crate::branch_summary::status_line_git_summary;
use crate::workspace_command::WorkspaceCommand;
use crate::workspace_command::WorkspaceCommandError;
use crate::workspace_command::WorkspaceCommandExecutor;
use crate::workspace_command::WorkspaceCommandOutput;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::CommandExecParams;
use codex_app_server_protocol::CommandExecResponse;
use codex_app_server_protocol::GetWorkspaceMessagesResponse;
use codex_app_server_protocol::RequestId;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub(super) struct StatusMetadata {
    cwd: String,
    pub(super) branch: Option<String>,
    pub(super) git: StatusLineGitSummary,
    pub(super) headline: Option<String>,
    next_refresh: Option<Instant>,
}

impl StatusMetadata {
    pub(super) fn invalidate(&mut self) {
        self.next_refresh = None;
    }
    pub(super) fn for_cwd(&self, cwd: &str) -> Option<&Self> {
        (self.cwd == cwd).then_some(self)
    }
}

impl ShellState {
    pub(super) fn refresh_status_metadata<S: AppShellBackend>(&mut self, app_server: &S) {
        let prefs = &self.status_surfaces.preferences;
        let branch = prefs.status.contains(&StatusLineItem::GitBranch)
            || prefs.title.contains(&TerminalTitleItem::GitBranch);
        let summary = prefs.status.contains(&StatusLineItem::PullRequestNumber)
            || prefs.status.contains(&StatusLineItem::BranchChanges);
        let headline = prefs.status.contains(&StatusLineItem::WorkspaceHeadline);
        if !(branch || summary || headline)
            || self.has_pending_backend_action(ActionGroup::StatusMetadata)
        {
            return;
        }
        let cache = &self.status_surfaces.metadata;
        if cache.cwd == self.cwd && cache.next_refresh.is_some_and(|next| Instant::now() < next) {
            return;
        }
        let Some(handle) = app_server.app_server_request_handle() else {
            return;
        };
        let cwd = self.cwd.clone();
        let thread_id = self.thread_id;
        if self.status_surfaces.metadata.cwd != cwd {
            self.status_surfaces.metadata = StatusMetadata {
                cwd: cwd.clone(),
                ..StatusMetadata::default()
            };
        }
        self.status_surfaces.metadata.next_refresh =
            Some(Instant::now() + crate::workspace_messages::WORKSPACE_HEADLINE_REFRESH_INTERVAL);
        self.backend_actions.start(Some(ActionGroup::StatusMetadata), async move {
            let runner = MetadataRunner { handle: handle.clone(), next_slot: AtomicUsize::new(0) };
            let fetch = async {
                let branch_value = if branch { current_branch_name(&runner, Path::new(&cwd)).await } else { None };
                let git = if summary { status_line_git_summary(&runner, Path::new(&cwd)).await } else { StatusLineGitSummary::default() };
                let headline = if headline {
                    let response = handle.request_typed::<GetWorkspaceMessagesResponse>(ClientRequest::GetWorkspaceMessages { request_id: RequestId::String("tui-status-workspace-headline".into()), params: None }).await;
                    match response.map(crate::workspace_messages::workspace_headline_from_response) {
                        Ok(crate::workspace_messages::WorkspaceHeadlineFetchResult::Available(headline)) => headline.map(|value| value.chars().take(512).collect()),
                        Ok(crate::workspace_messages::WorkspaceHeadlineFetchResult::FeatureDisabled) | Err(_) => None,
                    }
                } else { None };
                StatusMetadata { cwd: cwd.clone(), branch: branch_value, git, headline, next_refresh: Some(Instant::now() + crate::workspace_messages::WORKSPACE_HEADLINE_REFRESH_INTERVAL) }
            };
            let result = tokio::time::timeout(Duration::from_secs(30), fetch).await.ok();
            BackendActionResult::StatusMetadata { thread_id, cwd, result }
        });
    }

    pub(super) fn complete_status_metadata(
        &mut self,
        thread_id: codex_protocol::ThreadId,
        cwd: String,
        result: Option<StatusMetadata>,
    ) {
        if self.thread_id != thread_id || self.cwd != cwd {
            return;
        }
        if let Some(result) = result {
            self.status_surfaces.metadata = result;
        } else {
            self.status_surfaces.metadata.cwd = cwd;
            self.status_surfaces.metadata.next_refresh = Some(
                Instant::now() + crate::workspace_messages::WORKSPACE_HEADLINE_REFRESH_INTERVAL,
            );
        }
    }
}

struct MetadataRunner {
    handle: AppServerRequestHandle,
    next_slot: AtomicUsize,
}

impl WorkspaceCommandExecutor for MetadataRunner {
    fn run(
        &self,
        command: WorkspaceCommand,
    ) -> Pin<
        Box<dyn Future<Output = Result<WorkspaceCommandOutput, WorkspaceCommandError>> + Send + '_>,
    > {
        // Reuse a bounded request namespace when a disconnected server never responds.
        let slot = self.next_slot.fetch_add(1, Ordering::Relaxed) % 8;
        Box::pin(async move {
            let response: CommandExecResponse = self
                .handle
                .request_typed(ClientRequest::OneOffCommandExec {
                    request_id: RequestId::String(format!("tui-status-metadata-{slot}")),
                    params: CommandExecParams {
                        command: command.argv,
                        process_id: None,
                        tty: false,
                        stream_stdin: false,
                        stream_stdout_stderr: false,
                        output_bytes_cap: Some(command.output_bytes_cap.min(128 * 1024)),
                        disable_output_cap: false,
                        disable_timeout: false,
                        timeout_ms: Some(
                            i64::try_from(command.timeout.as_millis())
                                .unwrap_or(5000)
                                .min(5000),
                        ),
                        cwd: command.cwd,
                        env: Some(command.env),
                        size: None,
                        sandbox_policy: None,
                        permission_profile: None,
                    },
                })
                .await
                .map_err(|error| WorkspaceCommandError::new(error.to_string()))?;
            Ok(WorkspaceCommandOutput {
                exit_code: response.exit_code,
                stdout: response.stdout,
                stderr: response.stderr,
            })
        })
    }
}

#[cfg(test)]
#[path = "status_surface_metadata_tests.rs"]
mod tests;
