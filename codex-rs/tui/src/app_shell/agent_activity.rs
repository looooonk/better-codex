use codex_app_server_protocol::CollabAgentState;
#[cfg(test)]
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::AgentPath;
use codex_protocol::openai_models::ReasoningEffort;
use std::collections::HashMap;
use std::collections::VecDeque;

mod metadata;
mod text;
mod timeline;

use metadata::agent_order;
use metadata::agent_parent_path;
use metadata::agent_path_depth;
use metadata::fallback_status;
use text::MAX_LATEST_MESSAGE_CHARS;
use text::MAX_MODEL_CHARS;
use text::append_bounded;
use text::bounded_text;
use text::concise_summary;
pub(super) use timeline::AgentChildEvent;
pub(super) use timeline::AgentItemPhase;
pub(super) use timeline::AgentLifecycleStatus;
pub(super) use timeline::AgentTimelineEntry;
pub(super) use timeline::AgentTimelineEvent;
use timeline::child_item_summary;

pub(super) const MAX_TRACKED_AGENTS: usize = 64;
pub(super) const MAX_AGENT_TIMELINE_ENTRIES: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct AgentActivity {
    pub(super) thread_id: String,
    pub(super) path: Option<AgentPath>,
    pub(super) parent_path: Option<AgentPath>,
    pub(super) depth: Option<usize>,
    pub(super) task_summary: Option<String>,
    pub(super) model: Option<String>,
    pub(super) reasoning_effort: Option<ReasoningEffort>,
    pub(super) status: AgentLifecycleStatus,
    pub(super) latest_message: Option<String>,
    pub(super) timeline: VecDeque<AgentTimelineEntry>,
}

impl AgentActivity {
    fn new(thread_id: String) -> Self {
        Self {
            thread_id,
            path: None,
            parent_path: None,
            depth: None,
            task_summary: None,
            model: None,
            reasoning_effort: None,
            status: AgentLifecycleStatus::Unknown,
            latest_message: None,
            timeline: VecDeque::new(),
        }
    }

    pub(super) fn display_name(&self) -> &str {
        self.path
            .as_ref()
            .map(AgentPath::name)
            .unwrap_or(&self.thread_id)
    }

    fn set_path(&mut self, path: &str) {
        let Ok(path) = AgentPath::try_from(path) else {
            return;
        };
        self.depth = Some(agent_path_depth(&path));
        self.parent_path = agent_parent_path(&path);
        self.path = Some(path);
    }

    fn apply_collab_metadata(
        &mut self,
        tool: &CollabAgentTool,
        prompt: Option<&str>,
        model: Option<&str>,
        reasoning_effort: Option<&ReasoningEffort>,
    ) {
        if let Some(prompt) = prompt
            && (matches!(tool, CollabAgentTool::SpawnAgent) || self.task_summary.is_none())
        {
            self.task_summary = concise_summary(prompt);
        }
        if let Some(model) = model {
            self.model = bounded_text(model, MAX_MODEL_CHARS);
        }
        if let Some(reasoning_effort) = reasoning_effort {
            self.reasoning_effort = Some(reasoning_effort.clone());
        }
    }

    fn apply_state(&mut self, state: &CollabAgentState) {
        self.status = state.into();
        if let Some(message) = state.message.as_deref() {
            self.latest_message = bounded_text(message, MAX_LATEST_MESSAGE_CHARS);
        }
    }

    fn mark_active(&mut self) {
        if matches!(
            self.status,
            AgentLifecycleStatus::Unknown
                | AgentLifecycleStatus::PendingInit
                | AgentLifecycleStatus::Running
        ) {
            self.status = AgentLifecycleStatus::Running;
        }
    }

