use super::AgentActivity;
use super::AgentActivityState;
use super::text::MAX_MODEL_CHARS;
use super::text::bounded_text;
use codex_app_server_protocol::ThreadSettings;
use codex_protocol::openai_models::ReasoningEffort;

impl AgentActivity {
    pub(super) fn update_runtime_metadata(
        &mut self,
        model: Option<&str>,
        effort: Option<&ReasoningEffort>,
    ) {
        self.model = model.and_then(|model| bounded_text(model, MAX_MODEL_CHARS));
        self.reasoning_effort = effort.map(bounded_effort);
    }
}

impl AgentActivityState {
    pub(in crate::app_shell) fn record_child_settings(
        &mut self,
        thread_id: &str,
        settings: &ThreadSettings,
    ) {
        let Some(agent) = self.agents.get_mut(thread_id) else {
            return;
        };
        agent.update_runtime_metadata(Some(&settings.model), settings.effort.as_ref());
        // An in-flight history response must not replace newer settings notifications.
        agent.live_runtime_settings = true;
    }
}

pub(super) fn bounded_effort(effort: &ReasoningEffort) -> ReasoningEffort {
    let mut effort = effort.clone();
    if let ReasoningEffort::Custom(value) = &mut effort {
        *value = bounded_text(value, MAX_MODEL_CHARS).unwrap_or_default();
    }
    effort
}
