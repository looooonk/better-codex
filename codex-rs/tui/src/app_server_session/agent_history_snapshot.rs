use super::MAX_RESUMED_AGENT_THREAD_CANDIDATES;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_protocol::protocol::SubAgentSource;
use codex_utils_path_uri::LegacyAppPathString;
use std::collections::HashSet;
use std::mem;
use std::path::Path;

const MAX_ITEMS_PER_TURN: usize = 64;
const MAX_ITEMS_SCANNED_PER_TURN: usize = 128;
const MAX_TURNS: usize = 12;
const MAX_ITEM_TEXT_CHARS: usize = 512;
const MAX_ITEM_ID_CHARS: usize = 128;
const MAX_AGENT_PATH_CHARS: usize = 512;
const MAX_COLLAB_AGENTS_PER_ITEM: usize = 64;
const MAX_REFERENCED_AGENT_THREADS: usize = MAX_RESUMED_AGENT_THREAD_CANDIDATES + 1;

#[derive(Debug)]
pub(crate) struct AgentHistorySnapshot {
    pub(crate) thread_id: String,
    pub(crate) agent_path: Option<String>,
    pub(crate) turns: Vec<Turn>,
}

impl AgentHistorySnapshot {
    pub(super) fn metadata(thread: &Thread) -> Self {
        Self {
            thread_id: thread.id.clone(),
            agent_path: agent_path(thread),
            turns: Vec::new(),
        }
    }

    pub(super) fn loaded(mut thread: Thread) -> Self {
        Self {
            thread_id: thread.id.clone(),
            agent_path: agent_path(&thread),
            turns: bounded_turns(mem::take(&mut thread.turns)),
        }
    }
}

pub(super) fn referenced_agent_thread_ids(turns: &[Turn]) -> Vec<String> {
    let mut seen = HashSet::with_capacity(MAX_REFERENCED_AGENT_THREADS);
    let mut thread_ids = Vec::with_capacity(MAX_REFERENCED_AGENT_THREADS);
    // Candidate loading is bounded, so preserve the most recently encountered agents first.
    for item in turns.iter().rev().flat_map(|turn| turn.items.iter().rev()) {
        match item {
            ThreadItem::CollabAgentToolCall {
                receiver_thread_ids,
                agents_states,
                ..
            } => {
                let mut state_thread_ids = agents_states.keys().collect::<Vec<_>>();
                state_thread_ids.sort_unstable();
                for thread_id in receiver_thread_ids.iter().chain(state_thread_ids) {
                    if insert_reference(&mut thread_ids, &mut seen, thread_id) {
                        return thread_ids;
                    }
                }
            }
            ThreadItem::SubAgentActivity {
                agent_thread_id, ..
            } if insert_reference(&mut thread_ids, &mut seen, agent_thread_id) => {
                return thread_ids;
            }
            _ => {}
        }
    }
    thread_ids
}

fn insert_reference<'a>(
    thread_ids: &mut Vec<String>,
    seen: &mut HashSet<&'a str>,
    thread_id: &'a str,
) -> bool {
    if thread_id.len() <= MAX_ITEM_ID_CHARS && seen.insert(thread_id) {
        thread_ids.push(thread_id.to_string());
    }
    thread_ids.len() >= MAX_REFERENCED_AGENT_THREADS
}

fn agent_path(thread: &Thread) -> Option<String> {
    let SessionSource::SubAgent(SubAgentSource::ThreadSpawn { agent_path, .. }) = &thread.source
    else {
        return None;
    };
    agent_path.as_ref().map(|path| {
        let mut path = String::from(path.clone());
        truncate(&mut path, MAX_AGENT_PATH_CHARS);
        path
    })
}

fn bounded_turns(mut turns: Vec<Turn>) -> Vec<Turn> {
    if turns.len() > MAX_TURNS {
        turns = turns.into_iter().rev().take(MAX_TURNS).collect();
        turns.reverse();
    } else {
        turns.shrink_to_fit();
    }
    for turn in &mut turns {
        truncate(&mut turn.id, MAX_ITEM_ID_CHARS);
        if let Some(error) = &mut turn.error {
            truncate(&mut error.message, MAX_ITEM_TEXT_CHARS);
            error.codex_error_info = None;
            error.additional_details = None;
        }
        let mut items = mem::take(&mut turn.items)
            .into_iter()
            .rev()
            .take(MAX_ITEMS_SCANNED_PER_TURN)
            .filter_map(bounded_item)
            .take(MAX_ITEMS_PER_TURN)
            .collect::<Vec<_>>();
        items.reverse();
        items.shrink_to_fit();
        turn.items = items;
    }
    turns
}

