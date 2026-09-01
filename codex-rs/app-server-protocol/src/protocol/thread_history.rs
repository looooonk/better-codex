mod collaboration_event_handlers;
mod item_event_handlers;
mod message_event_handlers;
mod turn_event_handlers;

use crate::protocol::item_builders::build_command_execution_begin_item;
use crate::protocol::item_builders::build_command_execution_end_item;
use crate::protocol::item_builders::build_file_change_approval_request_item;
use crate::protocol::item_builders::build_file_change_begin_item;
use crate::protocol::item_builders::build_file_change_end_item;
use crate::protocol::item_builders::build_item_from_guardian_event;
use crate::protocol::item_builders::review_output_text;
use crate::protocol::v2::CollabAgentState;
use crate::protocol::v2::CollabAgentTool;
use crate::protocol::v2::CollabAgentToolCallStatus;
use crate::protocol::v2::CommandExecutionStatus;
use crate::protocol::v2::DynamicToolCallOutputContentItem;
use crate::protocol::v2::DynamicToolCallStatus;
use crate::protocol::v2::McpToolCallAppContext;
use crate::protocol::v2::McpToolCallError;
use crate::protocol::v2::McpToolCallResult;
use crate::protocol::v2::McpToolCallStatus;
use crate::protocol::v2::ThreadItem;
use crate::protocol::v2::Turn;
use crate::protocol::v2::TurnError as V2TurnError;
use crate::protocol::v2::TurnError;
use crate::protocol::v2::TurnItemsView;
use crate::protocol::v2::TurnStatus;
use crate::protocol::v2::UserInput;
#[cfg(test)]
use crate::protocol::v2::WebSearchAction;
use crate::protocol::v2::WebSearchItem;
use crate::protocol::v2::web_search_action_from_core;
use codex_extension_items::image_generation::ImageGenerationItem;
use codex_history::CompactedItem;
use codex_history::RolloutItem;
use codex_protocol::items::parse_hook_prompt_message;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::AgentReasoningEvent;
use codex_protocol::protocol::AgentReasoningRawContentEvent;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::ApplyPatchApprovalRequestEvent;
use codex_protocol::protocol::ContextCompactedEvent;
use codex_protocol::protocol::DynamicToolCallResponseEvent;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ExecCommandBeginEvent;
use codex_protocol::protocol::ExecCommandEndEvent;
use codex_protocol::protocol::GuardianAssessmentEvent;
use codex_protocol::protocol::GuardianAssessmentStatus;
use codex_protocol::protocol::ImageGenerationBeginEvent;
use codex_protocol::protocol::ImageGenerationEndEvent;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ItemStartedEvent;
use codex_protocol::protocol::McpToolCallBeginEvent;
use codex_protocol::protocol::McpToolCallEndEvent;
use codex_protocol::protocol::PatchApplyBeginEvent;
use codex_protocol::protocol::PatchApplyEndEvent;
use codex_protocol::protocol::ThreadRolledBackEvent;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::UserMessageEvent;
use codex_protocol::protocol::ViewImageToolCallEvent;
use codex_protocol::protocol::WebSearchBeginEvent;
use codex_protocol::protocol::WebSearchEndEvent;
#[cfg(test)]
use codex_protocol::review_format::REVIEW_FALLBACK_MESSAGE;
use std::collections::HashMap;
use tracing::warn;
use uuid::Uuid;

#[cfg(test)]
use crate::protocol::v2::CommandAction;
#[cfg(test)]
use crate::protocol::v2::FileUpdateChange;
#[cfg(test)]
use crate::protocol::v2::PatchApplyStatus;
#[cfg(test)]
use crate::protocol::v2::PatchChangeKind;
#[cfg(test)]
use codex_protocol::protocol::ExecCommandStatus as CoreExecCommandStatus;
#[cfg(test)]
use codex_protocol::protocol::PatchApplyStatus as CorePatchApplyStatus;

/// Convert persisted [`RolloutItem`] entries into a sequence of [`Turn`] values.
///
/// When available, this uses `TurnContext.turn_id` as the canonical turn id so
/// resumed/rebuilt thread history preserves the original turn identifiers.
pub fn build_turns_from_rollout_items(items: &[RolloutItem]) -> Vec<Turn> {
    let mut builder = ThreadHistoryBuilder::new();
    for item in items {
        builder.handle_rollout_item(item);
    }
    builder.finish()
}

