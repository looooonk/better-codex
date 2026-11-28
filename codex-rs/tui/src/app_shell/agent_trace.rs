//! Communication history for the current agent tree, independent of inspector limits.
use super::agent_activity::AgentLifecycleStatus;
use super::agent_log::AgentLogTarget;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadStatus;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

mod history;
pub(super) use history::fill_unindexed_turns;
mod load;
mod projected;
mod records;
pub(super) use load::load;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Body {
    Text(String),
    Encrypted,
    Unavailable,
}

impl Body {
    pub(super) fn from_prompt(text: Option<&str>) -> Self {
        match text {
            Some(text) if is_encrypted_prompt(text) => Self::Encrypted,
            Some(text) if !text.is_empty() => Self::Text(text.into()),
            _ => Self::Unavailable,
        }
    }

    pub(super) fn text(&self) -> &str {
        match self {
            Self::Text(text) => text,
            Self::Encrypted => "Encrypted message",
            Self::Unavailable => "Message unavailable",
        }
    }
}

// Some legacy send records carry a Fernet token in an ordinary prompt field.
pub(super) fn is_encrypted_prompt(text: &str) -> bool {
    text.starts_with("gAAAAA") && !text.contains(char::is_whitespace)
}

#[derive(Debug, Clone)]
pub(super) struct Message {
    pub(super) at: i64,
    pub(super) received_at: Option<i64>,
    pub(super) call_id: Option<String>,
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) body: Arc<Body>,
    pub(super) kind: String,
}

#[derive(Debug, Default)]
struct Records {
    spans: Vec<(i64, Option<i64>)>,
    messages: Vec<Delivery>,
    sent: Vec<Delivery>,
    answers: HashMap<u64, Vec<i64>>,
    status: Option<AgentLifecycleStatus>,
}

#[derive(Debug)]
struct Delivery {
    at: i64,
    id: Option<String>,
    fingerprint: Option<u64>,
    from: String,
    to: String,
    body: Arc<Body>,
    kind: String,
}

#[derive(Debug)]
pub(super) struct Lane {
    pub(super) target: AgentLogTarget,
    spans: Vec<(i64, Option<i64>)>,
    created: i64,
    // File identity is checked on refresh; an unchanged rollout needs no scan.
    file: Option<(PathBuf, u64, SystemTime)>,
    records: Arc<Records>,
}

impl Lane {
    pub(super) fn span_at(&self, at: i64) -> Option<usize> {
        let spans = &self.spans;
        let i = spans
            .partition_point(|(start, _)| *start <= at)
            .checked_sub(1)?;
        spans[i].1.is_none_or(|end| at <= end).then_some(i)
    }

    pub(super) fn running(&self) -> bool {
        self.spans.last().is_some_and(|(_, end)| end.is_none())
    }

    pub(super) fn span_is_open(&self, span: usize) -> bool {
        self.spans[span].1.is_none() && span + 1 == self.spans.len()
    }

