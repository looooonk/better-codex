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
    name: &'static str,
    description: &'static str,
    accepts_arguments: bool,
}

impl SlashCommandDefinition {
    pub(super) const fn id(self) -> SlashCommandId {
        self.id
    }

    pub(super) const fn name(self) -> &'static str {
        self.name
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
        id: SlashCommandId::Workspace(WorkspaceCommand::Find),
        name: "/find",
        description: "Search the text retained in this conversation",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Keymap),
        name: "/keymap",
        description: "View, rebind, or disable keyboard shortcuts",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::StatusSurface(
            super::status_surfaces::Surface::StatusLine,
        )),
        name: "/statusline",
        description: "Choose status items, in display order, or turn the extra row off",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::StatusSurface(
            super::status_surfaces::Surface::Title,
        )),
        name: "/title",
        description: "Choose terminal title items, in display order, or turn it off",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Pets),
        name: "/pets",
        description: "Choose a terminal pet or hide it",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Attach),
        name: "/attach",
        description: "Attach local images to your next message",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Detach),
        name: "/detach",
        description: "Remove an attached image by number, or all",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Ide),
        name: "/ide",
        description: "Include the IDE selection and open tabs in new messages",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Diagnostic(
            super::diagnostic_commands::DiagnosticCommand::Init,
        )),
        name: "/init",
        description: "Create repository instructions in AGENTS.md",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Mention),
        name: "/mention",
        description: "Find a file to mention; accepts a search query",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Diagnostic(
            super::diagnostic_commands::DiagnosticCommand::Warnings,
        )),
        name: "/warnings",
        description: "Show retained warnings and diagnostic details",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Diagnostic(
            super::diagnostic_commands::DiagnosticCommand::Config,
        )),
        name: "/debug-config",
        description: "Inspect configuration layers and setting sources",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Diagnostic(
            super::diagnostic_commands::DiagnosticCommand::Rollout,
        )),
        name: "/rollout",
        description: "Show the current session rollout path",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Approve),
        name: "/approve",
        description: "Inspect a denial or approve one retry by review ID",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Export),
        name: "/export",
        description: "Export the conversation to a new local Markdown file",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Recap),
        name: "/recap",
        description: "Summarize recent conversation and next steps",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Daybreak),
        name: "/daybreak",
        description: "Manage broader access for cybersecurity work",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Worktree),
        name: "/worktree",
        description: "Start or fork a session in a managed worktree",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Usage),
        name: "/usage",
        description: "Inspect account usage and available resets",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Side),
        name: "/side",
        description: "Open a side conversation or return to the main conversation",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Side),
        name: "/btw",
        description: "Ask a quick question in a side conversation",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Daemon),
        name: "/daemon",
        description: "Inspect or update the local background server",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Clear,
        name: "/clear",
        description: "Clear the visible transcript",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Copy,
        name: "/copy",
        description: "Copy response Markdown; accepts an optional 1-9 index",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Goal,
        name: "/goal",
        description: "Show or update the active goal",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Login,
        name: "/login",
        description: "Sign in to your OpenAI account",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Logout,
        name: "/logout",
        description: "Sign out of your OpenAI account",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Vim,
        name: "/vim",
        description: "Edit the prompt in Vim or Neovim",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Exit,
        name: "/exit",
        description: "Exit Better Codex",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::New),
        name: "/new",
        description: "Start a new session",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Resume),
        name: "/resume",
        description: "Find and resume a session",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Fork),
        name: "/fork",
        description: "Fork a saved session",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Model),
        name: "/model",
        description: "Choose a model and reasoning effort",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Permissions),
        name: "/permissions",
        description: "Choose approval permissions",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Status),
        name: "/status",
        description: "Show session settings and usage",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Agents),
        name: "/agent",
        description: "Inspect agents and their activity",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Mcp),
        name: "/mcp",
        description: "Manage MCP servers",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Plugins),
        name: "/plugins",
        description: "Manage plugins and connected apps",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Compact),
        name: "/compact",
        description: "Compact the current context",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Review),
        name: "/review",
        description: "Review changes; accepts instructions or --base/--commit",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Plan),
        name: "/plan",
        description: "Switch planning mode: on or off",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Skills),
        name: "/skills",
        description: "List skills or set <name> on/off",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Hooks),
        name: "/hooks",
        description: "Inspect hooks or set on/off/trust for a reviewed hook",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Experimental),
        name: "/experimental",
        description: "List features or set <name> on/off",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Background),
        name: "/ps",
        description: "List background terminals",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Clean),
        name: "/clean",
        description: "Stop background terminals for this session",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Memory),
        name: "/memory",
        description: "Show memory status or set on/off for this session",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Pwd),
        name: "/pwd",
        description: "Show the current working directory",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Cd),
        name: "/cd",
        description: "Change the session working directory",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Rename),
        name: "/rename",
        description: "Rename the current session",
        accepts_arguments: true,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Diff),
        name: "/diff",
        description: "Review session file changes",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Import),
        name: "/import",
        description: "Import Claude Code setup",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Theme),
        name: "/theme",
        description: "Choose the workspace appearance",
        accepts_arguments: false,
    },
    SlashCommandDefinition {
        id: SlashCommandId::Workspace(WorkspaceCommand::Voice),
        name: "/voice",
        description: "Voice conversation: on, off, mute, unmute, or settings",
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
        let command = match parts.next()? {
            "/quit" => "/exit",
            "/pet" => "/pets",
            "/apps" => "/plugins",
            "/agents" | "/subagents" => "/agent",
            "/memories" => "/memory",
            "/stop" => "/clean",
            "/settings" => "/status",
            "/cwd" => "/pwd",
            command => command,
        };
        let args = parts.next().unwrap_or("").trim();
        let definition = SLASH_COMMANDS
            .iter()
            .copied()
            .find(|definition| definition.name() == command)?;
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