/// A materialized `ThreadItem` snapshot that changed while handling one input.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadHistoryItemChange {
    pub turn_id: String,
    pub item: ThreadItem,
}

/// Lightweight turn metadata snapshot for projectors that track turn status without
/// re-reading the full item list.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadHistoryTurnChange {
    pub turn_id: String,
    pub status: TurnStatus,
    pub abort_reason: Option<TurnAbortReason>,
    pub error: Option<TurnError>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub duration_ms: Option<i64>,
}

/// Incremental changes produced by opt-in `ThreadHistoryBuilder` handlers.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ThreadHistoryChangeSet {
    pub changed_items: Vec<ThreadHistoryItemChange>,
    pub changed_turns: Vec<ThreadHistoryTurnChange>,
    pub removed_turn_ids: Vec<String>,
}

impl ThreadHistoryChangeSet {
    pub fn is_empty(&self) -> bool {
        self.changed_items.is_empty()
            && self.changed_turns.is_empty()
            && self.removed_turn_ids.is_empty()
    }
}

impl ThreadHistoryTurnChange {
    fn from_pending_turn(turn: &PendingTurn) -> Self {
        Self {
            turn_id: turn.id.clone(),
            status: turn.status.clone(),
            abort_reason: None,
            error: turn.error.clone(),
            started_at: turn.started_at,
            completed_at: turn.completed_at,
            duration_ms: turn.duration_ms,
        }
    }

    fn from_turn(turn: &Turn) -> Self {
        Self {
            turn_id: turn.id.clone(),
            status: turn.status.clone(),
            abort_reason: None,
            error: turn.error.clone(),
            started_at: turn.started_at,
            completed_at: turn.completed_at,
            duration_ms: turn.duration_ms,
        }
    }
}

/// Coalesces per-rollout-item changes into an end-of-batch view. It preserves
/// first-change order while replacing repeated item/turn snapshots with their
/// latest value, and drops accumulated changes for turns removed by rollback.
#[derive(Default)]
struct ThreadHistoryChangeAccumulator {
    changed_items: Vec<Option<ThreadHistoryItemChange>>,
    changed_item_indexes: HashMap<(String, String), usize>,
    changed_turns: Vec<Option<ThreadHistoryTurnChange>>,
    changed_turn_indexes: HashMap<String, usize>,
    removed_turn_ids: Vec<String>,
    removed_turn_indexes: HashMap<String, usize>,
}

impl ThreadHistoryChangeAccumulator {
    fn push(&mut self, changes: ThreadHistoryChangeSet) {
        for turn_id in changes.removed_turn_ids {
            self.push_removed_turn_id(turn_id);
        }
        for item_change in changes.changed_items {
            self.push_item_change(item_change);
        }
        for turn_change in changes.changed_turns {
            self.push_turn_change(turn_change);
        }
    }

    fn finish(self) -> ThreadHistoryChangeSet {
        ThreadHistoryChangeSet {
            changed_items: self.changed_items.into_iter().flatten().collect(),
            changed_turns: self.changed_turns.into_iter().flatten().collect(),
            removed_turn_ids: self.removed_turn_ids,
        }
    }

    fn push_item_change(&mut self, change: ThreadHistoryItemChange) {
        let key = (change.turn_id.clone(), change.item.id().to_string());
        if let Some(index) = self.changed_item_indexes.get(&key).copied() {
            self.changed_items[index] = Some(change);
            return;
        }

        self.changed_item_indexes
            .insert(key, self.changed_items.len());
        self.changed_items.push(Some(change));
    }

    fn push_turn_change(&mut self, change: ThreadHistoryTurnChange) {
        if let Some(index) = self.changed_turn_indexes.get(&change.turn_id).copied() {
            self.changed_turns[index] = Some(change);
            return;
        }

        self.changed_turn_indexes
            .insert(change.turn_id.clone(), self.changed_turns.len());
        self.changed_turns.push(Some(change));
    }