    fn upsert_timeline(
        &mut self,
        item_id: &str,
        event: AgentTimelineEvent,
        detail: Option<String>,
    ) {
        if let Some(entry) = self
            .timeline
            .iter_mut()
            .find(|entry| entry.item_id == item_id)
        {
            entry.event = event;
            entry.detail = detail;
            return;
        }
        self.timeline.push_back(AgentTimelineEntry {
            item_id: item_id.to_string(),
            event,
            detail,
        });
        while self.timeline.len() > MAX_AGENT_TIMELINE_ENTRIES {
            self.timeline.pop_front();
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct AgentActivityCounts {
    pub(super) total: usize,
    pub(super) active: usize,
    pub(super) interrupted: usize,
    pub(super) completed: usize,
    pub(super) failed: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct AgentActivityState {
    agents: HashMap<String, AgentActivity>,
    insertion_order: VecDeque<String>,
    selected_thread_id: Option<String>,
}

impl AgentActivityState {
    pub(super) fn reduce_started(&mut self, item: &ThreadItem) -> bool {
        self.reduce(item, AgentItemPhase::Started)
    }

    pub(super) fn reduce_completed(&mut self, item: &ThreadItem) -> bool {
        self.reduce(item, AgentItemPhase::Completed)
    }

    pub(super) fn is_known_thread(&self, thread_id: &str) -> bool {
        self.agents.contains_key(thread_id)
    }

    pub(super) fn record_child_item(
        &mut self,
        thread_id: &str,
        item: &ThreadItem,
        phase: AgentItemPhase,
    ) -> bool {
        let Some(agent) = self.agents.get_mut(thread_id) else {
            return false;
        };
        agent.mark_active();
        let (event, detail) = child_item_summary(item);
        if detail.is_some() {
            agent.latest_message = detail.clone();
        }
        agent.upsert_timeline(
            item.id(),
            AgentTimelineEvent::ChildItem { event, phase },
            detail,
        );
        true
    }

    pub(super) fn record_child_progress(
        &mut self,
        thread_id: &str,
        item_id: &str,
        progress: AgentChildEvent,
        delta: &str,
    ) -> bool {
        let Some(agent) = self.agents.get_mut(thread_id) else {
            return false;
        };
        agent.mark_active();
        let existing = agent
            .timeline
            .iter()
            .find(|entry| {
                entry.item_id == item_id
                    && entry.event == AgentTimelineEvent::ChildProgress(progress)
            })
            .and_then(|entry| entry.detail.as_deref());
        let detail = append_bounded(existing, delta, MAX_LATEST_MESSAGE_CHARS);
        agent.latest_message = detail.clone();
        agent.upsert_timeline(item_id, AgentTimelineEvent::ChildProgress(progress), detail);
        true
    }

    pub(super) fn record_child_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        status: &TurnStatus,
    ) -> bool {
        let Some(agent) = self.agents.get_mut(thread_id) else {
            return false;
        };
        let status = match status {
            TurnStatus::Completed => AgentLifecycleStatus::Completed,
            TurnStatus::Failed => AgentLifecycleStatus::Errored,
            TurnStatus::Interrupted => AgentLifecycleStatus::Interrupted,
            TurnStatus::InProgress => AgentLifecycleStatus::Running,
        };
        agent.status = status;
        agent.upsert_timeline(
            turn_id,
            AgentTimelineEvent::Lifecycle {
                status,
                retrying: false,
            },
            None,
        );
        true
    }

    pub(super) fn record_child_error(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        message: &str,
        will_retry: bool,
    ) -> bool {
        let Some(agent) = self.agents.get_mut(thread_id) else {
            return false;
        };
        let status = if will_retry {
            AgentLifecycleStatus::Running
        } else {
            AgentLifecycleStatus::Errored
        };
        let detail = bounded_text(message, MAX_LATEST_MESSAGE_CHARS);
        agent.status = status;
        agent.latest_message = detail.clone();
        agent.upsert_timeline(
            turn_id,
            AgentTimelineEvent::Lifecycle {
                status,
                retrying: will_retry,
            },
            detail,
        );
        true
    }

    pub(super) fn agent(&self, thread_id: &str) -> Option<&AgentActivity> {
        self.agents.get(thread_id)
    }

    pub(super) fn ordered_agents(&self) -> Vec<&AgentActivity> {
        let mut agents = self.agents.values().collect::<Vec<_>>();
        agents.sort_by(|left, right| agent_order(left, right));
        agents
    }

    pub(super) fn selected(&self) -> Option<&AgentActivity> {
        self.selected_thread_id
            .as_deref()
            .and_then(|thread_id| self.agents.get(thread_id))
    }

    pub(super) fn selected_thread_id(&self) -> Option<&str> {
        self.selected_thread_id.as_deref()
    }

    pub(super) fn select_thread(&mut self, thread_id: &str) -> bool {
        if !self.agents.contains_key(thread_id) {
            return false;
        }
        self.selected_thread_id = Some(thread_id.to_string());
        true
    }

    pub(super) fn move_selection_up(&mut self) {
        self.move_selection(-1);
    }

    pub(super) fn move_selection_down(&mut self) {
        self.move_selection(1);
    }

    pub(super) fn counts(&self) -> AgentActivityCounts {
        let mut counts = AgentActivityCounts {
            total: self.agents.len(),
            ..Default::default()
        };
        for agent in self.agents.values() {
            match agent.status {
                AgentLifecycleStatus::Unknown => {}
                AgentLifecycleStatus::PendingInit | AgentLifecycleStatus::Running => {
                    counts.active += 1;
                }
                AgentLifecycleStatus::Interrupted => counts.interrupted += 1,
                AgentLifecycleStatus::Completed | AgentLifecycleStatus::Shutdown => {
                    counts.completed += 1;
                }
                AgentLifecycleStatus::Errored | AgentLifecycleStatus::NotFound => {
                    counts.failed += 1;
                }
            }
        }
        counts
    }

    fn reduce(&mut self, item: &ThreadItem, phase: AgentItemPhase) -> bool {
        match item {
            ThreadItem::CollabAgentToolCall {
                id,
                tool,
                status,
                receiver_thread_ids,
                prompt,
                model,
                reasoning_effort,
                agents_states,
                ..
            } => self.reduce_collaboration(
                id,
                tool,
                status,
                receiver_thread_ids,
                prompt.as_deref(),
                model.as_deref(),
                reasoning_effort.as_ref(),
                agents_states,
                phase,
            ),
            ThreadItem::SubAgentActivity {
                id,
                kind,
                agent_thread_id,
                agent_path,
            } => {
                let agent = self.ensure_agent(agent_thread_id);
                agent.set_path(agent_path);
                agent.status = match kind {
                    SubAgentActivityKind::Started | SubAgentActivityKind::Interacted => {
                        AgentLifecycleStatus::Running
                    }
                    SubAgentActivityKind::Interrupted => AgentLifecycleStatus::Interrupted,
                };
                agent.upsert_timeline(id, AgentTimelineEvent::Activity(*kind), None);
                true
            }
            _ => false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_collaboration(
        &mut self,
        item_id: &str,
        tool: &CollabAgentTool,
        status: &CollabAgentToolCallStatus,
        receiver_thread_ids: &[String],
        prompt: Option<&str>,
        model: Option<&str>,
        reasoning_effort: Option<&ReasoningEffort>,
        agents_states: &HashMap<String, CollabAgentState>,
        phase: AgentItemPhase,
    ) -> bool {
        let mut target_ids = receiver_thread_ids.to_vec();
        target_ids.extend(agents_states.keys().cloned());
        target_ids.sort_unstable();
        target_ids.dedup();

        for thread_id in target_ids {
            let agent = self.ensure_agent(&thread_id);
            agent.apply_collab_metadata(tool, prompt, model, reasoning_effort);
            if let Some(state) = agents_states.get(&thread_id) {
                agent.apply_state(state);
            } else if let Some(fallback) = fallback_status(tool, status) {
                agent.status = fallback;
            }
            agent.upsert_timeline(
                item_id,
                AgentTimelineEvent::Collaboration {
                    tool: tool.clone(),
                    phase,
                    status: status.clone(),
                },
                None,
            );
        }
        true
    }

    fn ensure_agent(&mut self, thread_id: &str) -> &mut AgentActivity {
        if !self.agents.contains_key(thread_id) {
            while self.agents.len() >= MAX_TRACKED_AGENTS {
                let Some(oldest) = self.insertion_order.pop_front() else {
                    break;
                };
                self.agents.remove(&oldest);
                if self.selected_thread_id.as_deref() == Some(oldest.as_str()) {
                    self.selected_thread_id = None;
                }
            }
            let thread_id = thread_id.to_string();
            self.insertion_order.push_back(thread_id.clone());
            self.agents
                .insert(thread_id.clone(), AgentActivity::new(thread_id));
            if self.selected_thread_id.is_none() {
                self.selected_thread_id = self.insertion_order.back().cloned();
            }
        }
        self.agents
            .get_mut(thread_id)
            .expect("agent was inserted before access")
    }

    fn move_selection(&mut self, offset: isize) {
        let ordered = self.ordered_agents();
        if ordered.is_empty() {
            self.selected_thread_id = None;
            return;
        }
        let index = self
            .selected_thread_id
            .as_deref()
            .and_then(|selected| ordered.iter().position(|agent| agent.thread_id == selected))
            .unwrap_or_default();
        let index = index.saturating_add_signed(offset).min(ordered.len() - 1);
        let thread_id = ordered[index].thread_id.clone();
        self.selected_thread_id = Some(thread_id);
    }
}

#[cfg(test)]
#[path = "agent_activity_tests.rs"]
mod tests;
