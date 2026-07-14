use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::AgentPath;
use codex_protocol::openai_models::ReasoningEffort;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::VecDeque;

pub(super) const MAX_TRACKED_AGENTS: usize = 64;
pub(super) const MAX_AGENT_TIMELINE_ENTRIES: usize = 12;
const MAX_TASK_SUMMARY_CHARS: usize = 240;
const MAX_LATEST_MESSAGE_CHARS: usize = 512;
const MAX_MODEL_CHARS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentItemPhase {
    Started,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentChildEvent {
    Message,
    Reasoning,
    Command,
    Output,
    Activity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentLifecycleStatus {
    Unknown,
    PendingInit,
    Running,
    Interrupted,
    Completed,
    Errored,
    Shutdown,
    NotFound,
}

impl AgentLifecycleStatus {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::PendingInit => "starting",
            Self::Running => "running",
            Self::Interrupted => "interrupted",
            Self::Completed => "completed",
            Self::Errored => "errored",
            Self::Shutdown => "stopped",
            Self::NotFound => "not found",
        }
    }
}

impl From<&CollabAgentState> for AgentLifecycleStatus {
    fn from(state: &CollabAgentState) -> Self {
        match state.status {
            CollabAgentStatus::PendingInit => Self::PendingInit,
            CollabAgentStatus::Running => Self::Running,
            CollabAgentStatus::Interrupted => Self::Interrupted,
            CollabAgentStatus::Completed => Self::Completed,
            CollabAgentStatus::Errored => Self::Errored,
            CollabAgentStatus::Shutdown => Self::Shutdown,
            CollabAgentStatus::NotFound => Self::NotFound,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum AgentTimelineEvent {
    Collaboration {
        tool: CollabAgentTool,
        phase: AgentItemPhase,
        status: CollabAgentToolCallStatus,
    },
    Activity(SubAgentActivityKind),
    ChildItem {
        event: AgentChildEvent,
        phase: AgentItemPhase,
    },
    ChildProgress(AgentChildEvent),
    Lifecycle {
        status: AgentLifecycleStatus,
        retrying: bool,
    },
}

impl AgentTimelineEvent {
    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Collaboration {
                tool,
                phase: AgentItemPhase::Started,
                ..
            } => match tool {
                CollabAgentTool::SpawnAgent => "spawning agent",
                CollabAgentTool::SendInput => "sending input",
                CollabAgentTool::ResumeAgent => "resuming agent",
                CollabAgentTool::Wait => "waiting for agent",
                CollabAgentTool::CloseAgent => "stopping agent",
            },
            Self::Collaboration {
                tool,
                phase: AgentItemPhase::Completed,
                status,
            } => collaboration_result_label(tool, status),
            Self::Activity(SubAgentActivityKind::Started) => "agent started",
            Self::Activity(SubAgentActivityKind::Interacted) => "agent interacted",
            Self::Activity(SubAgentActivityKind::Interrupted) => "agent interrupted",
            Self::ChildItem { event, phase } => child_item_label(*event, *phase),
            Self::ChildProgress(AgentChildEvent::Message) => "message",
            Self::ChildProgress(AgentChildEvent::Reasoning) => "reasoning",
            Self::ChildProgress(AgentChildEvent::Output) => "command output",
            Self::ChildProgress(AgentChildEvent::Command | AgentChildEvent::Activity) => "activity",
            Self::Lifecycle { retrying: true, .. } => "agent retrying",
            Self::Lifecycle {
                status,
                retrying: false,
            } => match status {
                AgentLifecycleStatus::Unknown => "agent status unknown",
                AgentLifecycleStatus::PendingInit => "agent starting",
                AgentLifecycleStatus::Running => "agent running",
                AgentLifecycleStatus::Interrupted => "agent interrupted",
                AgentLifecycleStatus::Completed => "agent completed",
                AgentLifecycleStatus::Errored => "agent failed",
                AgentLifecycleStatus::Shutdown => "agent stopped",
                AgentLifecycleStatus::NotFound => "agent not found",
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct AgentTimelineEntry {
    pub(super) item_id: String,
    pub(super) event: AgentTimelineEvent,
    detail: Option<String>,
}

impl AgentTimelineEntry {
    pub(super) fn label(&self) -> String {
        let label = self.event.label();
        self.detail
            .as_deref()
            .and_then(concise_summary)
            .map_or_else(|| label.to_string(), |detail| format!("{label}: {detail}"))
    }
}

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

fn agent_order(left: &AgentActivity, right: &AgentActivity) -> Ordering {
    match (&left.path, &right.path) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| left.thread_id.cmp(&right.thread_id))
}

fn fallback_status(
    tool: &CollabAgentTool,
    status: &CollabAgentToolCallStatus,
) -> Option<AgentLifecycleStatus> {
    match status {
        CollabAgentToolCallStatus::Failed => Some(AgentLifecycleStatus::Errored),
        CollabAgentToolCallStatus::InProgress => match tool {
            CollabAgentTool::SpawnAgent => Some(AgentLifecycleStatus::PendingInit),
            CollabAgentTool::SendInput | CollabAgentTool::ResumeAgent => {
                Some(AgentLifecycleStatus::Running)
            }
            CollabAgentTool::Wait | CollabAgentTool::CloseAgent => None,
        },
        CollabAgentToolCallStatus::Completed => match tool {
            CollabAgentTool::SpawnAgent
            | CollabAgentTool::SendInput
            | CollabAgentTool::ResumeAgent => Some(AgentLifecycleStatus::Running),
            CollabAgentTool::CloseAgent => Some(AgentLifecycleStatus::Shutdown),
            CollabAgentTool::Wait => None,
        },
    }
}

fn collaboration_result_label(
    tool: &CollabAgentTool,
    status: &CollabAgentToolCallStatus,
) -> &'static str {
    match status {
        CollabAgentToolCallStatus::Failed => "agent operation failed",
        CollabAgentToolCallStatus::InProgress => "agent operation pending",
        CollabAgentToolCallStatus::Completed => match tool {
            CollabAgentTool::SpawnAgent => "agent spawned",
            CollabAgentTool::SendInput => "input delivered",
            CollabAgentTool::ResumeAgent => "agent resumed",
            CollabAgentTool::Wait => "wait complete",
            CollabAgentTool::CloseAgent => "agent stopped",
        },
    }
}

fn child_item_label(event: AgentChildEvent, phase: AgentItemPhase) -> &'static str {
    match (event, phase) {
        (AgentChildEvent::Message, AgentItemPhase::Started) => "writing message",
        (AgentChildEvent::Message, AgentItemPhase::Completed) => "message completed",
        (AgentChildEvent::Reasoning, AgentItemPhase::Started) => "reasoning started",
        (AgentChildEvent::Reasoning, AgentItemPhase::Completed) => "reasoning completed",
        (AgentChildEvent::Command, AgentItemPhase::Started) => "running command",
        (AgentChildEvent::Command, AgentItemPhase::Completed) => "command completed",
        (AgentChildEvent::Output | AgentChildEvent::Activity, AgentItemPhase::Started) => {
            "activity started"
        }
        (AgentChildEvent::Output | AgentChildEvent::Activity, AgentItemPhase::Completed) => {
            "activity completed"
        }
    }
}

fn child_item_summary(item: &ThreadItem) -> (AgentChildEvent, Option<String>) {
    match item {
        ThreadItem::AgentMessage { text, .. } => (
            AgentChildEvent::Message,
            bounded_text(text, MAX_LATEST_MESSAGE_CHARS),
        ),
        ThreadItem::Reasoning { summary, .. } => (
            AgentChildEvent::Reasoning,
            summary
                .last()
                .and_then(|text| bounded_text(text, MAX_LATEST_MESSAGE_CHARS)),
        ),
        ThreadItem::CommandExecution {
            command,
            aggregated_output,
            ..
        } => (
            AgentChildEvent::Command,
            bounded_text(
                aggregated_output.as_deref().unwrap_or(command),
                MAX_LATEST_MESSAGE_CHARS,
            ),
        ),
        _ => (AgentChildEvent::Activity, None),
    }
}

fn agent_parent_path(path: &AgentPath) -> Option<AgentPath> {
    let (parent, _) = path.as_str().rsplit_once('/')?;
    AgentPath::try_from(parent).ok()
}

fn agent_path_depth(path: &AgentPath) -> usize {
    path.as_str().matches('/').count().saturating_sub(1)
}

fn concise_summary(text: &str) -> Option<String> {
    let summary = text.split_whitespace().collect::<Vec<_>>().join(" ");
    bounded_text(&summary, MAX_TASK_SUMMARY_CHARS)
}

fn bounded_text(text: &str, max_chars: usize) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut chars = text.chars();
    let bounded = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_none() {
        Some(bounded)
    } else {
        Some(format!(
            "{}...",
            bounded
                .chars()
                .take(max_chars.saturating_sub(3))
                .collect::<String>()
        ))
    }
}

fn append_bounded(current: Option<&str>, delta: &str, max_chars: usize) -> Option<String> {
    let combined = format!("{}{delta}", current.unwrap_or_default());
    if combined.trim().is_empty() {
        return None;
    }
    let char_count = combined.chars().count();
    if char_count <= max_chars {
        return Some(combined);
    }
    Some(format!(
        "...{}",
        combined
            .chars()
            .skip(char_count.saturating_sub(max_chars.saturating_sub(3)))
            .collect::<String>()
    ))
}

#[cfg(test)]
#[path = "agent_activity_tests.rs"]
mod tests;
