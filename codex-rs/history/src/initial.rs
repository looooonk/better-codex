use std::path::PathBuf;
use std::sync::Arc;

use codex_protocol::ThreadId;
use codex_protocol::capabilities::SelectedCapabilityRoot;
use codex_protocol::config_types::MultiAgentMode;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::models::BaseInstructions;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::SessionMeta;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::ThreadSource;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, TS)]
pub struct ResumedHistory {
    pub conversation_id: ThreadId,
    pub history: Arc<Vec<RolloutItem>>,
    pub rollout_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, TS)]
pub enum InitialHistory {
    New,
    Cleared,
    Resumed(ResumedHistory),
    Forked(Vec<RolloutItem>),
}

impl InitialHistory {
    pub fn scan_rollout_items(&self, mut predicate: impl FnMut(&RolloutItem) -> bool) -> bool {
        match self {
            Self::New | Self::Cleared => false,
            Self::Resumed(resumed) => resumed.history.iter().any(&mut predicate),
            Self::Forked(items) => items.iter().any(predicate),
        }
    }

    pub fn forked_from_id(&self) -> Option<ThreadId> {
        match self {
            Self::New | Self::Cleared => None,
            Self::Resumed(resumed) => resumed.history.iter().find_map(|item| match item {
                RolloutItem::SessionMeta(meta_line) => meta_line.meta.forked_from_id,
                _ => None,
            }),
            Self::Forked(items) => items.iter().find_map(|item| match item {
                RolloutItem::SessionMeta(meta_line) => Some(meta_line.meta.id),
                _ => None,
            }),
        }
    }

    pub fn session_cwd(&self) -> Option<PathBuf> {
        match self {
            Self::New | Self::Cleared => None,
            Self::Resumed(resumed) => session_cwd_from_items(&resumed.history),
            Self::Forked(items) => session_cwd_from_items(items),
        }
    }

    pub fn get_rollout_items(&self) -> &[RolloutItem] {
        match self {
            Self::New | Self::Cleared => &[],
            Self::Resumed(resumed) => &resumed.history,
            Self::Forked(items) => items,
        }
    }

