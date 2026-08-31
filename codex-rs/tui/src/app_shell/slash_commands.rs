use super::transcript_copy::CopyResponseRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SlashCommandId {
    Clear,
    Copy,
    Exit,
    Goal,
    Login,
    Logout,
    Vim,
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

pub(super) const SLASH_COMMANDS: [SlashCommandDefinition; 7] = [
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
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LocalSlashCommand {
    Clear,
    Copy(CopyResponseRequest),
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
            .into_iter()
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
