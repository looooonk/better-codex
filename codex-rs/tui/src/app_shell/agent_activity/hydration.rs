use super::AgentActivityState;
use super::AgentItemPhase;
use crate::app_server_session::AgentHistorySnapshot;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
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
            self.hydrate_agent(snapshot.thread_id, snapshot.agent_path, snapshot.turns);
        }
    }

    fn hydrate_thread(&mut self, thread: Thread) {
        let Thread {
            id, source, turns, ..
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
        self.hydrate_agent(id, path, turns);
    }

    fn hydrate_agent(
        &mut self,
        id: String,
        path: Option<String>,
        turns: Vec<codex_app_server_protocol::Turn>,
    ) {
        let preserved_agents = self
            .agents
            .iter()
            .filter(|(thread_id, agent)| {
                thread_id.as_str() != id && (agent.authoritative_state || agent.live_state)
            })
            .map(|(thread_id, agent)| (thread_id.clone(), agent.clone()))
            .collect::<Vec<_>>();
        let agent = self.ensure_agent(&id);
        if let Some(path) = path {
            agent.set_path(&path);
        }
        let preserve_live_state = agent.authoritative_state || agent.live_state;
        let live_state = preserve_live_state.then(|| {
            (
                agent.status,
                agent.latest_message.clone(),
                agent.authoritative_state,
                agent.live_state,
            )
        });
        let live_timeline = if preserve_live_state {
            mem::take(&mut agent.timeline)
        } else {
            Default::default()
        };

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
        let agent = self.ensure_agent(&id);
        for entry in live_timeline {
            agent
                .timeline
                .retain(|historical| historical.item_id != entry.item_id);
            agent.upsert_timeline(&entry.item_id, entry.event, entry.detail);
        }
        if let Some((status, latest_message, authoritative_state, live_state)) = live_state {
            agent.status = status;
            if latest_message.is_some() {
                agent.latest_message = latest_message;
            }
            agent.authoritative_state |= authoritative_state;
            agent.live_state |= live_state;
        }
    }
}

#[cfg(test)]
#[path = "hydration_tests.rs"]
mod tests;
