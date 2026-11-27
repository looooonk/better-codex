use super::transcript_copy::CopyResponseRequest;
use super::workspace_commands::WorkspaceCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SlashCommandId {
    Clear,
    Copy,
    Exit,
    Goal,
    Login,
    Logout,
    Vim,
    Workspace(WorkspaceCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SlashCommandDefinition {
    id: SlashCommandId,
    // The first spelling is the canonical name shown and inserted by the picker.
    pub(super) names: &'static [&'static str],
    description: &'static str,
    accepts_arguments: bool,
}

impl SlashCommandDefinition {
    pub(super) const fn id(self) -> SlashCommandId {
        self.id
    }

    pub(super) const fn name(self) -> &'static str {
        self.names[0]
    }

    pub(super) const fn description(self) -> &'static str {
        self.description
    }

    pub(super) const fn accepts_arguments(self) -> bool {
        self.accepts_arguments
    }
}

pub(super) const SLASH_COMMANDS: &[SlashCommandDefinition] = &[
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Config),
        names: &["/config", "/settings", "/options"],
        description: "Open workspace settings",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Model),
        names: &["/model"],
        description: "Choose a model and reasoning effort",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::New),
        names: &["/new"],
        description: "Start a new session",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Resume),
        names: &["/resume"],
        description: "Find and resume a session",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Find),
        names: &["/find"],
        description: "Search this conversation",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Goal,
        names: &["/goal"],
        description: "Show or update the active goal",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Presets),
        names: &["/presets"],
        description: "Load or manage saved prompts",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Archive),
        names: &["/archive"],
        description: "Archive the current session",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Delete),
        names: &["/delete"],
        description: "Delete this session after confirmation",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Keymap),
        names: &["/keymap"],
        description: "View or change keyboard shortcuts",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::StatusSurface(
            super::status_surfaces::Surface::StatusLine,
        )),
        names: &["/statusline"],
        description: "Customize the status line",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::StatusSurface(
            super::status_surfaces::Surface::Title,
        )),
        names: &["/title"],
        description: "Customize the terminal title",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Pets),
        names: &["/pets", "/pet"],
        description: "Choose a terminal pet or hide it",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Attach),
        names: &["/attach"],
        description: "Attach images to your next message",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Detach),
        names: &["/detach"],
        description: "Remove attached images",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Ide),
        names: &["/ide"],
        description: "Include IDE selection and open tabs",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Init),
        names: &["/init"],
        description: "Create repository instructions in AGENTS.md",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Mention),
        names: &["/mention"],
        description: "Find a file to mention",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Warnings),
        names: &["/warnings"],
        description: "Inspect recent warnings",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Approve),
        names: &["/approve"],
        description: "Inspect denied actions or authorize a retry",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Export),
        names: &["/export"],
        description: "Export this conversation to Markdown",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Recap),
        names: &["/recap"],
        description: "Summarize the conversation and next steps",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Daybreak),
        names: &["/daybreak"],
        description: "Manage Daybreak access",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Worktree),
        names: &["/worktree"],
        description: "Start or fork a session in a worktree",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Usage),
        names: &["/usage"],
        description: "View account usage and resets",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Side),
        names: &["/side", "/btw"],
        description: "Ask a question in a side conversation",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Daemon),
        names: &["/daemon"],
        description: "Inspect or update the local daemon",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Clear,
        names: &["/clear"],
        description: "Clear the visible transcript",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Copy,
        names: &["/copy"],
        description: "Copy a response as Markdown",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Login,
        names: &["/login"],
        description: "Sign in across Codex clients",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Logout,
        names: &["/logout"],
        description: "Sign out across Codex clients",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Vim,
        names: &["/vim"],
        description: "Edit the prompt in Vim or Neovim",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Exit,
        names: &["/exit", "/quit"],
        description: "Exit Better Codex",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Fork),
        names: &["/fork"],
        description: "Fork a saved session",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Permissions),
        names: &["/permissions"],
        description: "Choose the sandbox and approval reviewer",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Status),
        names: &["/status"],
        description: "Show session settings and usage",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Agents),
        names: &["/agent", "/agents", "/subagents"],
        description: "Inspect agents and their activity",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Mcp),
        names: &["/mcp"],
        description: "Manage MCP servers",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Apps),
        names: &["/apps"],
        description: "Manage connected apps and permissions",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Provider),
        names: &["/provider", "/providers"],
        description: "Choose or configure a model provider",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Account),
        names: &["/account", "/accounts"],
        description: "Switch accounts and workspaces",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Plugins),
        names: &["/plugins"],
        description: "Manage plugins and connected apps",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Compact),
        names: &["/compact"],
        description: "Compact the current context",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Review),
        names: &["/review"],
        description: "Review changes, a branch, or a commit",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Plan),
        names: &["/plan"],
        description: "Toggle planning mode",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Skills),
        names: &["/skills"],
        description: "View or enable skills",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Hooks),
        names: &["/hooks"],
        description: "Inspect hooks and manage trust",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Experimental),
        names: &["/experimental"],
        description: "View or change experimental features",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Background),
        names: &["/ps"],
        description: "List background terminals",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Clean),
        names: &["/clean", "/stop"],
        description: "Stop background terminals for this session",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Memory),
        names: &["/memory", "/memories"],
        description: "Read or configure memory",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Pwd),
        names: &["/pwd", "/cwd"],
        description: "Show the current working directory",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Cd),
        names: &["/cd"],
        description: "Change the session working directory",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Rename),
        names: &["/rename"],
        description: "Rename the current session",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Diff),
        names: &["/diff"],
        description: "Review session file changes",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Theme),
        names: &["/theme"],
        description: "Choose the workspace appearance",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Voice),
        names: &["/voice"],
        description: "Manage voice, microphone, and settings",
        accepts_arguments: true,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LocalSlashCommand {
    Clear,
    Copy(CopyResponseRequest),
    Workspace(WorkspaceCommand, String),
    Exit,
    Goal(GoalSlashCommand),
    Login,
    Logout,
    Vim,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GoalSlashCommand {
    Show,
    Set(String),
    Clear,
    Pause,
    Resume,
    Edit,
}

impl LocalSlashCommand {
    pub(super) fn parse(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let command = parts.next()?;
        let args = parts.next().unwrap_or("").trim();
        let definition = SLASH_COMMANDS
            .iter()
            .copied()
            .find(|definition| definition.names.contains(&command))?;
        if !definition.accepts_arguments() && !args.is_empty() {
            return None;
        }

        match definition.id() {
            SlashCommandId::Clear => Some(Self::Clear),
            SlashCommandId::Copy => Some(Self::Copy(CopyResponseRequest::parse_args(args))),
            SlashCommandId::Exit => Some(Self::Exit),
            SlashCommandId::Goal => Some(Self::Goal(GoalSlashCommand::parse(args))),
            SlashCommandId::Login => Some(Self::Login),
            SlashCommandId::Logout => Some(Self::Logout),
            SlashCommandId::Vim => Some(Self::Vim),
            SlashCommandId::Workspace(command) => Some(Self::Workspace(command, args.to_string())),
        }
    }
}

impl GoalSlashCommand {
    fn parse(args: &str) -> Self {
        match args {
            "" => Self::Show,
            "clear" => Self::Clear,
            "pause" => Self::Pause,
            "resume" => Self::Resume,
            "edit" => Self::Edit,
            objective => Self::Set(objective.to_string()),
        }
    }
}

#[cfg(test)]
#[path = "slash_commands_tests.rs"]
mod tests;