    fn new(thread: &Thread, path: String, name: String, records: Arc<Records>) -> Self {
        let status = match thread.status {
            ThreadStatus::Active { .. } => AgentLifecycleStatus::Running,
            ThreadStatus::SystemError => AgentLifecycleStatus::Errored,
            _ => records.status.unwrap_or(AgentLifecycleStatus::Unknown),
        };
        Self {
            target: AgentLogTarget {
                thread_id: thread.id.clone(),
                display_name: name,
                path,
                task_summary: None,
                status,
            },
            spans: records.spans.clone(),
            created: thread.created_at,
            records,
            file: None,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Trace {
    pub(super) lanes: Vec<Lane>,
    pub(super) messages: Vec<Message>,
    pub(super) partial: bool,
}

impl Trace {
    fn assemble(mut lanes: Vec<Lane>, root: &str, mut partial: bool) -> Self {
        lanes.sort_by(|a, b| {
            (a.target.thread_id != root, a.created, &a.target.thread_id).cmp(&(
                b.target.thread_id != root,
                b.created,
                &b.target.thread_id,
            ))
        });
        let mut names = HashMap::<String, usize>::new();
        for lane in &lanes {
            *names.entry(lane.target.display_name.clone()).or_default() += 1;
        }
        for lane in &mut lanes {
            if lane.target.thread_id != root && names[&lane.target.display_name] > 1 {
                lane.target.display_name = lane.target.path.clone();
            }
        }
        let mut by_key = HashMap::new();
        for (index, lane) in lanes.iter().enumerate() {
            by_key.insert(lane.target.thread_id.as_str(), index);
            by_key.insert(lane.target.path.as_str(), index);
        }
        let mut seen = HashSet::new();
        let mut messages = Vec::new();
        // Recipient records can arrive much later. Match the successful send, retaining
        // the recipient's disclosure state and the clock at each end of the exchange.
        let mut pending: HashMap<(usize, usize), Vec<&Delivery>> = HashMap::new();
        for (from, lane) in lanes.iter().enumerate() {
            // Older rollouts can record both a tool completion and a lifecycle item.
            // Prefer the record containing the prompt when they share a call ID.
            let mut ids = HashSet::new();
            let mut sends = lane.records.sent.iter().collect::<Vec<_>>();
            sends.sort_by_key(|send| (send.fingerprint.is_none(), send.at));
            for send in sends {
                if let Some(id) = &send.id
                    && !ids.insert((id, &send.to))
                {
                    continue;
                }
                if let Some(&to) = by_key.get(send.to.as_str()) {
                    pending.entry((from, to)).or_default().push(send);
                } else {
                    partial = true;
                }
            }
        }
        for queue in pending.values_mut() {
            queue.sort_by_key(|send| send.at);
        }
        for lane in &lanes {
            for msg in &lane.records.messages {
                let (Some(&from), Some(&to)) =
                    (by_key.get(msg.from.as_str()), by_key.get(msg.to.as_str()))
                else {
                    partial = true;
                    continue;
                };
                if let Some(id) = &msg.id
                    && !seen.insert((id, from, to))
                {
                    continue;
                }
                let mut at = msg.at;
                let mut call_id = None;
                if msg.kind == "Final answer" {
                    if let Some(end) = msg
                        .fingerprint
                        .and_then(|key| lanes[from].records.answers.get(&key))
                    {
                        // A short final answer may recur on several runs.
                        if let Some(time) = end.iter().min_by_key(|time| time.abs_diff(msg.at)) {
                            at = *time;
                        }
                    }
                } else if let Some(queue) = pending.get_mut(&(from, to)) {
                    let exact = msg.fingerprint.and_then(|key| {
                        queue.iter().position(|send| send.fingerprint == Some(key))
                    });
                    let index = exact.or_else(|| {
                        queue.iter().position(|send| {
                            send.fingerprint.is_none() && send.at <= msg.at.saturating_add(1000)
                        })
                    });
                    if let Some(i) = index {
                        let send = queue.remove(i);
                        at = send.at;
                        call_id = send.id.clone();
                    }
                }
                messages.push(Message {
                    at,
                    received_at: Some(msg.at),
                    call_id,
                    from,
                    to,
                    body: msg.body.clone(),
                    kind: msg.kind.clone(),
                });
            }
        }
        for ((from, to), queue) in pending {
            for send in queue {
                messages.push(Message {
                    at: send.at,
                    received_at: None,
                    call_id: send.id.clone(),
                    from,
                    to,
                    body: send.body.clone(),
                    kind: send.kind.clone(),
                });
            }
        }
        // Use lane order to break ties between simultaneous sends on different routes.
        messages.sort_by_key(|message| (message.at, message.from, message.to));
        for msg in &messages {
            let lane = &mut lanes[msg.to];
            if msg.kind == "Task" && lane.span_at(msg.at).is_none() {
                let i = lane.spans.partition_point(|(start, _)| *start < msg.at);
                if let Some((start, _)) = lane.spans.get_mut(i) {
                    *start = msg.at;
                }
            }
        }
        Self {
            lanes,
            messages,
            partial,
        }
    }
}

pub(super) fn clock(at: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at).map_or_else(String::new, |t| {
        t.with_timezone(&chrono::Local)
            .format("%H:%M:%S")
            .to_string()
    })
}

#[cfg(test)]
#[path = "agent_trace_tests.rs"]
mod tests;