fn bounded_item(mut item: ThreadItem) -> Option<ThreadItem> {
    match &mut item {
        ThreadItem::AgentMessage {
            id,
            text,
            memory_citation,
            ..
        } => {
            truncate(id, MAX_ITEM_ID_CHARS);
            truncate(text, MAX_ITEM_TEXT_CHARS);
            *memory_citation = None;
        }
        ThreadItem::Reasoning {
            id,
            summary,
            content,
        } => {
            truncate(id, MAX_ITEM_ID_CHARS);
            let latest = summary.pop().map(|mut text| {
                truncate(&mut text, MAX_ITEM_TEXT_CHARS);
                text
            });
            *summary = latest.into_iter().collect();
            *content = Vec::new();
        }
        ThreadItem::CommandExecution {
            id,
            command,
            cwd,
            process_id,
            command_actions,
            aggregated_output,
            ..
        } => {
            truncate(id, MAX_ITEM_ID_CHARS);
            truncate(command, MAX_ITEM_TEXT_CHARS);
            *cwd = LegacyAppPathString::from_path(Path::new("."));
            if let Some(process_id) = process_id {
                truncate(process_id, MAX_ITEM_ID_CHARS);
            }
            *command_actions = Vec::new();
            if let Some(output) = aggregated_output {
                truncate(output, MAX_ITEM_TEXT_CHARS);
            }
        }
        ThreadItem::CollabAgentToolCall {
            id,
            sender_thread_id,
            receiver_thread_ids,
            prompt,
            model,
            agents_states,
            ..
        } => {
            truncate(id, MAX_ITEM_ID_CHARS);
            truncate(sender_thread_id, MAX_ITEM_ID_CHARS);
            *receiver_thread_ids = mem::take(receiver_thread_ids)
                .into_iter()
                .filter(|thread_id| thread_id.len() <= MAX_ITEM_ID_CHARS)
                .take(MAX_COLLAB_AGENTS_PER_ITEM)
                .map(|mut thread_id| {
                    truncate(&mut thread_id, MAX_ITEM_ID_CHARS);
                    thread_id
                })
                .collect();
            if let Some(prompt) = prompt {
                truncate(prompt, MAX_ITEM_TEXT_CHARS);
            }
            if let Some(model) = model {
                truncate(model, MAX_ITEM_ID_CHARS);
            }
            *agents_states = mem::take(agents_states)
                .into_iter()
                .filter(|(thread_id, _)| thread_id.len() <= MAX_ITEM_ID_CHARS)
                .take(MAX_COLLAB_AGENTS_PER_ITEM)
                .map(|(mut thread_id, mut state)| {
                    truncate(&mut thread_id, MAX_ITEM_ID_CHARS);
                    if let Some(message) = &mut state.message {
                        truncate(message, MAX_ITEM_TEXT_CHARS);
                    }
                    (thread_id, state)
                })
                .collect();
        }
        ThreadItem::SubAgentActivity {
            id,
            agent_thread_id,
            agent_path,
            ..
        } => {
            truncate(id, MAX_ITEM_ID_CHARS);
            if agent_thread_id.len() > MAX_ITEM_ID_CHARS {
                return None;
            }
            truncate(agent_thread_id, MAX_ITEM_ID_CHARS);
            truncate(agent_path, MAX_AGENT_PATH_CHARS);
        }
        ThreadItem::UserMessage { .. }
        | ThreadItem::HookPrompt { .. }
        | ThreadItem::Plan { .. }
        | ThreadItem::FileChange { .. }
        | ThreadItem::McpToolCall { .. }
        | ThreadItem::DynamicToolCall { .. }
        | ThreadItem::WebSearch(_)
        | ThreadItem::ImageView { .. }
        | ThreadItem::Sleep { .. }
        | ThreadItem::ImageGeneration(_)
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. }
        | ThreadItem::ContextCompaction { .. } => return None,
    }
    Some(item)
}

fn truncate(text: &mut String, max_chars: usize) {
    *text = text
        .chars()
        .take(max_chars)
        .collect::<String>()
        .into_boxed_str()
        .into_string();
}