    pub fn get_event_msgs(&self) -> Option<Vec<EventMsg>> {
        match self {
            Self::New | Self::Cleared => None,
            Self::Resumed(resumed) => Some(
                resumed
                    .history
                    .iter()
                    .filter_map(|item| match item {
                        RolloutItem::EventMsg(event) => Some(event.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
            Self::Forked(items) => Some(
                items
                    .iter()
                    .filter_map(|item| match item {
                        RolloutItem::EventMsg(event) => Some(event.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
        }
    }

    pub fn get_base_instructions(&self) -> Option<BaseInstructions> {
        match self {
            Self::New | Self::Cleared => None,
            Self::Resumed(resumed) => resumed.history.iter().find_map(|item| match item {
                RolloutItem::SessionMeta(meta_line) => meta_line.meta.base_instructions.clone(),
                _ => None,
            }),
            Self::Forked(items) => items.iter().find_map(|item| match item {
                RolloutItem::SessionMeta(meta_line) => meta_line.meta.base_instructions.clone(),
                _ => None,
            }),
        }
    }

    pub fn get_dynamic_tools(&self) -> Option<Vec<DynamicToolSpec>> {
        match self {
            Self::New | Self::Cleared => None,
            Self::Resumed(resumed) => resumed.history.iter().find_map(|item| match item {
                RolloutItem::SessionMeta(meta_line) => meta_line.meta.dynamic_tools.clone(),
                _ => None,
            }),
            Self::Forked(items) => items.iter().find_map(|item| match item {
                RolloutItem::SessionMeta(meta_line) => meta_line.meta.dynamic_tools.clone(),
                _ => None,
            }),
        }
    }

    pub fn get_selected_capability_roots(&self) -> Vec<SelectedCapabilityRoot> {
        self.get_session_meta()
            .map(|meta| meta.selected_capability_roots.clone())
            .unwrap_or_default()
    }

    pub fn get_multi_agent_version(&self) -> Option<MultiAgentVersion> {
        match self {
            Self::New | Self::Cleared => None,
            Self::Resumed(resumed) => {
                multi_agent_version_from_items(&resumed.history, Some(resumed.conversation_id))
            }
            Self::Forked(items) => {
                multi_agent_version_from_items(items, /*thread_id*/ None)
            }
        }
    }

    pub fn get_history_mode(&self, default_history_mode: ThreadHistoryMode) -> ThreadHistoryMode {
        match self {
            Self::New | Self::Cleared => default_history_mode,
            // Forks copy their source rollout items as-is, so they keep the source format.
            Self::Resumed(_) | Self::Forked(_) => self
                .get_session_meta()
                .map(|meta| meta.history_mode)
                .unwrap_or(default_history_mode),
        }
    }

    pub fn get_latest_effective_multi_agent_mode(&self) -> Option<MultiAgentMode> {
        let items = match self {
            Self::New | Self::Cleared => return None,
            Self::Resumed(resumed) => &resumed.history,
            Self::Forked(items) => items,
        };
        items
            .iter()
            .rev()
            .find_map(|item| match item {
                RolloutItem::TurnContext(turn_context) => Some(turn_context),
                RolloutItem::SessionMeta(_)
                | RolloutItem::ResponseItem(_)
                | RolloutItem::InterAgentCommunication(_)
                | RolloutItem::InterAgentCommunicationMetadata { .. }
                | RolloutItem::Compacted(_)
                | RolloutItem::WorldState(_)
                | RolloutItem::EventMsg(_) => None,
            })
            .and_then(|turn_context| turn_context.multi_agent_mode.clone())
    }

    pub fn get_resumed_session_sources(&self) -> Option<(SessionSource, Option<ThreadSource>)> {
        let meta = self.get_resumed_session_meta()?;
        Some((meta.source.clone(), meta.thread_source.clone()))
    }

    pub fn get_resumed_thread_source(&self) -> Option<ThreadSource> {
        self.get_resumed_session_meta()
            .and_then(|meta| meta.thread_source.clone())
    }

    pub fn get_session_originator(&self) -> Option<String> {
        self.get_session_meta()
            .map(|meta| meta.originator.clone())
            .filter(|originator| !originator.is_empty())
    }

    pub fn get_resumed_parent_thread_id(&self) -> Option<ThreadId> {
        self.get_resumed_session_meta()
            .and_then(|meta| meta.parent_thread_id)
    }

    fn get_session_meta(&self) -> Option<&SessionMeta> {
        match self {
            Self::New | Self::Cleared => None,
            Self::Resumed(resumed) => resumed.history.iter().find_map(|item| match item {
                RolloutItem::SessionMeta(meta_line) => Some(&meta_line.meta),
                _ => None,
            }),
            Self::Forked(items) => items.iter().find_map(|item| match item {
                RolloutItem::SessionMeta(meta_line) => Some(&meta_line.meta),
                _ => None,
            }),
        }
    }

    fn get_resumed_session_meta(&self) -> Option<&SessionMeta> {
        match self {
            Self::New | Self::Cleared | Self::Forked(_) => None,
            Self::Resumed(resumed) => resumed.history.iter().find_map(|item| match item {
                RolloutItem::SessionMeta(meta_line) => Some(&meta_line.meta),
                _ => None,
            }),
        }
    }
}

fn session_cwd_from_items(items: &[RolloutItem]) -> Option<PathBuf> {
    items.iter().find_map(|item| match item {
        RolloutItem::SessionMeta(meta_line) => Some(meta_line.meta.cwd.clone()),
        _ => None,
    })
}

fn multi_agent_version_from_items(
    items: &[RolloutItem],
    thread_id: Option<ThreadId>,
) -> Option<MultiAgentVersion> {
    let session_meta_version = items.iter().rev().find_map(|item| match item {
        RolloutItem::SessionMeta(meta_line)
            if thread_id.is_none_or(|thread_id| meta_line.meta.id == thread_id) =>
        {
            meta_line.meta.multi_agent_version
        }
        _ => None,
    });

    session_meta_version.or_else(|| {
        items.iter().rev().find_map(|item| match item {
            RolloutItem::TurnContext(turn_context) => turn_context.multi_agent_version,
            RolloutItem::SessionMeta(_)
            | RolloutItem::ResponseItem(_)
            | RolloutItem::InterAgentCommunication(_)
            | RolloutItem::InterAgentCommunicationMetadata { .. }
            | RolloutItem::Compacted(_)
            | RolloutItem::WorldState(_)
            | RolloutItem::EventMsg(_) => None,
        })
    })
}
