#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandPaletteAction {
    CopyTranscript,
    ClearTranscript,
    SelectLatestTranscript,
    ScrollTranscriptTop,
    ScrollTranscriptBottom,
    InterruptTurn,
    SwitchModel,
    ChangePermissions,
    ResumeThread,
    ForkThread,
    CompactContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandPaletteEntry {
    pub(super) action: CommandPaletteAction,
    pub(super) title: &'static str,
    pub(super) detail: &'static str,
    pub(super) enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CommandPaletteState {
    selected: usize,
}

impl CommandPaletteState {
    pub(super) fn selected(&self) -> usize {
        self.selected
    }

    pub(super) fn move_up(&mut self, entries: &[CommandPaletteEntry]) {
        if entries.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    pub(super) fn move_down(&mut self, entries: &[CommandPaletteEntry]) {
        let Some(max_index) = entries.len().checked_sub(1) else {
            self.selected = 0;
            return;
        };
        self.selected = self.selected.saturating_add(1).min(max_index);
    }

    pub(super) fn select_last(&mut self, entries: &[CommandPaletteEntry]) {
        self.selected = entries.len().saturating_sub(1);
    }

    pub(super) fn selected_action(
        &self,
        entries: &[CommandPaletteEntry],
    ) -> Option<CommandPaletteAction> {
        entries
            .get(self.selected)
            .filter(|entry| entry.enabled)
            .map(|entry| entry.action)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CommandPaletteContext {
    pub(super) active_turn: bool,
    pub(super) can_copy_transcript: bool,
    pub(super) has_transcript: bool,
}

pub(super) fn command_palette_entries(context: CommandPaletteContext) -> Vec<CommandPaletteEntry> {
    vec![
        CommandPaletteEntry {
            action: CommandPaletteAction::CopyTranscript,
            title: "Copy transcript item",
            detail: "Copy selection or latest assistant message",
            enabled: context.can_copy_transcript,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ClearTranscript,
            title: "Clear visible transcript",
            detail: "Keep the thread, reset the app surface",
            enabled: context.has_transcript,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::SelectLatestTranscript,
            title: "Select latest transcript item",
            detail: "Enter transcript selection mode",
            enabled: context.has_transcript,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ScrollTranscriptTop,
            title: "Jump transcript to top",
            detail: "Show the oldest retained transcript rows",
            enabled: context.has_transcript,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ScrollTranscriptBottom,
            title: "Jump transcript to bottom",
            detail: "Return to the live conversation tail",
            enabled: context.has_transcript,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::InterruptTurn,
            title: "Interrupt active turn",
            detail: "Stop the current agent turn",
            enabled: context.active_turn,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::SwitchModel,
            title: "Switch model",
            detail: "Open model settings",
            enabled: true,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ChangePermissions,
            title: "Change permissions",
            detail: "Open approval policy settings",
            enabled: true,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ResumeThread,
            title: "Resume thread",
            detail: "Session navigation is not wired in the app shell yet",
            enabled: false,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::ForkThread,
            title: "Fork thread",
            detail: "Session navigation is not wired in the app shell yet",
            enabled: false,
        },
        CommandPaletteEntry {
            action: CommandPaletteAction::CompactContext,
            title: "Compact context",
            detail: "Context compaction action is not wired yet",
            enabled: false,
        },
    ]
}
