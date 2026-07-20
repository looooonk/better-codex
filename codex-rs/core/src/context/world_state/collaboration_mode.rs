use super::PreviousSectionState;
use super::WorldStateSection;
use crate::context::CollaborationModeInstructions;
use crate::context::ContextualUserFragment;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use serde::Deserialize;
use serde::Serialize;

/// Collaboration-mode instructions currently visible to the model.
#[derive(Clone, Debug)]
pub(crate) struct CollaborationModeState {
    snapshot: CollaborationModeSnapshot,
    instructions: Option<CollaborationModeInstructions>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct CollaborationModeSnapshot {
    mode: ModeKind,
    instructions_visible: bool,
}

impl CollaborationModeState {
    pub(crate) fn from_collaboration_mode(collaboration_mode: &CollaborationMode) -> Self {
        let instructions =
            CollaborationModeInstructions::from_collaboration_mode(collaboration_mode);
        Self {
            snapshot: CollaborationModeSnapshot {
                mode: collaboration_mode.mode,
                instructions_visible: instructions.is_some(),
            },
            instructions,
        }
    }
}

impl WorldStateSection for CollaborationModeState {
    const ID: &'static str = "collaboration_mode";
    type Snapshot = CollaborationModeSnapshot;

    fn snapshot(&self) -> Self::Snapshot {
        self.snapshot
    }

    fn matches_legacy_fragment(role: &str, text: &str) -> bool {
        role == "developer" && CollaborationModeInstructions::matches_text(text)
    }

    fn has_retained_fragment_matcher() -> bool {
        true
    }

    fn matches_retained_fragment(&self, role: &str, text: &str) -> bool {
        if role != "developer" {
            return false;
        }
        let instructions = self
            .instructions
            .clone()
            .unwrap_or_else(|| CollaborationModeInstructions::new(""));
        text.contains(&instructions.render())
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<Box<dyn ContextualUserFragment>> {
        match previous {
            PreviousSectionState::Known(previous) if previous == &self.snapshot => None,
            PreviousSectionState::Absent if self.instructions.is_none() => None,
            PreviousSectionState::Unknown => None,
            PreviousSectionState::Absent | PreviousSectionState::Known(_) => Some(Box::new(
                self.instructions
                    .clone()
                    .unwrap_or_else(|| CollaborationModeInstructions::new("")),
            )),
        }
    }
}

#[cfg(test)]
#[path = "collaboration_mode_tests.rs"]
mod tests;
