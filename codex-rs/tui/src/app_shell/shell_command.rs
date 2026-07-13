use crate::workspace_command::WorkspaceCommand;
use std::path::Path;
use std::time::Duration;

const SHELL_COMMAND_TIMEOUT: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ShellCommand {
    command: String,
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
