use super::*;

impl ThreadHistoryBuilder {
    pub(super) fn handle_response_item(&mut self, item: &codex_protocol::models::ResponseItem) {
        let codex_protocol::models::ResponseItem::Message {
            role, content, id, ..
        } = item
        else {
            return;
        };

        if role != "user" {
            return;
        }

        let Some(hook_prompt) = parse_hook_prompt_message(id.as_deref(), content) else {
            return;
        };

        self.push_item_in_current_turn(ThreadItem::HookPrompt {
            id: hook_prompt.id,
            fragments: hook_prompt
                .fragments
                .into_iter()
                .map(crate::protocol::v2::HookPromptFragment::from)
                .collect(),
        });
    }

    pub(super) fn handle_user_message(&mut self, payload: &UserMessageEvent) {
        // User messages should stay in explicitly opened turns. For backward
        // compatibility with older streams that did not open turns explicitly,
        // close any implicit/inactive turn and start a fresh one for this input.
        if let Some(turn) = self.current_turn.as_ref()
            && !turn.opened_explicitly
            && !(turn.saw_compaction && turn.items.is_empty())
        {
            self.finish_current_turn();
        }
        let id = self.next_item_id();
        let content = self.build_user_inputs(payload);
        self.push_item_in_current_turn(ThreadItem::UserMessage {
            id,
            client_id: payload.client_id.clone(),
            content,
        });
    }

    pub(super) fn handle_agent_message(
        &mut self,
        text: String,
        phase: Option<MessagePhase>,
        memory_citation: Option<crate::protocol::v2::MemoryCitation>,
    ) {
        if text.is_empty() {
            return;
        }

        let id = self.next_item_id();
        self.push_item_in_current_turn(ThreadItem::AgentMessage {
            id,
            text,
            phase,
            memory_citation,
        });
    }

    pub(super) fn handle_agent_reasoning(&mut self, payload: &AgentReasoningEvent) {
        if payload.text.is_empty() {
            return;
        }

        // If the last item is a reasoning item, add the new text to the summary.
        let existing_item_change = {
            let tracking_changes = self.is_tracking_changes();
            let turn = self.ensure_turn();
            if let Some(ThreadItem::Reasoning { summary, .. }) = turn.items.last_mut() {
                summary.push(payload.text.clone());
                let changed_item = if tracking_changes {
                    turn.items
                        .last()
                        .cloned()
                        .map(|item| (turn.id.clone(), item))
                } else {
                    None
                };
                Some(changed_item)
            } else {
                None
            }
        };
        if let Some(changed_item) = existing_item_change {
            if let Some((turn_id, item)) = changed_item {
                self.record_changed_item(turn_id, item);
            }
            return;
        }

        // Otherwise, create a new reasoning item.
        let id = self.next_item_id();
        self.push_item_in_current_turn(ThreadItem::Reasoning {
            id,
            summary: vec![payload.text.clone()],
            content: Vec::new(),
        });
    }

    pub(super) fn handle_agent_reasoning_raw_content(
        &mut self,
        payload: &AgentReasoningRawContentEvent,
    ) {
        if payload.text.is_empty() {
            return;
        }

        // If the last item is a reasoning item, add the new text to the content.
        let existing_item_change = {
            let tracking_changes = self.is_tracking_changes();
            let turn = self.ensure_turn();
            if let Some(ThreadItem::Reasoning { content, .. }) = turn.items.last_mut() {
                content.push(payload.text.clone());
                let changed_item = if tracking_changes {
                    turn.items
                        .last()
                        .cloned()
                        .map(|item| (turn.id.clone(), item))
                } else {
                    None
                };
                Some(changed_item)
            } else {
                None
            }
        };
        if let Some(changed_item) = existing_item_change {
            if let Some((turn_id, item)) = changed_item {
                self.record_changed_item(turn_id, item);
            }
            return;
        }

        // Otherwise, create a new reasoning item.
        let id = self.next_item_id();
        self.push_item_in_current_turn(ThreadItem::Reasoning {
            id,
            summary: Vec::new(),
            content: vec![payload.text.clone()],
        });
    }

    pub(super) fn handle_item_started(&mut self, payload: &ItemStartedEvent) {
        self.handle_materialized_item_lifecycle(&payload.turn_id, &payload.item);
    }

    pub(super) fn handle_item_completed(&mut self, payload: &ItemCompletedEvent) {
        self.handle_materialized_item_lifecycle(&payload.turn_id, &payload.item);
    }

    fn handle_materialized_item_lifecycle(
        &mut self,
        turn_id: &str,
        item: &codex_protocol::items::TurnItem,
    ) {
        let is_review_mode_item = matches!(
            item,
            codex_protocol::items::TurnItem::EnteredReviewMode(_)
                | codex_protocol::items::TurnItem::ExitedReviewMode(_)
        );
        let should_upsert = match item {
            codex_protocol::items::TurnItem::Plan(plan) => !plan.text.is_empty(),
            codex_protocol::items::TurnItem::Sleep(_)
            | codex_protocol::items::TurnItem::HookPrompt(_)
            | codex_protocol::items::TurnItem::CommandExecution(_)
            | codex_protocol::items::TurnItem::DynamicToolCall(_)
            | codex_protocol::items::TurnItem::CollabAgentToolCall(_)
            | codex_protocol::items::TurnItem::SubAgentActivity(_)
            | codex_protocol::items::TurnItem::Extension(_)
            | codex_protocol::items::TurnItem::EnteredReviewMode(_)
            | codex_protocol::items::TurnItem::ExitedReviewMode(_) => true,
            codex_protocol::items::TurnItem::UserMessage(_)
            | codex_protocol::items::TurnItem::AgentMessage(_)
            | codex_protocol::items::TurnItem::Reasoning(_)
            | codex_protocol::items::TurnItem::WebSearch(_)
            | codex_protocol::items::TurnItem::ImageView(_)
            | codex_protocol::items::TurnItem::ImageGeneration(_)
            | codex_protocol::items::TurnItem::FileChange(_)
            | codex_protocol::items::TurnItem::McpToolCall(_)
            | codex_protocol::items::TurnItem::ContextCompaction(_) => false,
        };

        if should_upsert {
            let item = ThreadItem::from(item.clone());
            if is_review_mode_item {
                self.upsert_review_mode_item(Some(turn_id), item);
            } else {
                self.upsert_item_in_turn_id(turn_id, item);
            }
        }
    }
}
