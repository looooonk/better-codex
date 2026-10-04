use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use super::workspace_requests::WorkspaceRequest;
use super::workspace_requests::workspace_request_id;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_protocol::ThreadId;
use color_eyre::Result;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiagnosticCommand {
    Init,
    Warnings,
    Config,
    Rollout,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum DiagnosticRequest {
    Config(String),
    Rollout,
}

#[derive(Default)]
pub(super) struct WarningHistory(VecDeque<String>);

impl ShellState {
    pub(super) fn retain_warning(&mut self, message: String) {
        let mut retained = message.chars().take(16_384).collect::<String>();
        if retained.len() < message.len() {
            retained.push_str("\n[warning truncated]");
        }
        if self.warnings.0.back() != Some(&retained) {
            self.warnings.0.push_back(retained);
            if self.warnings.0.len() > 100 {
                self.warnings.0.pop_front();
            }
        }
        self.push_status(message.lines().next().unwrap_or("warning"));
    }

    pub(super) fn run_diagnostic_command<S: AppShellBackend>(
        &mut self,
        command: DiagnosticCommand,
        app_server: &S,
    ) -> Result<()> {
        let request = match command {
            DiagnosticCommand::Init => {
                self.submit_prompt(
                    app_server,
                    include_str!("../../assets/prompt_for_init_command.md").to_string(),
                );
                return Ok(());
            }
            DiagnosticCommand::Warnings => {
                self.push_system(if self.warnings.0.is_empty() {
                    "No retained warnings".to_string()
                } else {
                    self.warnings
                        .0
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n\n")
                });
                return Ok(());
            }
            DiagnosticCommand::Config => DiagnosticRequest::Config(self.cwd.clone()),
            DiagnosticCommand::Rollout => DiagnosticRequest::Rollout,
        };
        let thread_id = self.thread_id;
        let result = app_server
            .workspace_request_in_background(thread_id, WorkspaceRequest::Diagnostic(request));
        self.start_backend_action(ActionGroup::Workspace, "loading diagnostics", async move {
            BackendActionResult::Workspace {
                thread_id,
                result: result.await,
            }
        });
        Ok(())
    }
}

pub(super) async fn execute(
    client: AppServerRequestHandle,
    thread_id: ThreadId,
    request: DiagnosticRequest,
) -> Result<String> {
    let request_id = workspace_request_id("diagnostics");
    match request {
        DiagnosticRequest::Config(cwd) => {
            let response: ConfigReadResponse = client
                .request_typed(ClientRequest::ConfigRead {
                    request_id,
                    params: ConfigReadParams {
                        include_layers: true,
                        cwd: Some(cwd),
                    },
                })
                .await?;
            let mut lines = vec!["Configuration layers (in precedence order)".to_string()];
            for layer in response.layers.unwrap_or_default() {
                let source = serde_json::to_string(&layer.name)?;
                let state = layer
                    .disabled_reason
                    .map(|reason| format!("disabled: {reason}"))
                    .unwrap_or_else(|| "active".to_string());
                lines.push(format!("{source} [{state}] version {}", layer.version));
            }
            lines.push("Effective setting sources".to_string());
            let mut origins = response.origins.into_iter().collect::<Vec<_>>();
            origins.sort_by(|left, right| left.0.cmp(&right.0));
            for (key, origin) in origins {
                lines.push(format!("{key}: {}", serde_json::to_string(&origin.name)?));
            }
            Ok(lines.join("\n"))
        }
        DiagnosticRequest::Rollout => {
            let response: ThreadReadResponse = client
                .request_typed(ClientRequest::ThreadRead {
                    request_id,
                    params: ThreadReadParams {
                        thread_id: thread_id.to_string(),
                        include_turns: false,
                    },
                })
                .await?;
            Ok(response
                .thread
                .path
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "This session has no local rollout file".to_string()))
        }
    }
}

#[cfg(test)]
#[path = "diagnostic_commands_tests.rs"]
mod tests;
