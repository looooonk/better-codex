use super::AgentActivityState;
use super::AgentItemPhase;
use crate::app_server_session::AgentHistorySnapshot;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadStatus;
use codex_protocol::protocol::SubAgentSource;
use std::mem;

impl AgentActivityState {
    pub(in crate::app_shell) fn hydrate_threads(&mut self, threads: Vec<Thread>) {
        for thread in threads {
            self.hydrate_thread(thread);
        }
    }

    pub(in crate::app_shell) fn hydrate_snapshots(&mut self, snapshots: Vec<AgentHistorySnapshot>) {
        for snapshot in snapshots {
            self.hydrate_agent(
                snapshot.thread_id,
                snapshot.agent_path,
                snapshot.status,
                snapshot.turns,
            );
        }
    }

    fn hydrate_thread(&mut self, thread: Thread) {
        let Thread {
            id,
            source,
            status,
            turns,
            ..
        } = thread;
        let path = match source {
            SessionSource::SubAgent(SubAgentSource::ThreadSpawn { agent_path, .. }) => {
                agent_path.map(String::from)
            }
            SessionSource::Cli
            | SessionSource::VsCode
            | SessionSource::Exec
            | SessionSource::AppServer
            | SessionSource::Custom(_)
            | SessionSource::SubAgent(
                SubAgentSource::Review
                | SubAgentSource::Compact
                | SubAgentSource::MemoryConsolidation
                | SubAgentSource::Other(_),
            )
            | SessionSource::Unknown => None,
        };
        self.hydrate_agent(id, path, status, turns);
    }

    fn hydrate_agent(
        &mut self,
        id: String,
        path: Option<String>,
        thread_status: ThreadStatus,
        turns: Vec<codex_app_server_protocol::Turn>,
    ) {
        let original_order = self.insertion_order.clone();
        let preserved_agents = self
            .insertion_order
            .iter()
            .filter_map(|thread_id| {
                let agent = self.agents.get(thread_id)?;
                (thread_id.as_str() != id && (agent.thread_status_known || agent.live_state))
                    .then(|| (thread_id.clone(), agent.clone()))
            })
            .collect::<Vec<_>>();
        let agent = self.ensure_agent(&id);
        if let Some(path) = path {
            agent.set_path(&path);
        }
        let preserve_live_state = agent.live_state;
        let live_state = preserve_live_state
            .then(|| (agent.status, agent.latest_message.clone(), agent.live_state));
        let live_timeline = if preserve_live_state {
            mem::take(&mut agent.timeline)
        } else {
            Default::default()
        };
        self.insertion_order
            .retain(|thread_id| thread_id.as_str() != id);
        self.insertion_order.push_back(id.clone());
        let previous_protected_thread_id = self.hydration_protected_thread_id.replace(id.clone());

        for turn in turns {
            for item in turn.items {
                self.reduce_completed(&item);
                self.record_child_item(&id, &item, AgentItemPhase::Completed);
            }
            if let Some(error) = turn.error {
                self.record_child_error(&id, &turn.id, &error.message, /*will_retry*/ false);
            } else {
                self.record_child_turn(&id, &turn.id, &turn.status);
            }
        }
        for (thread_id, agent) in preserved_agents {
            self.agents.insert(thread_id, agent);
        }
        let replay_order = mem::take(&mut self.insertion_order);
        let mut candidate_order = Vec::new();
        for thread_id in original_order.into_iter().chain(replay_order) {
            if thread_id != id
                && self.agents.contains_key(&thread_id)
                && !candidate_order.contains(&thread_id)
            {
                candidate_order.push(thread_id);
            }
        }
        let mut missing = self
            .agents
            .keys()
            .filter(|thread_id| thread_id.as_str() != id)
            .filter(|thread_id| !candidate_order.contains(thread_id))
            .cloned()
            .collect::<Vec<_>>();
        missing.sort_unstable();
        candidate_order.extend(missing);
        let (high_priority, ordinary): (Vec<_>, Vec<_>) =
            candidate_order.into_iter().partition(|thread_id| {
                self.agents.get(thread_id).is_some_and(|agent| {
                    agent.live_state
                        || (agent.thread_status_known
                            && matches!(agent.status, super::AgentLifecycleStatus::Running))
                })
            });
        self.insertion_order.extend(ordinary);
        self.insertion_order.extend(high_priority);
        if self.agents.contains_key(&id) {
            self.insertion_order.push_back(id.clone());
        }
        self.enforce_agent_limit();
        let agent = self.ensure_agent(&id);
        agent.thread_status_known = true;
        if !agent.live_state {
            match thread_status {
                ThreadStatus::NotLoaded | ThreadStatus::Idle => {
                    agent.status = super::AgentLifecycleStatus::Shutdown;
                }
                ThreadStatus::SystemError => agent.status = super::AgentLifecycleStatus::Errored,
                ThreadStatus::Active { .. } => {
                    agent.status = super::AgentLifecycleStatus::Running;
                }
            }
        }
        for entry in live_timeline {
            agent
                .timeline
                .retain(|historical| historical.item_id != entry.item_id);
            agent.upsert_timeline(&entry.item_id, entry.event, entry.detail);
        }
        if let Some((status, latest_message, live_state)) = live_state {
            agent.status = status;
            if latest_message.is_some() {
                agent.latest_message = latest_message;
            }
            agent.live_state |= live_state;
        }
        self.hydration_protected_thread_id = previous_protected_thread_id;
    }
}

#[cfg(test)]
#[path = "hydration_tests.rs"]
mod tests;
