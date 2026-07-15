use super::ShellState;
use super::ToolBlockStatus;
use super::compact_output_for_transcript;
use crate::workspace_command::WorkspaceCommand;
use crate::workspace_command::WorkspaceCommandExecution;
use std::path::Path;
use std::time::Duration;

const SHELL_COMMAND_TIMEOUT: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ShellCommand {
    command: String,
}

pub(super) struct PendingShellCommand {
    command_text: String,
    execution: WorkspaceCommandExecution,
}

impl ShellCommand {
    pub(super) fn parse(text: &str) -> Option<Self> {
        text.trim().strip_prefix('!').map(|command| Self {
            command: command.trim().to_string(),
        })
    }

    pub(super) fn text(&self) -> &str {
        &self.command
    }

    pub(super) fn is_empty(&self) -> bool {
        self.command.is_empty()
    }

    pub(super) fn workspace_command(&self, cwd: &Path) -> WorkspaceCommand {
        WorkspaceCommand::new(["sh", "-lc", self.command.as_str()])
            .cwd(cwd)
            .timeout(SHELL_COMMAND_TIMEOUT)
    }
}

impl ShellState {
    pub(super) fn start_shell_command(&mut self, command: ShellCommand, prompt: String) {
        if self.pending_shell_command.is_some() {
            self.push_error("another shell command is already running");
            return;
        }
        self.composer.remember_submission(&prompt);
        self.composer.clear();
        if command.is_empty() {
            self.push_error("shell command cannot be empty");
            return;
        }
        let Some(runner) = self.workspace_command_runner.clone() else {
            self.push_error("shell command runner is unavailable");
            return;
        };

        let command_text = command.text().to_string();
        let workspace_command = command.workspace_command(Path::new(&self.cwd));
        self.pending_shell_command = Some(PendingShellCommand {
            command_text,
            execution: WorkspaceCommandExecution::start(runner, workspace_command),
        });
        self.status = "running shell command".to_string();
    }

    pub(super) fn has_pending_shell_command(&self) -> bool {
        self.pending_shell_command.is_some()
    }

    pub(super) fn cancel_shell_command(&mut self) -> bool {
        let Some(pending) = self.pending_shell_command.as_mut() else {
            return false;
        };
        if pending.execution.cancel() {
            self.status = "cancelling shell command".to_string();
            true
        } else {
            false
        }
    }

    pub(super) async fn poll_shell_command(&mut self) -> bool {
        let Some(pending) = self.pending_shell_command.as_ref() else {
            return false;
        };
        if !pending.execution.is_finished() {
            return false;
        }
        let Some(pending) = self.pending_shell_command.take() else {
            return false;
        };
        let command_text = pending.command_text;
        match pending.execution.wait().await {
            Ok(output) => {
                let tool_status = if output.success() {
                    ToolBlockStatus::Success
                } else {
                    ToolBlockStatus::Fail
                };
                self.push_tool_with_status(
                    format!("! {command_text} exit {}", output.exit_code),
                    tool_status,
                );
                let mut combined = output.stdout;
                if !output.stderr.is_empty() {
                    if !combined.is_empty() && !combined.ends_with('\n') {
                        combined.push('\n');
                    }
                    combined.push_str(&output.stderr);
                }
                if !combined.is_empty() {
                    self.push_output_with_status(
                        compact_output_for_transcript(combined),
                        tool_status,
                    );
                }
                self.status = format!("shell exit {}", output.exit_code);
            }
            Err(err) if err.is_cancelled() => {
                self.push_tool_with_status(
                    format!("! {command_text} cancelled"),
                    ToolBlockStatus::Fail,
                );
                self.status = "shell command cancelled".to_string();
            }
            Err(err) => {
                self.push_tool_with_status(format!("! {command_text}"), ToolBlockStatus::Fail);
                self.push_error(format!("shell command failed: {err}"));
                self.status = "shell command failed".to_string();
            }
        }
        self.mark_workspace_status_refresh_due();
        true
    }
}