    fn push_removed_turn_id(&mut self, turn_id: String) {
        if !self.removed_turn_indexes.contains_key(&turn_id) {
            self.removed_turn_indexes
                .insert(turn_id.clone(), self.removed_turn_ids.len());
            self.removed_turn_ids.push(turn_id.clone());
        }

        if let Some(index) = self.changed_turn_indexes.remove(&turn_id) {
            self.changed_turns[index] = None;
        }

        let removed_item_keys: Vec<(String, String)> = self
            .changed_item_indexes
            .keys()
            .filter(|(item_turn_id, _)| item_turn_id == &turn_id)
            .cloned()
            .collect();
        for key in removed_item_keys {
            if let Some(index) = self.changed_item_indexes.remove(&key) {
                self.changed_items[index] = None;
            }
        }
    }
}

pub struct ThreadHistoryBuilder {
    turns: Vec<Turn>,
    current_turn: Option<PendingTurn>,
    next_item_index: i64,
    current_rollout_index: usize,
    next_rollout_index: usize,
    active_change_set: Option<ThreadHistoryChangeSet>,
}

impl Default for ThreadHistoryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadHistoryBuilder {
    pub fn new() -> Self {
        Self {
            turns: Vec::new(),
            current_turn: None,
            next_item_index: 1,
            current_rollout_index: 0,
            next_rollout_index: 0,
            active_change_set: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn finish(mut self) -> Vec<Turn> {
        self.finish_current_turn();
        self.turns
    }

    pub fn active_turn_snapshot(&self) -> Option<Turn> {
        self.current_turn
            .as_ref()
            .map(Turn::from)
            .or_else(|| self.turns.last().cloned())
    }

    pub fn turn_snapshot(&self, turn_id: &str) -> Option<Turn> {
        self.current_turn
            .as_ref()
            .filter(|turn| turn.id == turn_id)
            .map(Turn::from)
            .or_else(|| self.turns.iter().find(|turn| turn.id == turn_id).cloned())
    }

    /// Returns the index of the active turn snapshot within the finished turn list.
    ///
    /// When a turn is still open, this is the index it will occupy after
    /// `finish`. When no turn is open, it is the index of the last finished turn.
    pub fn active_turn_position(&self) -> Option<usize> {
        if self.current_turn.is_some() {
            Some(self.turns.len())
        } else if self.turns.is_empty() {
            None
        } else {
            Some(self.turns.len() - 1)
        }
    }

    pub fn has_active_turn(&self) -> bool {
        self.current_turn.is_some()
    }

    pub fn active_turn_id_if_explicit(&self) -> Option<String> {
        self.current_turn
            .as_ref()
            .filter(|turn| turn.opened_explicitly)
            .map(|turn| turn.id.clone())
    }

    pub fn active_turn_start_index(&self) -> Option<usize> {
        self.current_turn
            .as_ref()
            .map(|turn| turn.rollout_start_index)
    }

    /// Shared reducer for persisted rollout replay and in-memory current-turn
    /// tracking used by running thread resume/rejoin.
    ///
    /// This function should handle all EventMsg variants that can be persisted in a rollout file.
    /// See `should_persist_event_msg` in `codex-rs/core/rollout/policy.rs`.
    pub fn handle_event(&mut self, event: &EventMsg) {
        match event {
            EventMsg::UserMessage(payload) => self.handle_user_message(payload),
            EventMsg::AgentMessage(payload) => self.handle_agent_message(
                payload.message.clone(),
                payload.phase.clone(),
                payload.memory_citation.clone().map(Into::into),
            ),
            EventMsg::AgentReasoning(payload) => self.handle_agent_reasoning(payload),
            EventMsg::AgentReasoningRawContent(payload) => {
                self.handle_agent_reasoning_raw_content(payload)
            }
            EventMsg::WebSearchBegin(payload) => self.handle_web_search_begin(payload),
            EventMsg::WebSearchEnd(payload) => self.handle_web_search_end(payload),
            EventMsg::ExecCommandBegin(payload) => self.handle_exec_command_begin(payload),
            EventMsg::ExecCommandEnd(payload) => self.handle_exec_command_end(payload),
            EventMsg::GuardianAssessment(payload) => self.handle_guardian_assessment(payload),
            EventMsg::ApplyPatchApprovalRequest(payload) => {
                self.handle_apply_patch_approval_request(payload)
            }
            EventMsg::PatchApplyBegin(payload) => self.handle_patch_apply_begin(payload),
            EventMsg::PatchApplyEnd(payload) => self.handle_patch_apply_end(payload),
            EventMsg::DynamicToolCallRequest(payload) => {
                self.handle_dynamic_tool_call_request(payload)
            }
            EventMsg::DynamicToolCallResponse(payload) => {
                self.handle_dynamic_tool_call_response(payload)
            }
            EventMsg::McpToolCallBegin(payload) => self.handle_mcp_tool_call_begin(payload),
            EventMsg::McpToolCallEnd(payload) => self.handle_mcp_tool_call_end(payload),
            EventMsg::ViewImageToolCall(payload) => self.handle_view_image_tool_call(payload),
            EventMsg::ImageGenerationBegin(payload) => self.handle_image_generation_begin(payload),
            EventMsg::ImageGenerationEnd(payload) => self.handle_image_generation_end(payload),
            EventMsg::CollabAgentSpawnBegin(payload) => {
                self.handle_collab_agent_spawn_begin(payload)
            }
            EventMsg::CollabAgentSpawnEnd(payload) => self.handle_collab_agent_spawn_end(payload),
            EventMsg::CollabAgentInteractionBegin(payload) => {
                self.handle_collab_agent_interaction_begin(payload)
            }
            EventMsg::CollabAgentInteractionEnd(payload) => {
                self.handle_collab_agent_interaction_end(payload)
            }
            EventMsg::SubAgentActivity(payload) => self.handle_sub_agent_activity(payload),
            EventMsg::CollabWaitingBegin(payload) => self.handle_collab_waiting_begin(payload),
            EventMsg::CollabWaitingEnd(payload) => self.handle_collab_waiting_end(payload),
            EventMsg::CollabCloseBegin(payload) => self.handle_collab_close_begin(payload),
            EventMsg::CollabCloseEnd(payload) => self.handle_collab_close_end(payload),
            EventMsg::CollabResumeBegin(payload) => self.handle_collab_resume_begin(payload),
            EventMsg::CollabResumeEnd(payload) => self.handle_collab_resume_end(payload),
            EventMsg::ContextCompacted(payload) => self.handle_context_compacted(payload),
            EventMsg::EnteredReviewMode(payload) => self.handle_entered_review_mode(payload),
            EventMsg::ExitedReviewMode(payload) => self.handle_exited_review_mode(payload),
            EventMsg::ItemStarted(payload) => self.handle_item_started(payload),
            EventMsg::ItemCompleted(payload) => self.handle_item_completed(payload),
            EventMsg::HookStarted(_) | EventMsg::HookCompleted(_) => {}
            EventMsg::Error(payload) => self.handle_error(payload),
            EventMsg::TokenCount(_) => {}
            EventMsg::ThreadRolledBack(payload) => self.handle_thread_rollback(payload),
            EventMsg::TurnAborted(payload) => self.handle_turn_aborted(payload),
            EventMsg::TurnStarted(payload) => self.handle_turn_started(payload),
            EventMsg::TurnComplete(payload) => self.handle_turn_complete(payload),
            _ => {}
        }
    }

    pub fn handle_rollout_item(&mut self, item: &RolloutItem) {
        self.current_rollout_index = self.next_rollout_index;
        self.next_rollout_index += 1;
        match item {
            RolloutItem::EventMsg(event) => self.handle_event(event),
            RolloutItem::Compacted(payload) => self.handle_compacted(payload),
            RolloutItem::ResponseItem(item) => self.handle_response_item(&item.item),
            RolloutItem::InterAgentCommunication(_)
            | RolloutItem::InterAgentCommunicationMetadata { .. }
            | RolloutItem::TurnContext(_)
            | RolloutItem::WorldState(_)
            | RolloutItem::SecurityRiskScore(_)
            | RolloutItem::SessionMeta(_) => {}
        }
    }

    /// Handles one event and returns the materialized items or turn metadata
    /// changed by that event.
    pub fn handle_event_with_changes(&mut self, event: &EventMsg) -> ThreadHistoryChangeSet {
        self.collect_changes(|builder| builder.handle_event(event))
    }

    /// Handles a rollout item and returns the materialized items or turn metadata
    /// changed by that one append.
    pub fn handle_rollout_item_with_changes(
        &mut self,
        item: &RolloutItem,
    ) -> ThreadHistoryChangeSet {
        self.collect_changes(|builder| builder.handle_rollout_item(item))
    }

    /// Handles rollout items in order and returns a coalesced end-of-batch
    /// change set. Multiple changes to the same item or turn are deduplicated
    /// so only the latest snapshot is emitted.
    pub fn handle_rollout_items_with_changes(
        &mut self,
        items: &[RolloutItem],
    ) -> ThreadHistoryChangeSet {
        let mut accumulator = ThreadHistoryChangeAccumulator::default();
        for item in items {
            accumulator.push(self.handle_rollout_item_with_changes(item));
        }
        accumulator.finish()
    }

    fn collect_changes(&mut self, handle: impl FnOnce(&mut Self)) -> ThreadHistoryChangeSet {
        debug_assert!(self.active_change_set.is_none());
        self.active_change_set = Some(ThreadHistoryChangeSet::default());
        handle(self);
        self.active_change_set.take().unwrap_or_default()
    }

    fn finish_current_turn(&mut self) {
        if let Some(turn) = self.current_turn.take() {
            if turn.items.is_empty() && !turn.opened_explicitly && !turn.saw_compaction {
                return;
            }
            self.turns.push(Turn::from(turn));
        }
    }

    fn new_turn(&mut self, id: Option<String>) -> PendingTurn {
        let id = id.unwrap_or_else(|| {
            if self.next_rollout_index == 0 {
                Uuid::now_v7().to_string()
            } else {
                format!("rollout-{}", self.current_rollout_index)
            }
        });
        PendingTurn {
            id,
            items: Vec::new(),
            error: None,
            status: TurnStatus::Completed,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            opened_explicitly: false,
            saw_compaction: false,
            rollout_start_index: self.current_rollout_index,
        }
    }

    fn ensure_turn(&mut self) -> &mut PendingTurn {
        if self.current_turn.is_none() {
            let turn = self.new_turn(/*id*/ None);
            self.record_changed_pending_turn(&turn);
            self.current_turn = Some(turn);
        }

        if let Some(turn) = self.current_turn.as_mut() {
            return turn;
        }

        unreachable!("current turn must exist after initialization");
    }

    fn push_item_in_current_turn(&mut self, item: ThreadItem) {
        let tracking_changes = self.is_tracking_changes();
        let changed_item = {
            let turn = self.ensure_turn();
            let changed_item = tracking_changes.then(|| (turn.id.clone(), item.clone()));
            turn.items.push(item);
            changed_item
        };
        if let Some((turn_id, item)) = changed_item {
            self.record_changed_item(turn_id, item);
        }
    }

    fn upsert_item_in_turn_id(&mut self, turn_id: &str, item: ThreadItem) {
        let tracking_changes = self.is_tracking_changes();
        if let Some(turn) = self.current_turn.as_mut()
            && turn.id == turn_id
        {
            let changed_item = {
                let item = upsert_turn_item(&mut turn.items, item);
                tracking_changes.then(|| (turn.id.clone(), item.clone()))
            };
            if let Some((turn_id, item)) = changed_item {
                self.record_changed_item(turn_id, item);
            }
            return;
        }

        if let Some(turn) = self.turns.iter_mut().find(|turn| turn.id == turn_id) {
            let changed_item = {
                let item = upsert_turn_item(&mut turn.items, item);
                tracking_changes.then(|| (turn.id.clone(), item.clone()))
            };
            if let Some((turn_id, item)) = changed_item {
                self.record_changed_item(turn_id, item);
            }
            return;
        }

        warn!(
            item_id = item.id(),
            "dropping turn-scoped item for unknown turn id `{turn_id}`"
        );
    }

    fn upsert_item_in_current_turn(&mut self, item: ThreadItem) {
        let tracking_changes = self.is_tracking_changes();
        let changed_item = {
            let turn = self.ensure_turn();
            let item = upsert_turn_item(&mut turn.items, item);
            tracking_changes.then(|| (turn.id.clone(), item.clone()))
        };
        if let Some((turn_id, item)) = changed_item {
            self.record_changed_item(turn_id, item);
        }
    }

    fn is_tracking_changes(&self) -> bool {
        self.active_change_set.is_some()
    }

    fn record_changed_item(&mut self, turn_id: String, item: ThreadItem) {
        if let Some(change_set) = self.active_change_set.as_mut() {
            change_set
                .changed_items
                .push(ThreadHistoryItemChange { turn_id, item });
        }
    }

    fn record_changed_pending_turn(&mut self, turn: &PendingTurn) {
        if self.is_tracking_changes() {
            self.record_changed_turn(ThreadHistoryTurnChange::from_pending_turn(turn));
        }
    }

    fn record_changed_turn(&mut self, turn: ThreadHistoryTurnChange) {
        if let Some(change_set) = self.active_change_set.as_mut() {
            change_set.changed_turns.push(turn);
        }
    }

    fn record_removed_turn_ids(&mut self, removed_turn_ids: Vec<String>) {
        if let Some(change_set) = self.active_change_set.as_mut() {
            change_set.removed_turn_ids.extend(removed_turn_ids);
        }
    }

    fn next_item_id(&mut self) -> String {
        let id = format!("item-{}", self.next_item_index);
        self.next_item_index += 1;
        id
    }

    fn build_user_inputs(&self, payload: &UserMessageEvent) -> Vec<UserInput> {
        let mut content = Vec::new();
        if !payload.message.trim().is_empty() {
            content.push(UserInput::Text {
                text: payload.message.clone(),
                text_elements: payload
                    .text_elements
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect(),
            });
        }
        if let Some(images) = &payload.images {
            for (idx, image) in images.iter().enumerate() {
                content.push(UserInput::Image {
                    url: image.clone(),
                    detail: payload.image_details.get(idx).copied().flatten(),
                });
            }
        }
        for (idx, path) in payload.local_images.iter().enumerate() {
            content.push(UserInput::LocalImage {
                path: path.clone(),
                detail: payload.local_image_details.get(idx).copied().flatten(),
            });
        }
        content
    }
}

fn convert_dynamic_tool_content_items(
    items: &[codex_protocol::dynamic_tools::DynamicToolCallOutputContentItem],
) -> Vec<DynamicToolCallOutputContentItem> {
    items
        .iter()
        .cloned()
        .map(|item| match item {
            codex_protocol::dynamic_tools::DynamicToolCallOutputContentItem::InputText { text } => {
                DynamicToolCallOutputContentItem::InputText { text }
            }
            codex_protocol::dynamic_tools::DynamicToolCallOutputContentItem::InputImage {
                image_url,
            } => DynamicToolCallOutputContentItem::InputImage { image_url },
        })
        .collect()
}

fn upsert_turn_item(items: &mut Vec<ThreadItem>, item: ThreadItem) -> &ThreadItem {
    if let Some(existing_item_index) = items
        .iter()
        .position(|existing_item| existing_item.id() == item.id())
    {
        items[existing_item_index] = item;
        return &items[existing_item_index];
    }
    let inserted_item_index = items.len();
    items.push(item);
    &items[inserted_item_index]
}

struct PendingTurn {
    id: String,
    items: Vec<ThreadItem>,
    error: Option<TurnError>,
    status: TurnStatus,
    started_at: Option<i64>,
    completed_at: Option<i64>,
    duration_ms: Option<i64>,
    /// True when this turn originated from an explicit `turn_started`/`turn_complete`
    /// boundary, so we preserve it even if it has no renderable items.
    opened_explicitly: bool,
    /// True when this turn includes a persisted `RolloutItem::Compacted`, which
    /// should keep the turn from being dropped even without normal items.
    saw_compaction: bool,
    /// Index of the rollout item that opened this turn during replay.
    rollout_start_index: usize,
}

impl PendingTurn {
    fn opened_explicitly(mut self) -> Self {
        self.opened_explicitly = true;
        self
    }

    fn with_status(mut self, status: TurnStatus) -> Self {
        self.status = status;
        self
    }

    fn with_started_at(mut self, started_at: Option<i64>) -> Self {
        self.started_at = started_at;
        self
    }
}

impl From<PendingTurn> for Turn {
    fn from(value: PendingTurn) -> Self {
        Self {
            id: value.id,
            items: value.items,
            items_view: TurnItemsView::Full,
            error: value.error,
            status: value.status,
            started_at: value.started_at,
            completed_at: value.completed_at,
            duration_ms: value.duration_ms,
        }
    }
}

impl From<&PendingTurn> for Turn {
    fn from(value: &PendingTurn) -> Self {
        Self {
            id: value.id.clone(),
            items: value.items.clone(),
            items_view: TurnItemsView::Full,
            error: value.error.clone(),
            status: value.status.clone(),
            started_at: value.started_at,
            completed_at: value.completed_at,
            duration_ms: value.duration_ms,
        }
    }
}

#[cfg(test)]
#[path = "thread_history_tests.rs"]
mod tests;
