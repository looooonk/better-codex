use super::ContextualUserFragment;
use super::bound_developer_configuration_text;
use codex_prompts::END_INSTRUCTIONS;
use codex_protocol::protocol::REALTIME_CONVERSATION_CLOSE_TAG;
use codex_protocol::protocol::REALTIME_CONVERSATION_OPEN_TAG;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RealtimeEndInstructions {
    reason: String,
}

impl RealtimeEndInstructions {
    pub(crate) fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: bound_developer_configuration_text(&reason.into()),
        }
    }
}

impl ContextualUserFragment for RealtimeEndInstructions {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (
            REALTIME_CONVERSATION_OPEN_TAG,
            REALTIME_CONVERSATION_CLOSE_TAG,
        )
    }

    fn body(&self) -> String {
        bound_developer_configuration_text(&format!(
            "\n{}\n\nReason: {}\n",
            END_INSTRUCTIONS.trim(),
            self.reason
        ))
    }
}
